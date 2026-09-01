use std::collections::BTreeMap;
use std::sync::Arc;

use prost::Message;
use rlogs_events::{
    AbilityId, ActorEvent, ActorId, ActorKind, ActorLoadoutEvidence, ActorLoadoutObservation,
    ActorLoadoutSlot, ActorOwnershipUpdate, ActorState, BoundaryReason, CanonicalEventDraft,
    CanonicalEventDraftKind, CastEvent, CastState, CharacterIdentity, CooldownEvent, DamageEvent,
    DamageFlags, DataGapEvent, DataGapKind, DungeonEvent, DungeonEventKind, DungeonFlowPhase,
    DungeonFlowSnapshot, DungeonObjectiveCatalogReference, DungeonObjectiveCatalogResolution,
    EncounterState, EntityAttribute, EntityAttributeEvent, EntityAttributeUpdateKind,
    EntityAttributeValue, EntityRef, EntityUuid, EventEnvelope, EventEnvelopeFactory,
    EventProvenance, EventSensitivity, EventTime, HealingEvent, LifeState, MonsterId,
    PartyRosterEvent, PartyRosterMember, PartyRosterObservation, PositionEvent, RegionContext,
    RegionEvidence, RegionEvidenceKind, RegionIdentity, ResourceCooldown, ResourceEvent, RunState,
    SceneId, StatusEffectId, StatusEffectInstanceId, StatusEvent, StatusOrigin, StatusState,
    TemporaryAttribute, TemporaryAttributeEvent, TimelineEventKind, UnresolvedActionEvent,
    UnresolvedActionReason, UnresolvedStatusEvent, UnresolvedStatusReason, WorldContext,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dirty_blob_v1;
use crate::dungeon_dirty_v1::{self, DirtyDungeonObjectiveMutation};
use crate::game_schema_v1 as schema;
use crate::specialization_detection::{
    SpecializationAbilityEvidenceStrength, specialization_ability_evidence,
    specialization_from_evidence, specialization_from_passive_selectors,
    specialization_identity_from_passive_selectors,
};
use crate::{
    AchievementProgress, AchievementProgressProfile, ActivityProgress, AllowedDataDomain,
    BattleImagineSkill, CaptureGapKind, CaptureRecord, CaptureRecordKind, CharacterAppearance,
    CharacterProfilePatch, CharacterProgression, CollectionSummary, CombatPowerBreakdown,
    CombatPowerComponent, CombatPowerSubcomponent, CombatProfessionProfile, CultivationAreaProfile,
    CultivationLineProfile, DecodeDisposition, DungeonProgress, DungeonTargetProgress,
    EquipmentAttributeProfile, EquipmentEnchantmentProfile, EquipmentItem,
    EquipmentSuitEntryProfile, EquippedActionSlot, GameBuild, HandbookProgress,
    LifeProfessionProfile, MasterModeDungeonProgress, ModuleItemProfile, ModulePartProfile,
    ModuleProfile, ModuleUpgradeRecord, ObjectiveCatalogResolver, ProhibitedDataClass,
    ProtocolPack, ReputationProgress, RgbColor, SeasonAchievementProgress,
    SeasonCultivationProfile, SeasonMedalHole, SeasonMedalNode, SeasonMedalProfile, SeasonProfile,
    SkillLevel, SocialDisplay, TalentLevel, TalentProgressProfile, WeeklyTowerProgress,
    auxiliary_action_presentation, battle_imagine_presentation, character_id_from_entity_uuid,
    normalize_auxiliary_imagine_tier, project_actor_loadouts,
};

const ATTR_NAME: i32 = 0x01;
const ATTR_MONSTER_ID: i32 = 0x0a;
const ATTR_POSITION: i32 = 0x34;
// Exact build 24687926 `Zproto.EAttrType::AttrTargetPos`. The numeric ID is
// authoritative; the native enum name is retained only as build-locked
// semantic evidence. Its payload uses the same `schema::Position` wire shape
// as `AttrPos`.
const ATTR_TARGET_POSITION: i32 = 0x35;
const ATTR_CLASS_ID: i32 = 0xdc;
const ATTR_LEVEL: i32 = 0x2710;
const ATTR_COMBAT_POWER: i32 = 0x272e;
const ATTR_SEASON_STRENGTH: i32 = 0x2cb0;
const ATTR_SCENE_ID: i32 = 0x155;
const ATTR_SCENE_LINE: i32 = 0x157;
const ATTR_ACTIVE_SKILL_LEVELS: i32 = 116;
const ATTR_EQUIPMENT: i32 = 200;
const ATTR_ACTION_SLOTS: i32 = 226;
/// `EAttrType::AttrBreakingStage`. The scalar payload uses
/// `EBreakingStage` (`0 = Breaking`, `1 = BreakEnd`) and is retained as the
/// exact enum integer so game-specific reducers can reconstruct target-state
/// predicates without reparsing packet archives.
const ATTR_BREAKING_STAGE: i32 = 455;
// Summoned combat entities carry their owning player's stable entity UUID in
// two independently encoded attributes. Both values must agree before any
// consumer treats the relationship as exact.
const ATTR_SUMMON_OWNER_PRIMARY: i32 = 90;
const ATTR_SUMMON_OWNER_CONFIRMATION: i32 = 91;
pub(crate) const ATTR_CURRENT_HP: i32 = 11310;
const ATTR_MAX_HP_FINAL: i32 = 11320;
const ATTR_MAX_HP_TOTAL: i32 = 11321;
const ATTR_MAX_HP_ADD: i32 = 11322;
pub(crate) const ATTR_MAX_HP_EXTRA_ADD: i32 = 11323;
const ATTR_MAX_HP_PERCENT: i32 = 11324;
const ATTR_MAX_HP_EXTRA_PERCENT: i32 = 11325;
// Exact current-build `EAttrType` formula inputs. Packet replay proves these
// attributes use protobuf scalar-varint payloads just like HP and resources:
// physical attack `11330` and Mastery `11940` occur on the damage source, and
// magical attack `11340` is the sibling formula input used by magic classes.
// Decode them once here so live reducers, history, and validation consume the
// same canonical value instead of independently reparsing raw bytes.
pub(crate) const ATTR_PHYSICAL_ATTACK: i32 = 11330;
pub(crate) const ATTR_MAGICAL_ATTACK: i32 = 11340;
pub(crate) const ATTR_MASTERY: i32 = 11940;
const ATTR_CURRENT_ENERGY: i32 = 20010;
const ATTR_MAX_ENERGY_FINAL: i32 = 20020;
const ATTR_MAX_ENERGY_TOTAL: i32 = 20021;
const ATTR_MAX_ENERGY_ADD: i32 = 20022;
const ATTR_MAX_ENERGY_EXTRA_ADD: i32 = 20023;
const ATTR_MAX_ENERGY_PERCENT: i32 = 20024;
const ATTR_MAX_ENERGY_EXTRA_PERCENT: i32 = 20025;
const WEAPON_SLOT_ID: i32 = 200;
const PRIMARY_LOADOUT_SLOTS: std::ops::RangeInclusive<i32> = 7..=8;
const AUXILIARY_LOADOUT_SLOTS: std::ops::RangeInclusive<i32> = 21..=24;

const ENTITY_PLAYER: i32 = 10;
const MODULE_PACKAGE_ID: i32 = 5;
const DAMAGE_FLAG_CRITICAL: i32 = 0b0001;
const DAMAGE_FLAG_BLOCKED: i32 = 0b0010;
const DAMAGE_FLAG_CAUSES_LUCKY: i32 = 0b0100;
const BUFF_EVENT_REMOVE: i32 = 2;
const BUFF_EVENT_LAYER_CHANGE_5: i32 = 5;
const BUFF_EVENT_LAYER_CHANGE_6: i32 = 6;
const BUFF_LOGIC_ADD_BUFF: i32 = 18;
const BUFF_LOGIC_CHANGE: i32 = 19;

const DUNGEON_STATE_NULL: i32 = 0;
const DUNGEON_STATE_ACTIVE: i32 = 1;
const DUNGEON_STATE_READY: i32 = 2;
const DUNGEON_STATE_PLAYING: i32 = 3;
const DUNGEON_STATE_END: i32 = 4;
const DUNGEON_STATE_SETTLEMENT: i32 = 5;
const DUNGEON_STATE_VOTE: i32 = 6;
/// Current-build dungeon party-wipe recovery effect. The server applies this
/// to the local character after a failed encounter so the client can restore
/// the party and clear encounter cooldowns.
const DUNGEON_WIPE_RECOVERY_EFFECT_ID: i64 = 510_072;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecoderKind {
    NotifyEnterWorldV1,
    SyncServerTimeV1,
    SyncSeasonV1,
    SyncDungeonDataV1,
    SyncDungeonDirtyDataV1,
    EnterSceneV1,
    NotifyLoadSceneEndV1,
    SyncNearEntitiesV1,
    SyncContainerDataV1,
    SyncContainerDirtyDataV1,
    NotifySocialDataV1,
    NotifyTeamMemberInfoV1,
    NotifyJoinTeamV1,
    NotifyLeaveTeamV1,
    NoticeTeamDissolveV1,
    SyncNearDeltaV1,
    SyncToMeDeltaV1,
    NotifyReviveV1,
    SyncClientUseSkillV1,
    /// Exact build-locked client `World.UseSlot` gameplay request. A promoted
    /// protocol pack may register it only after matching-build packet replay
    /// confirms the statically proven service and method route on the wire.
    WorldUseSlotV1,
}

impl DecoderKind {
    pub const fn domain(self) -> AllowedDataDomain {
        match self {
            Self::NotifyEnterWorldV1
            | Self::SyncServerTimeV1
            | Self::EnterSceneV1
            | Self::NotifyLoadSceneEndV1 => AllowedDataDomain::WorldState,
            Self::SyncDungeonDataV1 | Self::SyncDungeonDirtyDataV1 => AllowedDataDomain::Encounter,
            Self::SyncNearEntitiesV1 => AllowedDataDomain::ActorState,
            Self::SyncContainerDataV1
            | Self::SyncContainerDirtyDataV1
            | Self::NotifySocialDataV1
            | Self::NotifyTeamMemberInfoV1
            | Self::NotifyJoinTeamV1
            | Self::NotifyLeaveTeamV1
            | Self::NoticeTeamDissolveV1
            | Self::SyncSeasonV1 => AllowedDataDomain::CharacterProfile,
            Self::SyncNearDeltaV1
            | Self::SyncToMeDeltaV1
            | Self::NotifyReviveV1
            | Self::SyncClientUseSkillV1
            | Self::WorldUseSlotV1 => AllowedDataDomain::Combat,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolRuntimeConfig {
    pub max_entities: usize,
    pub max_events_per_packet: usize,
}

impl Default for ProtocolRuntimeConfig {
    fn default() -> Self {
        Self {
            max_entities: 65_536,
            max_events_per_packet: 16_384,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolDecodeStatus {
    Decoded,
    CaptureGap,
    Unrouted,
    OpaqueLocalOnly,
    Prohibited(ProhibitedDataClass),
    MissingApplicationPayload,
    DecodeFailed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDecodeBatch {
    pub capture_sequence: u64,
    pub status: ProtocolDecodeStatus,
    pub events: Vec<EventEnvelope>,
    /// Local-only game-plugin metadata used to resolve a server realm. This is
    /// deliberately not a canonical event and therefore is not written to
    /// website-bound `.rlog` output.
    pub announced_server: Option<AnnouncedServerEndpoint>,
    pub server_clock: Option<ServerClockObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncedServerEndpoint {
    pub host: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerClockObservation {
    pub client_milliseconds: Option<i64>,
    pub server_milliseconds: i64,
}

/// Stateful bridge from one exact protocol pack to canonical plugin events.
///
/// Entity UUIDs remain exact while compact actor IDs are assigned once per
/// session. The entity registry is bounded independently from packet buffers.
pub struct ProtocolRuntime<'a> {
    pack: &'a ProtocolPack,
    client_build: String,
    entities: EntityRegistry,
    status_effects: StatusEffectRegistry,
    dungeon: DungeonTracker,
    profile: ProfileTracker,
    state_deduplicator: CanonicalStateDeduplicator,
    envelopes: EventEnvelopeFactory,
    server_clock: Option<ServerClockAnchor>,
    objective_catalog: Option<Arc<dyn ObjectiveCatalogResolver>>,
    skill_action_scratch: Vec<u8>,
    config: ProtocolRuntimeConfig,
}

impl<'a> ProtocolRuntime<'a> {
    pub fn new(
        pack: &'a ProtocolPack,
        session_id: impl Into<String>,
        build: &GameBuild,
        region: RegionIdentity,
        mut evidence: Vec<RegionEvidence>,
        config: ProtocolRuntimeConfig,
    ) -> Result<Self, ProtocolRuntimeError> {
        if !pack.matches(build) {
            return Err(ProtocolRuntimeError::PackBuildMismatch);
        }
        if region.deployment_id != build.deployment_id {
            return Err(ProtocolRuntimeError::RegionBuildMismatch);
        }
        if let Some(build_region) = &build.region_id
            && &region.region_id != build_region
        {
            return Err(ProtocolRuntimeError::RegionBuildMismatch);
        }
        if config.max_entities == 0 || config.max_events_per_packet == 0 {
            return Err(ProtocolRuntimeError::InvalidConfig);
        }

        evidence.push(RegionEvidence {
            kind: RegionEvidenceKind::ProtocolPack,
            reference: pack.definition().pack_id.clone(),
        });
        let region_context = RegionContext {
            identity: region,
            client_build: build.build_id.clone(),
            protocol_pack_digest: pack.digest().to_owned(),
            evidence,
        };

        Ok(Self {
            pack,
            client_build: build.build_id.clone(),
            entities: EntityRegistry::new(config.max_entities),
            status_effects: StatusEffectRegistry::new(
                config.max_entities.saturating_mul(16).clamp(1_024, 262_144),
            ),
            dungeon: DungeonTracker::default(),
            profile: ProfileTracker::default(),
            state_deduplicator: CanonicalStateDeduplicator::new(config.max_entities),
            envelopes: EventEnvelopeFactory::new(session_id, region_context),
            server_clock: None,
            objective_catalog: None,
            skill_action_scratch: Vec::new(),
            config,
        })
    }

    pub fn with_objective_catalog(
        mut self,
        objective_catalog: Arc<dyn ObjectiveCatalogResolver>,
    ) -> Self {
        self.objective_catalog = Some(objective_catalog);
        self
    }

    pub fn region_context(&self) -> &RegionContext {
        self.envelopes.region()
    }

    pub fn process(
        &mut self,
        record: &CaptureRecord,
    ) -> Result<ProtocolDecodeBatch, ProtocolRuntimeError> {
        let (status, drafts) = match &record.kind {
            CaptureRecordKind::Gap(gap) => {
                let kind = match gap.kind {
                    CaptureGapKind::AdapterDrop | CaptureGapKind::QueueDrop => {
                        DataGapKind::CaptureDrop
                    }
                    CaptureGapKind::TcpGap => DataGapKind::TcpGap,
                    CaptureGapKind::UnsupportedFragment | CaptureGapKind::UnsupportedTransport => {
                        DataGapKind::UnsupportedFragment
                    }
                    CaptureGapKind::MalformedFrame | CaptureGapKind::DecompressionFailure => {
                        DataGapKind::DecodeFailure
                    }
                };
                (
                    ProtocolDecodeStatus::CaptureGap,
                    DecodedMessage::events(vec![CanonicalEventDraft {
                        time: EventTime {
                            observed_micros: record.observed_micros,
                            game_time_millis: None,
                        },
                        provenance: EventProvenance::wire(
                            record.sequence,
                            gap.connection_id.unwrap_or_default(),
                            gap.stream_id.unwrap_or_default(),
                        ),
                        sensitivity: EventSensitivity::PublicGameplay,
                        kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::DataGap(
                            DataGapEvent {
                                kind,
                                connection_id: gap.connection_id,
                                stream_id: gap.stream_id,
                                detail: gap.detail.clone(),
                            },
                        )),
                    }]),
                )
            }
            CaptureRecordKind::Packet(packet) => {
                let Some(routed) = packet.route else {
                    return Ok(ProtocolDecodeBatch {
                        capture_sequence: record.sequence,
                        status: ProtocolDecodeStatus::Unrouted,
                        events: Vec::new(),
                        announced_server: None,
                        server_clock: None,
                    });
                };

                match self.pack.disposition(Some(&routed.key)) {
                    DecodeDisposition::OpaqueLocalOnly => {
                        return Ok(ProtocolDecodeBatch {
                            capture_sequence: record.sequence,
                            status: ProtocolDecodeStatus::OpaqueLocalOnly,
                            events: Vec::new(),
                            announced_server: None,
                            server_clock: None,
                        });
                    }
                    DecodeDisposition::Prohibited(class) => {
                        return Ok(ProtocolDecodeBatch {
                            capture_sequence: record.sequence,
                            status: ProtocolDecodeStatus::Prohibited(class),
                            events: Vec::new(),
                            announced_server: None,
                            server_clock: None,
                        });
                    }
                    DecodeDisposition::Allowed(_) => {}
                }

                let Some(decoder) = self.pack.decoder(&routed.key) else {
                    return Ok(ProtocolDecodeBatch {
                        capture_sequence: record.sequence,
                        status: ProtocolDecodeStatus::OpaqueLocalOnly,
                        events: Vec::new(),
                        announced_server: None,
                        server_clock: None,
                    });
                };
                let Some(payload) = packet.payload.decode_input() else {
                    let draft = decode_gap_draft(
                        record,
                        packet.connection_id,
                        packet.stream_id,
                        "reviewed route has no decompressed application payload",
                    );
                    return self.finish_batch(
                        record.sequence,
                        ProtocolDecodeStatus::MissingApplicationPayload,
                        DecodedMessage::events(vec![draft]),
                    );
                };

                let metadata = DecodeMetadata {
                    time: EventTime {
                        observed_micros: record.observed_micros,
                        game_time_millis: self.game_time_millis(record.observed_micros),
                    },
                    provenance: EventProvenance::wire(
                        record.sequence,
                        packet.connection_id,
                        packet.stream_id,
                    ),
                    region: self.envelopes.region().identity.clone(),
                };
                match decode_message(
                    decoder,
                    payload,
                    &metadata,
                    &mut self.entities,
                    &mut self.status_effects,
                    &mut self.dungeon,
                    &mut self.profile,
                    &self.client_build,
                    &mut self.skill_action_scratch,
                ) {
                    Ok(decoded) => {
                        if let Some(clock) = decoded.server_clock {
                            self.server_clock = Some(ServerClockAnchor {
                                observed_micros: record.observed_micros,
                                server_milliseconds: clock.server_milliseconds,
                            });
                        }
                        (ProtocolDecodeStatus::Decoded, decoded)
                    }
                    Err(error) => (
                        ProtocolDecodeStatus::DecodeFailed,
                        DecodedMessage::events(vec![decode_gap_draft(
                            record,
                            packet.connection_id,
                            packet.stream_id,
                            &error.to_string(),
                        )]),
                    ),
                }
            }
        };

        self.finish_batch(record.sequence, status, drafts)
    }

    fn finish_batch(
        &mut self,
        capture_sequence: u64,
        status: ProtocolDecodeStatus,
        decoded: DecodedMessage,
    ) -> Result<ProtocolDecodeBatch, ProtocolRuntimeError> {
        if decoded.drafts.len() > self.config.max_events_per_packet {
            return Err(ProtocolRuntimeError::EventLimitExceeded {
                count: decoded.drafts.len(),
                limit: self.config.max_events_per_packet,
            });
        }
        let mut drafts = decoded.drafts;
        self.append_dungeon_wipe_boundary(&mut drafts);
        for draft in &mut drafts {
            self.attach_objective_catalog(draft);
        }
        drafts.retain(|draft| self.state_deduplicator.retain(draft));
        let events = drafts
            .into_iter()
            .map(|draft| self.envelopes.emit(draft))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ProtocolDecodeBatch {
            capture_sequence,
            status,
            events,
            announced_server: decoded.announced_server,
            server_clock: decoded.server_clock,
        })
    }

    fn append_dungeon_wipe_boundary(&self, drafts: &mut Vec<CanonicalEventDraft>) {
        if self.dungeon.state != Some(DUNGEON_STATE_PLAYING) {
            return;
        }
        let Some(local_entity_uuid) = self.profile.local_entity_uuid else {
            return;
        };
        let Some(source) = drafts.iter().find(|draft| {
            matches!(
                &draft.kind,
                CanonicalEventDraftKind::Timeline(TimelineEventKind::Status(status))
                    if status.effect.0 == DUNGEON_WIPE_RECOVERY_EFFECT_ID
                        && status.state == StatusState::Applied
                        && status.target.entity_uuid.0 == local_entity_uuid
            )
        }) else {
            return;
        };
        drafts.push(CanonicalEventDraft {
            time: source.time,
            provenance: source.provenance.clone(),
            sensitivity: source.sensitivity,
            kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::EncounterBoundary {
                state: EncounterState::Wiped,
                encounter_id: None,
                reason: BoundaryReason::Wipe,
            }),
        });
    }

    fn game_time_millis(&self, observed_micros: u64) -> Option<i64> {
        let anchor = self.server_clock?;
        let elapsed_millis =
            i64::try_from(observed_micros.checked_sub(anchor.observed_micros)? / 1_000).ok()?;
        anchor.server_milliseconds.checked_add(elapsed_millis)
    }

    fn attach_objective_catalog(&self, draft: &mut CanonicalEventDraft) {
        let CanonicalEventDraftKind::Dungeon(event) = &mut draft.kind else {
            return;
        };
        let Some(objective_id) = event.objective_id else {
            return;
        };
        if event.objective_catalog.is_some() {
            return;
        }

        event.objective_catalog = Some(match &self.objective_catalog {
            None => DungeonObjectiveCatalogReference {
                resolution: DungeonObjectiveCatalogResolution::CatalogNotConfigured,
                activity_target_key: None,
                scene_event_keys: Vec::new(),
            },
            Some(catalog) => match catalog.resolve(objective_id) {
                Ok(Some(reference)) => reference,
                Ok(None) => DungeonObjectiveCatalogReference {
                    resolution: DungeonObjectiveCatalogResolution::UnresolvedCurrentBuild,
                    activity_target_key: None,
                    scene_event_keys: Vec::new(),
                },
                Err(_) => DungeonObjectiveCatalogReference {
                    resolution: DungeonObjectiveCatalogResolution::CatalogUnavailable,
                    activity_target_key: None,
                    scene_event_keys: Vec::new(),
                },
            },
        });
    }
}

struct DecodedMessage {
    drafts: Vec<CanonicalEventDraft>,
    announced_server: Option<AnnouncedServerEndpoint>,
    server_clock: Option<ServerClockObservation>,
}

impl DecodedMessage {
    fn events(drafts: Vec<CanonicalEventDraft>) -> Self {
        Self {
            drafts,
            announced_server: None,
            server_clock: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ServerClockAnchor {
    observed_micros: u64,
    server_milliseconds: i64,
}

#[derive(Debug, Default)]
struct DungeonTracker {
    instance_id: Option<String>,
    difficulty_id: Option<i32>,
    state: Option<i32>,
    flow: Option<DungeonFlowSnapshot>,
    objectives: BTreeMap<i32, DungeonObjectiveState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DungeonObjectiveState {
    target_id: Option<i32>,
    value: Option<i32>,
    complete: Option<i32>,
}

#[derive(Debug, Default)]
struct ProfileTracker {
    local_character: Option<CharacterIdentity>,
    local_entity_uuid: Option<i64>,
    /// Latest complete local profile projection. Dirty slot updates patch this
    /// snapshot so every live consumer receives the same loadout projection as
    /// history without reparsing or consulting a stale identity cache.
    local_profile: Option<CharacterProfilePatch>,
    last_dirty_profile: Option<CharacterProfilePatch>,
    last_dirty_world: Option<WorldContext>,
    last_social_profile: Option<CharacterProfilePatch>,
    last_social_world: Option<WorldContext>,
    last_team_profiles: BTreeMap<i64, CharacterProfilePatch>,
}

/// Removes only exact, unchanged state echoes before envelope sequence numbers
/// are assigned. Damage, healing, attributes, resources, positions, casts, and
/// every status lifecycle remain byte-for-byte observable. Any run boundary,
/// world transition, or data gap clears the cache so a later state assertion
/// is retained even when an unobserved transition could have occurred.
#[derive(Debug)]
struct CanonicalStateDeduplicator {
    actors: BTreeMap<u64, ActorEvent>,
    cooldowns: BTreeMap<(u64, i64), CooldownEvent>,
    actor_limit: usize,
    cooldown_limit: usize,
}

impl CanonicalStateDeduplicator {
    fn new(max_entities: usize) -> Self {
        Self {
            actors: BTreeMap::new(),
            cooldowns: BTreeMap::new(),
            actor_limit: max_entities.max(1),
            cooldown_limit: max_entities.saturating_mul(16).clamp(1_024, 262_144),
        }
    }

    fn clear(&mut self) {
        self.actors.clear();
        self.cooldowns.clear();
    }

    fn clear_actor(&mut self, actor_id: u64) {
        self.actors.remove(&actor_id);
        self.cooldowns
            .retain(|(cached_actor_id, _), _| *cached_actor_id != actor_id);
    }

    fn insert_actor(&mut self, actor_id: u64, actor: ActorEvent) {
        if !self.actors.contains_key(&actor_id) && self.actors.len() >= self.actor_limit {
            // Missing despawns cannot make this optimization unbounded. As
            // with cooldown pressure, clearing emits more evidence, not less.
            self.actors.clear();
        }
        self.actors.insert(actor_id, actor);
    }

    fn retain(&mut self, draft: &CanonicalEventDraft) -> bool {
        match &draft.kind {
            CanonicalEventDraftKind::WorldChanged(_) => {
                self.clear();
                true
            }
            CanonicalEventDraftKind::Timeline(TimelineEventKind::RunBoundary { .. })
            | CanonicalEventDraftKind::Timeline(TimelineEventKind::DataGap(_)) => {
                self.clear();
                true
            }
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(actor)) => {
                let actor_id = actor.actor.actor_id.0;
                match actor.state {
                    ActorState::Despawned => {
                        self.clear_actor(actor_id);
                        true
                    }
                    ActorState::Spawned => {
                        self.clear_actor(actor_id);
                        self.insert_actor(actor_id, actor.clone());
                        true
                    }
                    ActorState::Transformed => {
                        self.insert_actor(actor_id, actor.clone());
                        true
                    }
                    ActorState::Updated => {
                        if self.actors.get(&actor_id) == Some(actor) {
                            false
                        } else {
                            self.insert_actor(actor_id, actor.clone());
                            true
                        }
                    }
                }
            }
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Cooldown(cooldown)) => {
                let key = (cooldown.actor.actor_id.0, cooldown.ability.0);
                if self.cooldowns.get(&key) == Some(cooldown) {
                    return false;
                }
                if !self.cooldowns.contains_key(&key) && self.cooldowns.len() >= self.cooldown_limit
                {
                    // Capacity pressure may reduce deduplication efficiency but
                    // never evidence: clearing makes subsequent echoes emit.
                    self.cooldowns.clear();
                }
                self.cooldowns.insert(key, cooldown.clone());
                true
            }
            _ => true,
        }
    }
}

#[derive(Debug, Clone)]
struct DecodeMetadata {
    time: EventTime,
    provenance: EventProvenance,
    region: RegionIdentity,
}

#[derive(Debug)]
struct EntityRegistry {
    by_uuid: BTreeMap<i64, EntityState>,
    next_actor_id: u64,
    limit: usize,
}

#[derive(Debug, Clone, Copy)]
struct EntityState {
    identity: EntityRef,
    entity_type_id: i32,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    specialization_evidence: Option<SpecializationEvidenceStrength>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SpecializationEvidenceStrength {
    Supporting,
    Primary,
    Authoritative,
}

#[derive(Debug)]
struct StatusEffectRegistry {
    active: BTreeMap<(i64, i32), ActiveStatusEffect>,
    limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveStatusEffect {
    base_id: i32,
    source_uuid: Option<i64>,
    origin: Option<StatusOrigin>,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
    create_time_millis: Option<i64>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
}

impl StatusEffectRegistry {
    fn new(limit: usize) -> Self {
        Self {
            active: BTreeMap::new(),
            limit,
        }
    }

    fn clear(&mut self) {
        self.active.clear();
    }

    fn remove_target(&mut self, target_uuid: i64) {
        self.active
            .retain(|(active_target, _), _| *active_target != target_uuid);
    }

    fn get(&self, target_uuid: i64, buff_uuid: i32) -> Option<ActiveStatusEffect> {
        self.active.get(&(target_uuid, buff_uuid)).copied()
    }

    fn insert(
        &mut self,
        target_uuid: i64,
        buff_uuid: i32,
        effect: ActiveStatusEffect,
    ) -> Option<ActiveStatusEffect> {
        let key = (target_uuid, buff_uuid);
        if !self.active.contains_key(&key)
            && self.active.len() >= self.limit
            && let Some(eviction_key) = self.active.keys().next().copied()
        {
            self.active.remove(&eviction_key);
        }
        self.active.insert(key, effect)
    }

    fn remove(&mut self, target_uuid: i64, buff_uuid: i32) -> Option<ActiveStatusEffect> {
        self.active.remove(&(target_uuid, buff_uuid))
    }
}

impl EntityRegistry {
    fn new(limit: usize) -> Self {
        Self {
            by_uuid: BTreeMap::new(),
            next_actor_id: 1,
            limit,
        }
    }

    fn resolve(
        &mut self,
        uuid: i64,
        entity_type_id: Option<i32>,
    ) -> Result<EntityState, ProtocolMessageError> {
        if let Some(state) = self.by_uuid.get_mut(&uuid) {
            if let Some(entity_type_id) = entity_type_id
                && entity_type_id != 0
            {
                state.entity_type_id = entity_type_id;
            }
            return Ok(*state);
        }
        if self.by_uuid.len() >= self.limit {
            return Err(ProtocolMessageError::EntityLimitExceeded(self.limit));
        }
        let actor_id = self.next_actor_id;
        self.next_actor_id = self
            .next_actor_id
            .checked_add(1)
            .ok_or(ProtocolMessageError::ActorSequenceExhausted)?;
        let state = EntityState {
            identity: EntityRef {
                actor_id: ActorId(actor_id),
                entity_uuid: EntityUuid(uuid),
            },
            entity_type_id: entity_type_id.unwrap_or_default(),
            class_id: None,
            specialization_id: None,
            specialization_evidence: None,
        };
        self.by_uuid.insert(uuid, state);
        Ok(state)
    }

    fn observe_combat_identity(
        &mut self,
        uuid: i64,
        class_id: Option<i32>,
        specialization_id: Option<i32>,
    ) -> Option<EntityState> {
        let state = self.by_uuid.get_mut(&uuid)?;
        if let Some(class_id) = class_id {
            if state.class_id != Some(class_id) {
                state.class_id = Some(class_id);
                state.specialization_id = None;
                state.specialization_evidence = None;
            }
            if state.entity_type_id == 0 {
                state.entity_type_id = ENTITY_PLAYER;
            }
        }
        if let Some(specialization_id) = specialization_id {
            state.specialization_id = Some(specialization_id);
            state.specialization_evidence = Some(SpecializationEvidenceStrength::Authoritative);
        }
        Some(*state)
    }

    fn observe_specialization_ability(
        &mut self,
        uuid: i64,
        ability_id: i64,
    ) -> Option<EntityState> {
        let state = self.by_uuid.get_mut(&uuid)?;
        let evidence = specialization_ability_evidence(state.class_id, ability_id)
            .ok()
            .flatten()?;
        let evidence_strength = match evidence.strength {
            SpecializationAbilityEvidenceStrength::Supporting => {
                SpecializationEvidenceStrength::Supporting
            }
            SpecializationAbilityEvidenceStrength::Primary => {
                SpecializationEvidenceStrength::Primary
            }
        };
        if state
            .specialization_evidence
            .is_some_and(|current| current > evidence_strength)
            || (state.specialization_evidence == Some(evidence_strength)
                && state.specialization_id != Some(evidence.specialization_id))
        {
            return None;
        }
        if state.class_id == Some(evidence.class_id)
            && state.specialization_id == Some(evidence.specialization_id)
            && state.specialization_evidence == Some(evidence_strength)
        {
            return None;
        }
        state.class_id = Some(evidence.class_id);
        state.specialization_id = Some(evidence.specialization_id);
        state.specialization_evidence = Some(evidence_strength);
        if state.entity_type_id == 0 {
            state.entity_type_id = ENTITY_PLAYER;
        }
        Some(*state)
    }

    fn observe_specialization_passives(
        &mut self,
        uuid: i64,
        passive_infos: &schema::SeqPassiveSkillInfo,
    ) -> Option<EntityState> {
        let state = self.by_uuid.get_mut(&uuid)?;
        let selector_ids = || {
            passive_infos
                .passive_infos
                .iter()
                .filter_map(|info| info.skill_id.map(i64::from))
        };
        let (class_id, specialization_id) = if let Some(class_id) = state.class_id {
            (
                class_id,
                specialization_from_passive_selectors(class_id, selector_ids())
                    .ok()
                    .flatten()?,
            )
        } else {
            specialization_identity_from_passive_selectors(selector_ids())
                .ok()
                .flatten()?
        };
        if state.class_id == Some(class_id) && state.specialization_id == Some(specialization_id) {
            state.specialization_evidence = Some(SpecializationEvidenceStrength::Authoritative);
            return None;
        }
        state.class_id = Some(class_id);
        state.specialization_id = Some(specialization_id);
        state.specialization_evidence = Some(SpecializationEvidenceStrength::Authoritative);
        if state.entity_type_id == 0 {
            state.entity_type_id = ENTITY_PLAYER;
        }
        Some(*state)
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_message(
    decoder: DecoderKind,
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
    dungeon: &mut DungeonTracker,
    profile: &mut ProfileTracker,
    client_build: &str,
    skill_action_scratch: &mut Vec<u8>,
) -> Result<DecodedMessage, ProtocolMessageError> {
    let drafts = match decoder {
        DecoderKind::NotifyEnterWorldV1 => return decode_notify_enter_world(payload),
        DecoderKind::SyncServerTimeV1 => return decode_server_time(payload),
        DecoderKind::SyncSeasonV1 => {
            decode_sync_season(payload, metadata, &profile.local_character)
        }
        DecoderKind::SyncDungeonDataV1 => decode_sync_dungeon(payload, metadata, dungeon),
        DecoderKind::SyncDungeonDirtyDataV1 => {
            decode_sync_dungeon_dirty(payload, metadata, dungeon)
        }
        DecoderKind::EnterSceneV1 => {
            status_effects.clear();
            decode_enter_scene(payload, metadata, entities, status_effects)
        }
        DecoderKind::NotifyLoadSceneEndV1 => decode_load_scene_end(payload, metadata),
        DecoderKind::SyncNearEntitiesV1 => {
            decode_sync_near_entities(payload, metadata, entities, status_effects)
        }
        DecoderKind::SyncContainerDataV1 => {
            decode_sync_container(payload, metadata, entities, profile)
        }
        DecoderKind::SyncContainerDirtyDataV1 => {
            decode_sync_container_dirty(payload, metadata, entities, profile)
        }
        DecoderKind::NotifySocialDataV1 => decode_notify_social_data(payload, metadata, profile),
        DecoderKind::NotifyTeamMemberInfoV1 => {
            decode_notify_team_member_info(payload, metadata, entities, profile)
        }
        DecoderKind::NotifyJoinTeamV1 => {
            decode_notify_join_team(payload, metadata, entities, profile)
        }
        DecoderKind::NotifyLeaveTeamV1 => decode_notify_leave_team(payload, metadata),
        DecoderKind::NoticeTeamDissolveV1 => decode_notice_team_dissolve(payload, metadata),
        DecoderKind::SyncNearDeltaV1 => {
            decode_sync_near_delta(payload, metadata, entities, status_effects)
        }
        DecoderKind::SyncToMeDeltaV1 => {
            decode_sync_to_me_delta(payload, metadata, entities, status_effects, profile)
        }
        DecoderKind::NotifyReviveV1 => decode_revive(payload, metadata, entities),
        DecoderKind::SyncClientUseSkillV1 => {
            decode_sync_client_use_skill(payload, metadata, entities, profile)
        }
        DecoderKind::WorldUseSlotV1 => decode_world_use_slot_skill_action(
            payload,
            metadata,
            entities,
            profile,
            client_build,
            skill_action_scratch,
        ),
    }?;
    Ok(DecodedMessage::events(drafts))
}

fn decode_notify_enter_world(payload: &[u8]) -> Result<DecodedMessage, ProtocolMessageError> {
    let message = schema::NotifyEnterWorld::decode(payload)?;
    let announced_server = message.request.and_then(|request| {
        let host = request.scene_host?.trim().to_owned();
        if host.is_empty() || host.len() > 253 || !host.is_ascii() {
            return None;
        }
        let port = request
            .scene_port
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0);
        Some(AnnouncedServerEndpoint { host, port })
    });
    Ok(DecodedMessage {
        drafts: Vec::new(),
        announced_server,
        server_clock: None,
    })
}

fn decode_server_time(payload: &[u8]) -> Result<DecodedMessage, ProtocolMessageError> {
    let message = schema::SyncServerTime::decode(payload)?;
    let server_clock =
        message
            .server_milliseconds
            .filter(|value| *value > 0)
            .map(|server_milliseconds| ServerClockObservation {
                client_milliseconds: message.client_milliseconds,
                server_milliseconds,
            });
    Ok(DecodedMessage {
        drafts: Vec::new(),
        announced_server: None,
        server_clock,
    })
}

fn decode_sync_season(
    payload: &[u8],
    metadata: &DecodeMetadata,
    local_character: &Option<CharacterIdentity>,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncSeason::decode(payload)?;
    let Some(season_id) = message.season_id.filter(|season_id| *season_id > 0) else {
        return Ok(Vec::new());
    };
    let Some(character) = local_character.clone() else {
        return Ok(Vec::new());
    };
    Ok(vec![draft(
        metadata,
        EventSensitivity::PersonalGameplay,
        CanonicalEventDraftKind::CharacterProfileObserved {
            profile: Box::new(
                CharacterProfilePatch {
                    character,
                    display_name: None,
                    display_id: None,
                    server_id: None,
                    class_id: None,
                    specialization_id: None,
                    level: None,
                    progression: None,
                    combat_power: None,
                    combat_power_breakdown: None,
                    season_strength: None,
                    master_score: None,
                    season: Some(SeasonProfile {
                        season_id: Some(i64::from(season_id)),
                        level: None,
                        experience: None,
                        power: None,
                        strength: None,
                    }),
                    appearance: None,
                    equipment: None,
                    equipment_suit_entries: None,
                    modules: None,
                    owned_imagines: None,
                    battle_imagine_skills: None,
                    equipped_action_slots: None,
                    active_skills: None,
                    talents: None,
                    talent_progress: None,
                    combat_professions: None,
                    life_professions: None,
                    cosmetics: None,
                    collection_summary: None,
                    activity_progress: None,
                    season_medals: None,
                    season_cultivation: None,
                    reputations: None,
                    current_profession_project_id: None,
                    social_display: None,
                }
                .into_game_event()?,
            ),
        },
    )])
}

fn decode_notify_social_data(
    payload: &[u8],
    metadata: &DecodeMetadata,
    tracker: &mut ProfileTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::NotifySocialData::decode(payload)?;
    let Some(social) = message.request.and_then(|request| request.data) else {
        return Ok(Vec::new());
    };
    let basic = social.basic.as_ref();
    let avatar = social.avatar.as_ref();
    let personal_zone = social.personal_zone.as_ref();
    let attributes = social.user_attributes.as_ref();

    let character_id = social
        .character_id
        .or_else(|| basic.and_then(|basic| basic.character_id));
    let profile = character_id.map(|character_id| {
        let avatar_id = avatar.and_then(|avatar| positive_i32(avatar.avatar_id));
        let profile_image_url = avatar
            .and_then(|avatar| avatar.profile.as_ref())
            .and_then(|picture| reviewed_picture_url(picture.url.as_deref()));
        let half_body_image_url = avatar
            .and_then(|avatar| avatar.half_body.as_ref())
            .and_then(|picture| reviewed_picture_url(picture.url.as_deref()));
        let business_card_style_id = avatar
            .and_then(|avatar| positive_i32(avatar.business_card_style_id))
            .or_else(|| personal_zone.and_then(|zone| positive_i32(zone.business_card_style_id)));
        let avatar_frame_id = avatar
            .and_then(|avatar| positive_i32(avatar.avatar_frame_id))
            .or_else(|| personal_zone.and_then(|zone| positive_i32(zone.avatar_frame_id)));
        let appearance = if basic.and_then(|basic| basic.gender_id).is_some()
            || basic.and_then(|basic| basic.body_size_id).is_some()
            || avatar_id.is_some()
            || profile_image_url.is_some()
            || half_body_image_url.is_some()
            || business_card_style_id.is_some()
            || avatar_frame_id.is_some()
        {
            Some(CharacterAppearance {
                gender_id: basic.and_then(|basic| basic.gender_id),
                body_size_id: basic.and_then(|basic| basic.body_size_id),
                height: None,
                voice_id: None,
                face_options: BTreeMap::new(),
                color_options: BTreeMap::new(),
                avatar_id,
                profile_image_url,
                half_body_image_url,
                business_card_style_id,
                avatar_frame_id,
                unlocked_profile_image_ids: Vec::new(),
                unlocked_face_item_ids: Vec::new(),
                unlocked_voice_ids: Vec::new(),
            })
        } else {
            None
        };

        let equipment = social_equipment(social.equipment.as_ref());

        let combat_professions = social.profession.as_ref().and_then(|profession| {
            let profession_id = positive_i32(profession.profession_id)?;
            Some(vec![CombatProfessionProfile {
                profession_id,
                level: None,
                experience: None,
                skills: Vec::new(),
                active_skill_ids: Vec::new(),
                slotted_skill_ids: BTreeMap::new(),
                weapon_skin_id: positive_i32(profession.weapon_skin_id).map(i64::from),
                talent_node_ids: Vec::new(),
                talent_points_used: None,
                talent_stage_config_id: None,
            }])
        });

        let guild_id = social
            .guild
            .as_ref()
            .and_then(|guild| guild.guild_id.filter(|guild_id| *guild_id > 0));
        let guild_name = social
            .guild
            .as_ref()
            .and_then(|guild| clean_text(guild.guild_name.as_deref()));
        let title_ids = personal_zone
            .and_then(|zone| positive_i32(zone.title_id))
            .map(|title_id| vec![i64::from(title_id)])
            .unwrap_or_default();
        let mut medal_ids = personal_zone
            .into_iter()
            .flat_map(|zone| zone.medals.values())
            .copied()
            .filter(|medal_id| *medal_id > 0)
            .map(i64::from)
            .collect::<Vec<_>>();
        medal_ids.sort_unstable();
        medal_ids.dedup();
        let social_display = if guild_id.is_some()
            || guild_name.is_some()
            || !title_ids.is_empty()
            || !medal_ids.is_empty()
        {
            let medal_slots = personal_zone
                .into_iter()
                .flat_map(|zone| zone.medals.iter())
                .filter_map(|(slot, medal_id)| {
                    positive_i32(Some(*medal_id)).map(|medal_id| (*slot, i64::from(medal_id)))
                })
                .collect();
            Some(SocialDisplay {
                guild_id,
                guild_name,
                title_ids,
                medal_ids,
                medal_slots,
                profile_theme_id: None,
            })
        } else {
            None
        };

        let season_level = basic
            .and_then(|basic| positive_i32(basic.season_level))
            .and_then(|level| u32::try_from(level).ok());
        let season_strength = attributes
            .and_then(|attributes| positive_i32(attributes.season_strength))
            .map(i64::from);
        let season = if season_level.is_some() || season_strength.is_some() {
            Some(SeasonProfile {
                season_id: None,
                level: season_level,
                experience: None,
                power: None,
                strength: season_strength,
            })
        } else {
            None
        };

        CharacterProfilePatch {
            character: CharacterIdentity {
                region: metadata.region.clone(),
                character_id: character_id.to_string(),
            },
            display_name: basic.and_then(|basic| clean_text(basic.display_name.as_deref())),
            display_id: basic
                .and_then(|basic| basic.display_id)
                .filter(|display_id| *display_id > 0)
                .map(|display_id| display_id.to_string()),
            server_id: None,
            class_id: social
                .profession
                .as_ref()
                .and_then(|profession| positive_i32(profession.profession_id)),
            specialization_id: None,
            level: basic
                .and_then(|basic| positive_i32(basic.level))
                .and_then(|level| u32::try_from(level).ok()),
            progression: None,
            combat_power: attributes
                .and_then(|attributes| attributes.combat_power)
                .filter(|combat_power| *combat_power > 0),
            combat_power_breakdown: None,
            season_strength,
            master_score: social
                .master_mode_dungeon
                .as_ref()
                .and_then(|master| positive_i32(master.season_score))
                .map(i64::from),
            season,
            appearance,
            equipment,
            equipment_suit_entries: None,
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: None,
            equipped_action_slots: None,
            active_skills: None,
            talents: None,
            talent_progress: None,
            combat_professions,
            life_professions: None,
            cosmetics: None,
            collection_summary: None,
            activity_progress: None,
            season_medals: None,
            season_cultivation: None,
            reputations: None,
            current_profession_project_id: None,
            social_display,
        }
    });

    let scene = social.scene.as_ref();
    let map_id = scene
        .and_then(|scene| scene.map_id.filter(|map_id| *map_id > 0))
        .or_else(|| scene.and_then(|scene| scene.level_map_id.filter(|map_id| *map_id > 0)))
        .or_else(|| basic.and_then(|basic| basic.scene_id.filter(|scene_id| *scene_id > 0)));
    let world = if map_id.is_some()
        || scene
            .and_then(|scene| scene.line_id.or(scene.channel_id))
            .is_some()
        || scene
            .and_then(|scene| scene.scene_instance_id.as_ref())
            .is_some()
        || basic
            .and_then(|basic| basic.scene_instance_id.as_ref())
            .is_some()
        || scene
            .and_then(|scene| scene.dungeon_instance_id.as_ref())
            .is_some()
    {
        Some(WorldContext {
            scene_id: map_id
                .and_then(|map_id| i32::try_from(map_id).ok())
                .map(SceneId),
            map_id,
            line_id: scene.and_then(|scene| scene.line_id.or(scene.channel_id)),
            scene_instance_id: scene
                .and_then(|scene| clean_text(scene.scene_instance_id.as_deref()))
                .or_else(|| basic.and_then(|basic| clean_text(basic.scene_instance_id.as_deref()))),
            dungeon_instance_id: scene
                .and_then(|scene| clean_text(scene.dungeon_instance_id.as_deref())),
        })
    } else {
        None
    };

    let mut drafts = Vec::with_capacity(2);
    if let Some(profile) = profile
        && tracker.last_social_profile.as_ref() != Some(&profile)
    {
        tracker.last_social_profile = Some(profile.clone());
        drafts.push(draft(
            metadata,
            EventSensitivity::PublicGameplay,
            CanonicalEventDraftKind::CharacterProfileObserved {
                profile: Box::new(profile.into_game_event()?),
            },
        ));
    }
    if let Some(world) = world
        && tracker.last_social_world.as_ref() != Some(&world)
    {
        tracker.last_social_world = Some(world.clone());
        drafts.push(draft(
            metadata,
            EventSensitivity::PublicGameplay,
            CanonicalEventDraftKind::WorldChanged(world),
        ));
    }
    Ok(drafts)
}

fn decode_notify_team_member_info(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    tracker: &mut ProfileTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::NoticeUpdateTeamMemberInfo::decode(payload)?;
    let Some(request) = message.request else {
        return Ok(Vec::new());
    };
    let members = team_member_roster_members(&request.members, metadata);
    let mut drafts = Vec::new();
    if !members.is_empty() {
        drafts.push(party_roster_draft(
            metadata,
            PartyRosterObservation::MembersObserved { members },
        ));
    }
    drafts.extend(decode_team_members(
        request.members,
        metadata,
        entities,
        tracker,
    )?);
    Ok(drafts)
}

fn decode_notify_join_team(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    tracker: &mut ProfileTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::NotifyJoinTeam::decode(payload)?;
    let Some(request) = message.request else {
        return Ok(Vec::new());
    };
    let members = team_member_roster_members(&request.members, metadata);
    let party_id = request
        .base_info
        .and_then(|base| base.team_id)
        .filter(|party_id| *party_id > 0)
        .map(|party_id| party_id.to_string());
    let mut drafts = vec![party_roster_draft(
        metadata,
        PartyRosterObservation::FullSnapshot { party_id, members },
    )];
    drafts.extend(decode_team_members(
        request.members,
        metadata,
        entities,
        tracker,
    )?);
    Ok(drafts)
}

fn decode_notify_leave_team(
    payload: &[u8],
    metadata: &DecodeMetadata,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::NotifyLeaveTeam::decode(payload)?;
    let Some(request) = message.request else {
        return Ok(Vec::new());
    };
    let Some(character_id) = request
        .character_id
        .filter(|character_id| *character_id > 0)
    else {
        return Ok(Vec::new());
    };
    Ok(vec![party_roster_draft(
        metadata,
        PartyRosterObservation::MemberLeft {
            member: CharacterIdentity {
                region: metadata.region.clone(),
                character_id: character_id.to_string(),
            },
            leave_type: request.leave_type,
        },
    )])
}

fn decode_notice_team_dissolve(
    payload: &[u8],
    metadata: &DecodeMetadata,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::NoticeTeamDissolve::decode(payload)?;
    if message.request.is_none() {
        return Ok(Vec::new());
    }
    Ok(vec![party_roster_draft(
        metadata,
        PartyRosterObservation::Dissolved,
    )])
}

fn team_member_roster_members(
    members: &[schema::TeamMemberData],
    metadata: &DecodeMetadata,
) -> Vec<PartyRosterMember> {
    members
        .iter()
        .filter_map(|member| {
            let character_id = team_member_character_id(member)?;
            Some(PartyRosterMember {
                character: CharacterIdentity {
                    region: metadata.region.clone(),
                    character_id: character_id.to_string(),
                },
                enter_time: member.enter_time,
                online_status: member.online_status,
                scene_id: member.scene_id,
                group_id: member.group_id,
            })
        })
        .collect()
}

fn team_member_character_id(member: &schema::TeamMemberData) -> Option<i64> {
    member
        .character_id
        .or_else(|| {
            member
                .social
                .as_ref()
                .and_then(|social| social.basic.as_ref())
                .and_then(|basic| basic.character_id)
        })
        .filter(|character_id| *character_id > 0)
}

fn party_roster_draft(
    metadata: &DecodeMetadata,
    observation: PartyRosterObservation,
) -> CanonicalEventDraft {
    draft(
        metadata,
        EventSensitivity::PublicGameplay,
        CanonicalEventDraftKind::PartyRosterObserved(PartyRosterEvent { observation }),
    )
}

fn decode_team_members(
    members: Vec<schema::TeamMemberData>,
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    tracker: &mut ProfileTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let mut drafts = Vec::with_capacity(members.len().saturating_mul(2));
    for member in members {
        let Some(character_id) = team_member_character_id(&member) else {
            continue;
        };
        let Some(social) = member.social else {
            continue;
        };
        let basic = social.basic.as_ref();
        let profession = social.profession.as_ref();
        let attributes = social.user_attributes.as_ref();
        let season_level = basic
            .and_then(|basic| positive_i32(basic.season_level))
            .and_then(|level| u32::try_from(level).ok());
        let season_strength = attributes
            .and_then(|attributes| positive_i32(attributes.season_strength))
            .map(i64::from);
        let season = if season_level.is_some() || season_strength.is_some() {
            Some(SeasonProfile {
                season_id: None,
                level: season_level,
                experience: None,
                power: None,
                strength: season_strength,
            })
        } else {
            None
        };
        let equipment = social_equipment(social.equipment.as_ref());
        let profile = CharacterProfilePatch {
            character: CharacterIdentity {
                region: metadata.region.clone(),
                character_id: character_id.to_string(),
            },
            display_name: basic.and_then(|basic| clean_text(basic.display_name.as_deref())),
            display_id: basic
                .and_then(|basic| basic.display_id)
                .filter(|display_id| *display_id > 0)
                .map(|display_id| display_id.to_string()),
            server_id: None,
            class_id: profession.and_then(|profession| positive_i32(profession.profession_id)),
            // The observed team `talent_id` is a party-role value (damage 1,
            // healer 2, tank 3), not a specialization catalog ID. Keep it out
            // rather than presenting a guessed specialization.
            specialization_id: None,
            level: basic
                .and_then(|basic| positive_i32(basic.level))
                .and_then(|level| u32::try_from(level).ok()),
            progression: None,
            combat_power: attributes
                .and_then(|attributes| attributes.combat_power)
                .filter(|combat_power| *combat_power > 0),
            combat_power_breakdown: None,
            season_strength,
            master_score: None,
            season,
            appearance: None,
            equipment,
            equipment_suit_entries: None,
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: None,
            equipped_action_slots: None,
            active_skills: None,
            talents: None,
            talent_progress: None,
            combat_professions: None,
            life_professions: None,
            cosmetics: None,
            collection_summary: None,
            activity_progress: None,
            season_medals: None,
            season_cultivation: None,
            reputations: None,
            current_profession_project_id: None,
            social_display: None,
        };
        if tracker.last_team_profiles.get(&character_id) == Some(&profile) {
            continue;
        }
        if tracker.last_team_profiles.len() >= 64
            && !tracker.last_team_profiles.contains_key(&character_id)
            && let Some(oldest) = tracker.last_team_profiles.keys().next().copied()
        {
            tracker.last_team_profiles.remove(&oldest);
        }
        tracker
            .last_team_profiles
            .insert(character_id, profile.clone());
        if let Some(entity_uuid) = character_entity_uuid(character_id) {
            let state = entities.resolve(entity_uuid, Some(ENTITY_PLAYER))?;
            let state = entities
                .observe_combat_identity(entity_uuid, profile.class_id, profile.specialization_id)
                .unwrap_or(state);
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Actor(ActorEvent {
                    actor: state.identity,
                    state: ActorState::Updated,
                    entity_type_id: ENTITY_PLAYER,
                    kind: ActorKind::Player,
                    monster_id: None,
                    character_id: Some(character_id.to_string()),
                    display_name: profile.display_name.clone(),
                    class_id: profile.class_id,
                    specialization_id: profile.specialization_id,
                    level: profile.level,
                    ability_score: profile.combat_power,
                    weapon_item_id: profile
                        .equipment
                        .as_ref()
                        .and_then(|items| items.iter().find(|item| item.slot_id == 200))
                        .map(|item| item.item_id),
                    weapon_breakthrough_count: profile_weapon_breakthrough_count(&profile),
                    seasonal_score: profile.season_strength,
                    primary_loadout: Vec::new(),
                    auxiliary_loadout: Vec::new(),
                    loadout_observation: ActorLoadoutObservation::default(),
                }),
            ));
        }
        drafts.push(draft(
            metadata,
            EventSensitivity::PublicGameplay,
            CanonicalEventDraftKind::CharacterProfileObserved {
                profile: Box::new(profile.into_game_event()?),
            },
        ));
    }
    Ok(drafts)
}

fn character_entity_uuid(character_id: i64) -> Option<i64> {
    let shifted = i128::from(character_id).checked_shl(16)?;
    i64::try_from(shifted).ok()
}

fn social_equipment(equipment: Option<&schema::SocialEquipmentData>) -> Option<Vec<EquipmentItem>> {
    equipment.map(|equipment| {
        let mut items = equipment
            .items
            .iter()
            .filter_map(|item| {
                let slot_id = item.slot_id?;
                let item_id = item.item_id.filter(|item_id| *item_id > 0)?;
                Some(EquipmentItem {
                    slot_id,
                    item_id: i64::from(item_id),
                    instance_id: None,
                    level: None,
                    quality: None,
                    refinement_level: None,
                    refinement_failed_count: None,
                    attributes: None,
                    enchantment_ids: Vec::new(),
                    enchantments: Vec::new(),
                    set_id: None,
                })
            })
            .collect::<Vec<_>>();
        items.sort_unstable_by_key(|item| (item.slot_id, item.item_id));
        items
    })
}

fn profile_weapon_breakthrough_count(profile: &CharacterProfilePatch) -> Option<u32> {
    profile
        .equipment
        .as_ref()?
        .iter()
        .find(|item| item.slot_id == WEAPON_SLOT_ID)?
        .attributes
        .as_ref()?
        .breakthrough_count
        .and_then(|count| u32::try_from(count).ok())
}

fn actor_event_from_profile(
    actor: EntityRef,
    profile: &CharacterProfilePatch,
    primary_loadout: Vec<ActorLoadoutSlot>,
    auxiliary_loadout: Vec<ActorLoadoutSlot>,
) -> ActorEvent {
    ActorEvent {
        actor,
        state: ActorState::Updated,
        entity_type_id: ENTITY_PLAYER,
        kind: ActorKind::Player,
        monster_id: None,
        character_id: Some(profile.character.character_id.clone()),
        display_name: profile.display_name.clone(),
        class_id: profile.class_id,
        specialization_id: profile.specialization_id,
        level: profile.level,
        ability_score: profile.combat_power,
        weapon_item_id: profile
            .equipment
            .as_ref()
            .and_then(|items| items.iter().find(|item| item.slot_id == WEAPON_SLOT_ID))
            .map(|item| item.item_id),
        weapon_breakthrough_count: profile_weapon_breakthrough_count(profile),
        seasonal_score: profile
            .season_strength
            .or_else(|| profile.season.as_ref().and_then(|season| season.strength)),
        primary_loadout,
        auxiliary_loadout,
        loadout_observation: ActorLoadoutObservation {
            primary: ActorLoadoutEvidence::ExactSlots,
            auxiliary: ActorLoadoutEvidence::ExactSlots,
        },
    }
}

fn positive_i32(value: Option<i32>) -> Option<i32> {
    value.filter(|value| *value > 0)
}

fn clean_text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn reviewed_picture_url(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (value.len() <= 2_048
        && value.starts_with("https://")
        && !value.chars().any(char::is_control)
        && !value.chars().any(char::is_whitespace))
    .then(|| value.to_owned())
}

fn decode_sync_dungeon(
    payload: &[u8],
    metadata: &DecodeMetadata,
    tracker: &mut DungeonTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncDungeonData::decode(payload)?;
    let Some(dungeon) = message.dungeon else {
        return Ok(Vec::new());
    };
    let mut drafts = Vec::new();
    prepare_dungeon_identity(
        tracker,
        dungeon.scene_uuid.map(|uuid| uuid.to_string()),
        dungeon.scene_info.and_then(|info| info.difficulty),
    );
    record_dungeon_flow(
        metadata,
        tracker,
        dungeon.flow_info.and_then(flow_snapshot),
        false,
        &mut drafts,
    );

    if let Some(target) = dungeon.target {
        let mut targets = target.target_data.into_iter().collect::<Vec<_>>();
        targets.sort_unstable_by_key(|(map_key, _)| *map_key);
        for (map_key, target) in targets {
            let next = DungeonObjectiveState {
                target_id: target.target_id,
                value: target.value,
                complete: target.complete,
            };
            if tracker.objectives.get(&map_key) == Some(&next) {
                continue;
            }
            tracker.objectives.insert(map_key, next.clone());
            emit_objective_update(metadata, tracker, map_key, &next, &mut drafts);
        }
    }

    Ok(drafts)
}

fn decode_sync_dungeon_dirty(
    payload: &[u8],
    metadata: &DecodeMetadata,
    tracker: &mut DungeonTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncDungeonDirtyData::decode(payload)?;
    let Some(data) = message.data else {
        return Ok(Vec::new());
    };
    let Some(buffer) = data.buffer else {
        return Ok(Vec::new());
    };
    let patch =
        dungeon_dirty_v1::decode_dungeon_update(&buffer, data.stream_type.unwrap_or_default())?;
    let mut drafts = Vec::new();
    prepare_dungeon_identity(tracker, patch.scene_uuid.map(|uuid| uuid.to_string()), None);
    record_dungeon_flow(metadata, tracker, patch.flow, true, &mut drafts);

    for mutation in patch.objectives {
        match mutation {
            DirtyDungeonObjectiveMutation::Upsert {
                map_key,
                target_id,
                value,
                complete,
            } => {
                let previous = tracker.objectives.get(&map_key).cloned();
                let mut next = previous.clone().unwrap_or_default();
                if let Some(target_id) = target_id {
                    next.target_id = Some(target_id);
                }
                if let Some(value) = value {
                    next.value = Some(value);
                }
                if let Some(complete) = complete {
                    next.complete = Some(complete);
                }
                if previous.as_ref() == Some(&next) {
                    continue;
                }
                tracker.objectives.insert(map_key, next.clone());
                emit_objective_update(metadata, tracker, map_key, &next, &mut drafts);
            }
            DirtyDungeonObjectiveMutation::Remove { map_key } => {
                let removed = tracker.objectives.remove(&map_key);
                let mut event = dungeon_event(tracker, DungeonEventKind::ObjectiveRemoved);
                event.objective_map_key = Some(map_key);
                event.objective_id = Some(i64::from(
                    removed.and_then(|state| state.target_id).unwrap_or(map_key),
                ));
                drafts.push(dungeon_draft(metadata, event));
            }
        }
    }
    Ok(drafts)
}

fn prepare_dungeon_identity(
    tracker: &mut DungeonTracker,
    instance_id: Option<String>,
    difficulty_id: Option<i32>,
) {
    if instance_id.is_some() && tracker.instance_id != instance_id {
        tracker.instance_id = instance_id;
        tracker.difficulty_id = None;
        tracker.state = None;
        tracker.flow = None;
        tracker.objectives.clear();
    }
    if let Some(difficulty_id) = difficulty_id {
        tracker.difficulty_id = Some(difficulty_id);
    }
}

fn flow_snapshot(flow: schema::DungeonFlowInfo) -> Option<DungeonFlowSnapshot> {
    let state_id = flow.state;
    let snapshot = DungeonFlowSnapshot {
        state_id,
        phase: state_id.map(DungeonFlowPhase::from_protocol_id),
        active_time_raw: flow.active_time,
        ready_time_raw: flow.ready_time,
        play_time_raw: flow.play_time,
        end_time_raw: flow.end_time,
        settlement_time_raw: flow.settlement_time,
        dungeon_times_raw: flow.dungeon_times,
        result_id: flow.result,
    };
    snapshot.has_evidence().then_some(snapshot)
}

fn record_dungeon_flow(
    metadata: &DecodeMetadata,
    tracker: &mut DungeonTracker,
    incoming: Option<DungeonFlowSnapshot>,
    merge: bool,
    drafts: &mut Vec<CanonicalEventDraft>,
) {
    let Some(incoming) = incoming else {
        return;
    };
    let next = if merge {
        merge_dungeon_flow(tracker.flow.clone().unwrap_or_default(), incoming)
    } else {
        incoming
    };
    if tracker.flow.as_ref() == Some(&next) {
        return;
    }
    tracker.flow = Some(next.clone());
    let mut attached_to_boundary = false;

    if let Some(state) = next.state_id {
        let previous = tracker.state.replace(state);
        if previous != Some(state) {
            let transition = match state {
                DUNGEON_STATE_NULL if previous.is_some_and(|value| value != DUNGEON_STATE_NULL) => {
                    Some((DungeonEventKind::Exited, Some(RunState::Exited)))
                }
                DUNGEON_STATE_ACTIVE | DUNGEON_STATE_READY
                    if previous.is_none_or(|value| {
                        matches!(
                            value,
                            DUNGEON_STATE_NULL
                                | DUNGEON_STATE_END
                                | DUNGEON_STATE_SETTLEMENT
                                | DUNGEON_STATE_VOTE
                        )
                    }) =>
                {
                    Some((DungeonEventKind::Entered, None))
                }
                DUNGEON_STATE_PLAYING => Some((DungeonEventKind::Started, Some(RunState::Started))),
                DUNGEON_STATE_END if next.result_id == Some(1) => {
                    Some((DungeonEventKind::Completed, Some(RunState::Completed)))
                }
                DUNGEON_STATE_END => Some((DungeonEventKind::Ended, Some(RunState::Ended))),
                _ => None,
            };
            if let Some((kind, run_state)) = transition {
                let mut event = dungeon_event(tracker, kind);
                event.flow = Some(next.clone());
                drafts.push(dungeon_draft(metadata, event));
                attached_to_boundary = true;
                if let Some(run_state) = run_state {
                    drafts.push(timeline_draft(
                        metadata,
                        TimelineEventKind::RunBoundary {
                            state: run_state,
                            scene_id: None,
                            reason: BoundaryReason::AuthoritativePacket,
                        },
                    ));
                }
            }
        }
    }
    if !attached_to_boundary {
        let mut event = dungeon_event(tracker, DungeonEventKind::FlowUpdated);
        event.flow = Some(next);
        drafts.push(dungeon_draft(metadata, event));
    }
}

fn merge_dungeon_flow(
    mut current: DungeonFlowSnapshot,
    patch: DungeonFlowSnapshot,
) -> DungeonFlowSnapshot {
    macro_rules! replace_some {
        ($field:ident) => {
            if patch.$field.is_some() {
                current.$field = patch.$field;
            }
        };
    }
    replace_some!(state_id);
    replace_some!(phase);
    replace_some!(active_time_raw);
    replace_some!(ready_time_raw);
    replace_some!(play_time_raw);
    replace_some!(end_time_raw);
    replace_some!(settlement_time_raw);
    replace_some!(dungeon_times_raw);
    replace_some!(result_id);
    current
}

fn emit_objective_update(
    metadata: &DecodeMetadata,
    tracker: &DungeonTracker,
    map_key: i32,
    objective: &DungeonObjectiveState,
    drafts: &mut Vec<CanonicalEventDraft>,
) {
    let mut event = dungeon_event(tracker, DungeonEventKind::ObjectiveUpdated);
    event.objective_map_key = Some(map_key);
    event.objective_id = objective.target_id.map(i64::from);
    event.objective_value = objective.value.map(i64::from);
    event.objective_complete = objective.complete.map(|value| value != 0);
    drafts.push(dungeon_draft(metadata, event));
}

fn dungeon_event(tracker: &DungeonTracker, kind: DungeonEventKind) -> DungeonEvent {
    DungeonEvent {
        kind,
        dungeon_id: None,
        instance_id: tracker.instance_id.clone(),
        difficulty_id: tracker.difficulty_id,
        objective_map_key: None,
        objective_id: None,
        objective_value: None,
        objective_complete: None,
        objective_catalog: None,
        flow: None,
    }
}

fn dungeon_draft(metadata: &DecodeMetadata, event: DungeonEvent) -> CanonicalEventDraft {
    draft(
        metadata,
        EventSensitivity::PublicGameplay,
        CanonicalEventDraftKind::Dungeon(event),
    )
}

fn decode_enter_scene(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::EnterScene::decode(payload)?;
    let Some(info) = message.enter_scene_info else {
        return Ok(Vec::new());
    };
    let scene_id = info
        .scene_attrs
        .as_ref()
        .and_then(|attrs| attr_integer(attrs, ATTR_SCENE_ID))
        .and_then(|value| i32::try_from(value).ok())
        .map(SceneId);
    let line_id = info
        .scene_attrs
        .as_ref()
        .and_then(|attrs| attr_integer(attrs, ATTR_SCENE_LINE))
        .and_then(|value| u32::try_from(value).ok());
    let mut drafts = vec![
        draft(
            metadata,
            EventSensitivity::PublicGameplay,
            CanonicalEventDraftKind::WorldChanged(WorldContext {
                scene_id,
                map_id: scene_id.and_then(|id| u32::try_from(id.0).ok()),
                line_id,
                scene_instance_id: info.scene_instance_id,
                dungeon_instance_id: None,
            }),
        ),
        timeline_draft(
            metadata,
            TimelineEventKind::RunBoundary {
                state: RunState::Entered,
                scene_id,
                reason: BoundaryReason::AuthoritativePacket,
            },
        ),
    ];

    if let Some(mut player) = info.player_entity
        && player.uuid.is_some()
    {
        player.entity_type = Some(ENTITY_PLAYER);
        decode_entity(
            player,
            ActorState::Spawned,
            metadata,
            entities,
            status_effects,
            &mut drafts,
        )?;
    }
    Ok(drafts)
}

fn decode_load_scene_end(
    payload: &[u8],
    metadata: &DecodeMetadata,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::NotifyLoadSceneEnd::decode(payload)?;
    let Some(response) = message.response else {
        return Ok(Vec::new());
    };
    let scene_id = response.scene_id.map(SceneId);
    Ok(vec![draft(
        metadata,
        EventSensitivity::PublicGameplay,
        CanonicalEventDraftKind::WorldChanged(WorldContext {
            scene_id,
            map_id: scene_id.and_then(|id| u32::try_from(id.0).ok()),
            line_id: None,
            scene_instance_id: response.scene_instance_id,
            dungeon_instance_id: None,
        }),
    )])
}

fn decode_sync_near_entities(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncNearEntities::decode(payload)?;
    let mut drafts = Vec::new();
    for entity in message.appeared {
        decode_entity(
            entity,
            ActorState::Spawned,
            metadata,
            entities,
            status_effects,
            &mut drafts,
        )?;
    }
    for disappeared in message.disappeared {
        let Some(uuid) = disappeared.uuid else {
            continue;
        };
        status_effects.remove_target(uuid);
        let state = entities.resolve(uuid, None)?;
        drafts.push(timeline_draft(
            metadata,
            TimelineEventKind::Actor(ActorEvent {
                actor: state.identity,
                state: ActorState::Despawned,
                entity_type_id: state.entity_type_id,
                kind: actor_kind(state.entity_type_id),
                monster_id: None,
                character_id: (state.entity_type_id == ENTITY_PLAYER)
                    .then(|| character_id_from_entity_uuid(state.identity.entity_uuid.0))
                    .flatten(),
                display_name: None,
                class_id: None,
                specialization_id: None,
                level: None,
                ability_score: None,
                weapon_item_id: None,
                weapon_breakthrough_count: None,
                seasonal_score: None,
                primary_loadout: Vec::new(),
                auxiliary_loadout: Vec::new(),
                loadout_observation: ActorLoadoutObservation::default(),
            }),
        ));
    }
    Ok(drafts)
}

fn decode_sync_container(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    tracker: &mut ProfileTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncContainerData::decode(payload)?;
    let Some(character) = message.character else {
        return Ok(Vec::new());
    };
    let base = character.base.as_ref();
    let character_id = character
        .character_id
        .or_else(|| base.and_then(|base| base.character_id));
    let mut drafts = Vec::new();

    if let Some(character_id) = character_id {
        let identity = CharacterIdentity {
            region: metadata.region.clone(),
            character_id: character_id.to_string(),
        };
        tracker.local_character = Some(identity.clone());
        let professions = character.professions.as_ref();
        let current_profession_id =
            professions.and_then(|professions| positive_i32(professions.current_profession_id));
        let (combat_professions, active_skills, talents) = container_professions(professions);
        let specialization_id = current_profession_id.and_then(|class_id| {
            container_specialization_id(class_id, combat_professions.as_deref())
        });
        let battle_imagine_skills =
            container_battle_imagine_skills(professions, character.slots.as_ref());
        let equipped_action_slots = container_action_slots(character.slots.as_ref());
        let profile = CharacterProfilePatch {
            character: identity,
            display_name: base.and_then(|base| base.display_name.clone()),
            display_id: base
                .and_then(|base| base.display_id)
                .map(|value| value.to_string()),
            server_id: base
                .and_then(|base| base.server_id)
                .map(|value| value.to_string()),
            class_id: current_profession_id.or_else(|| base.and_then(|base| base.initial_class_id)),
            specialization_id,
            level: character
                .role_level
                .as_ref()
                .and_then(|role| role.level)
                .and_then(|value| u32::try_from(value).ok()),
            progression: container_progression(character.role_level.as_ref()),
            combat_power: base
                .and_then(|base| base.combat_power)
                .or_else(|| character.fight_power.as_ref().and_then(|power| power.total))
                .map(i64::from),
            combat_power_breakdown: container_fight_power(character.fight_power.as_ref()),
            season_strength: None,
            master_score: None,
            season: container_season(
                character.season_center.as_ref(),
                character.season_role_levels.as_ref(),
            ),
            appearance: container_appearance(
                base,
                character.profile_list.as_ref(),
                character.role_face.as_ref(),
                character.personal_zone.as_ref(),
            ),
            equipment: container_equipment(
                character.item_package.as_ref(),
                character.equipment.as_ref(),
            ),
            equipment_suit_entries: container_equipment_suit_entries(character.equipment.as_ref()),
            modules: container_modules(character.item_package.as_ref(), character.modules.as_ref()),
            owned_imagines: None,
            battle_imagine_skills,
            equipped_action_slots,
            active_skills,
            talents,
            talent_progress: container_talent_progress(professions),
            combat_professions,
            life_professions: container_life_professions(character.life_professions.as_ref()),
            cosmetics: None,
            collection_summary: container_collection(
                character.fashion.as_ref(),
                character.collection_book.as_ref(),
                character.personal_zone.as_ref(),
                character.rides.as_ref(),
                character.unlocked_emojis.as_ref(),
                character.handbook.as_ref(),
                character.vanity_pets.as_ref(),
                character.fantasy_atlas.as_ref(),
                character.season_achievements.as_ref(),
            ),
            activity_progress: container_activity_progress(
                character.challenge_dungeons.as_ref(),
                character.master_mode_dungeons.as_ref(),
                character.weekly_tower.as_ref(),
            ),
            season_medals: container_season_medals(character.season_medals.as_ref()),
            season_cultivation: container_season_cultivation(character.season_cultivation.as_ref()),
            reputations: container_reputations(character.reputations.as_ref()),
            current_profession_project_id: character
                .current_profession_project
                .as_ref()
                .and_then(|project| positive_i32(project.project_id)),
            social_display: container_personal_zone(character.personal_zone.as_ref()),
        };
        let (primary_loadout, auxiliary_loadout) = project_actor_loadouts(&profile);
        if let Some(entity_uuid) = character_entity_uuid(character_id) {
            let state = entities.resolve(entity_uuid, Some(ENTITY_PLAYER))?;
            let state = entities
                .observe_combat_identity(entity_uuid, profile.class_id, profile.specialization_id)
                .unwrap_or(state);
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Actor(actor_event_from_profile(
                    state.identity,
                    &profile,
                    primary_loadout,
                    auxiliary_loadout,
                )),
            ));
            if let Some(fight_attributes) = character.fight_attributes.as_ref() {
                if fight_attributes.origin_energy.is_some()
                    || !fight_attributes.resource_ids.is_empty()
                    || !fight_attributes.resources.is_empty()
                {
                    drafts.push(timeline_draft(
                        metadata,
                        TimelineEventKind::Resource(ResourceEvent {
                            actor: state.identity,
                            update_kind: EntityAttributeUpdateKind::Snapshot,
                            origin_energy_raw_bits: fight_attributes
                                .origin_energy
                                .map(f32::to_bits),
                            resource_ids: fight_attributes.resource_ids.clone(),
                            resource_values: fight_attributes.resources.clone(),
                            cooldowns: Vec::new(),
                        }),
                    ));
                }
                for cooldown in &fight_attributes.cooldowns {
                    let Some(skill_id) = cooldown.skill_level_id.filter(|skill_id| *skill_id > 0)
                    else {
                        continue;
                    };
                    drafts.push(timeline_draft(
                        metadata,
                        TimelineEventKind::Cooldown(CooldownEvent {
                            actor: state.identity,
                            ability: AbilityId(i64::from(skill_id)),
                            begin_time_millis: cooldown.skill_begin_time,
                            duration_millis: cooldown.duration,
                            valid_duration_millis: None,
                            cooldown_type: cooldown
                                .cooldown_type
                                .and_then(|value| i32::try_from(value).ok()),
                            profession_hold_begin_time_millis: cooldown.profession_hold_begin_time,
                            charge_count: cooldown.charge_count,
                            valid_cooldown_time_millis: cooldown.valid_cooldown_time,
                            sub_cooldown_ratio_raw: cooldown.sub_cooldown_ratio,
                            sub_cooldown_fixed_raw: cooldown.sub_cooldown_fixed,
                            accelerate_cooldown_ratio_raw: cooldown.accelerate_cooldown_ratio,
                        }),
                    ));
                }
            }
        }
        tracker.local_profile = Some(profile.clone());
        drafts.push(draft(
            metadata,
            EventSensitivity::PersonalGameplay,
            CanonicalEventDraftKind::CharacterProfileObserved {
                profile: Box::new(profile.into_game_event()?),
            },
        ));
    }

    if let Some(scene) = character.scene {
        drafts.push(draft(
            metadata,
            EventSensitivity::PublicGameplay,
            CanonicalEventDraftKind::WorldChanged(WorldContext {
                scene_id: scene
                    .map_id
                    .and_then(|value| i32::try_from(value).ok())
                    .map(SceneId),
                map_id: scene.map_id,
                line_id: scene.line_id.or(scene.channel_id),
                scene_instance_id: scene.scene_instance_id,
                dungeon_instance_id: scene.dungeon_instance_id,
            }),
        ));
    }
    Ok(drafts)
}

fn container_progression(role: Option<&schema::RoleLevel>) -> Option<CharacterProgression> {
    let role = role?;
    let current_experience = role.current_experience.filter(|value| *value >= 0);
    let previous_season_max_level = role
        .previous_season_max_level
        .and_then(|value| u32::try_from(value).ok());
    (current_experience.is_some() || previous_season_max_level.is_some()).then_some(
        CharacterProgression {
            current_experience,
            previous_season_max_level,
        },
    )
}

fn container_appearance(
    base: Option<&schema::CharacterBase>,
    profiles: Option<&schema::ProfileList>,
    role_face: Option<&schema::RoleFace>,
    personal_zone: Option<&schema::PersonalZone>,
) -> Option<CharacterAppearance> {
    let base = base?;
    let face = base.face.as_ref();
    let avatar = base.avatar.as_ref();
    let face_options: BTreeMap<i32, i32> = face
        .map(|face| {
            face.options
                .iter()
                .map(|(key, value)| (*key, *value))
                .collect()
        })
        .unwrap_or_default();
    let color_options: BTreeMap<i32, RgbColor> = face
        .map(|face| {
            face.colors
                .iter()
                .filter_map(|(key, color)| {
                    Some((
                        *key,
                        RgbColor {
                            red: color.x?,
                            green: color.y?,
                            blue: color.z?,
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let height = face
        .and_then(|face| face.height)
        .filter(|height| height.is_finite());
    let body_size_id = base
        .body_size_id
        .or_else(|| face.and_then(|face| face.body_size_id));
    let voice_id = face.and_then(|face| positive_i32(face.voice_id));
    let avatar_id = avatar.and_then(|avatar| positive_i32(avatar.avatar_id));
    let profile_image_url = avatar
        .and_then(|avatar| avatar.profile.as_ref())
        .and_then(|picture| reviewed_picture_url(picture.url.as_deref()));
    let half_body_image_url = avatar
        .and_then(|avatar| avatar.half_body.as_ref())
        .and_then(|picture| reviewed_picture_url(picture.url.as_deref()));
    let business_card_style_id = avatar
        .and_then(|avatar| positive_i32(avatar.business_card_style_id))
        .or_else(|| personal_zone.and_then(|zone| positive_i32(zone.business_card_style_id)));
    let avatar_frame_id = avatar
        .and_then(|avatar| positive_i32(avatar.avatar_frame_id))
        .or_else(|| personal_zone.and_then(|zone| positive_i32(zone.avatar_frame_id)));
    let unlocked_profile_image_ids = profiles
        .map(|profiles| enabled_ids(&profiles.unlocked_profile_ids))
        .unwrap_or_default();
    let unlocked_face_item_ids = role_face
        .map(|face| enabled_ids(&face.unlocked_item_ids))
        .unwrap_or_default();
    let mut unlocked_voice_ids = role_face
        .into_iter()
        .flat_map(|face| face.unlocked_voice_ids.iter())
        .copied()
        .filter(|voice_id| *voice_id > 0)
        .map(i64::from)
        .collect::<Vec<_>>();
    unlocked_voice_ids.sort_unstable();
    unlocked_voice_ids.dedup();

    (base.gender_id.is_some()
        || body_size_id.is_some()
        || height.is_some()
        || voice_id.is_some()
        || !face_options.is_empty()
        || !color_options.is_empty()
        || avatar_id.is_some()
        || profile_image_url.is_some()
        || half_body_image_url.is_some()
        || business_card_style_id.is_some()
        || avatar_frame_id.is_some()
        || !unlocked_profile_image_ids.is_empty()
        || !unlocked_face_item_ids.is_empty()
        || !unlocked_voice_ids.is_empty())
    .then_some(CharacterAppearance {
        gender_id: base.gender_id,
        body_size_id,
        height,
        voice_id,
        face_options,
        color_options,
        avatar_id,
        profile_image_url,
        half_body_image_url,
        business_card_style_id,
        avatar_frame_id,
        unlocked_profile_image_ids,
        unlocked_face_item_ids,
        unlocked_voice_ids,
    })
}

fn container_season(
    center: Option<&schema::SeasonCenter>,
    levels: Option<&schema::SeasonRoleLevelData>,
) -> Option<SeasonProfile> {
    let season_id = center.and_then(|center| positive_i32(center.season_id));
    let level = season_id
        .and_then(|season_id| levels?.levels.get(&season_id))
        .and_then(|level| level.level)
        .and_then(|level| u32::try_from(level).ok());
    let experience = season_id
        .and_then(|season_id| levels?.levels.get(&season_id))
        .and_then(|level| level.current_experience)
        .filter(|experience| *experience >= 0);
    (season_id.is_some() || level.is_some() || experience.is_some()).then_some(SeasonProfile {
        season_id: season_id.map(i64::from),
        level,
        experience,
        power: None,
        strength: None,
    })
}

fn container_fight_power(power: Option<&schema::FightPower>) -> Option<CombatPowerBreakdown> {
    let power = power?;
    let mut components = power
        .components
        .iter()
        .filter_map(|(map_id, component)| {
            let function_type_id = positive_i32(component.function_type_id)
                .or_else(|| (*map_id > 0).then_some(*map_id))?;
            let mut subcomponents = component
                .subcomponents
                .iter()
                .filter_map(|(map_id, subcomponent)| {
                    let function_type_id = positive_i32(subcomponent.function_type_id)
                        .or_else(|| (*map_id > 0).then_some(*map_id))?;
                    Some(CombatPowerSubcomponent {
                        function_type_id,
                        root_function_type_id: positive_i32(subcomponent.root_function_type_id),
                        points: subcomponent.points.map(i64::from),
                    })
                })
                .collect::<Vec<_>>();
            subcomponents.sort_unstable_by_key(|component| component.function_type_id);
            Some(CombatPowerComponent {
                function_type_id,
                total_points: component.total_points.map(i64::from),
                points: component.points.map(i64::from),
                subcomponents,
            })
        })
        .collect::<Vec<_>>();
    components.sort_unstable_by_key(|component| component.function_type_id);
    let total = power.total.map(i64::from);
    (total.is_some() || !components.is_empty())
        .then_some(CombatPowerBreakdown { total, components })
}

fn container_equipment(
    packages: Option<&schema::ItemPackage>,
    equipment: Option<&schema::EquipmentList>,
) -> Option<Vec<EquipmentItem>> {
    let equipment = equipment?;
    let mut items_by_uuid = BTreeMap::new();
    if let Some(packages) = packages {
        for package in packages.packages.values() {
            for (map_uuid, item) in &package.items {
                let uuid = item.uuid.unwrap_or(*map_uuid);
                items_by_uuid.entry(uuid).or_insert(item);
            }
        }
    }

    let mut items = equipment
        .equipped
        .iter()
        .filter_map(|(map_slot, equipped)| {
            let item_uuid = equipped.item_uuid?;
            let signed_uuid = i64::try_from(item_uuid).ok()?;
            let item = items_by_uuid.get(&signed_uuid)?;
            let item_id = positive_i32(item.item_id)?;
            let enchantments = equipment
                .enchantments
                .get(&signed_uuid)
                .and_then(container_enchantment)
                .into_iter()
                .collect::<Vec<_>>();
            Some(EquipmentItem {
                slot_id: equipped.slot_id.unwrap_or(*map_slot),
                item_id: i64::from(item_id),
                instance_id: Some(item_uuid.to_string()),
                level: None,
                quality: item.quality,
                refinement_level: equipped.refinement_level,
                refinement_failed_count: equipped.refinement_failed_count,
                attributes: item
                    .equipment_attributes
                    .as_ref()
                    .and_then(container_equipment_attributes),
                enchantment_ids: enchantments
                    .iter()
                    .map(|enchantment| enchantment.enchantment_id)
                    .collect(),
                enchantments,
                set_id: None,
            })
        })
        .collect::<Vec<_>>();
    items.sort_unstable_by_key(|item| (item.slot_id, item.item_id));
    Some(items)
}

fn container_equipment_suit_entries(
    equipment: Option<&schema::EquipmentList>,
) -> Option<Vec<EquipmentSuitEntryProfile>> {
    let equipment = equipment?;
    let mut entries = equipment
        .suit_entries
        .iter()
        .filter_map(|(map_key, entry)| {
            (*map_key > 0).then_some(EquipmentSuitEntryProfile {
                map_key: *map_key,
                attribute_type: entry.attribute_type,
                attributes: entry
                    .attributes
                    .iter()
                    .map(|(attribute_id, value)| (*attribute_id, *value))
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    entries.sort_unstable_by_key(|entry| entry.map_key);
    Some(entries)
}

fn container_modules(
    packages: Option<&schema::ItemPackage>,
    modules: Option<&schema::ModuleData>,
) -> Option<ModuleProfile> {
    let modules = modules?;
    let equipped_slots = modules
        .equipped_slots
        .iter()
        .filter(|(_, instance_id)| **instance_id != 0)
        .map(|(slot_id, instance_id)| (*slot_id, instance_id.to_string()))
        .collect();

    let mut inventory = packages
        .and_then(|packages| packages.packages.get(&MODULE_PACKAGE_ID))
        .into_iter()
        .flat_map(|package| package.items.iter())
        .filter_map(|(map_instance_id, item)| {
            let instance_id = item.uuid.unwrap_or(*map_instance_id);
            if instance_id == 0 {
                return None;
            }
            let config_id = positive_i32(item.item_id)?;
            let info = modules.module_infos.get(&instance_id);
            let item_parts = item.module_parts.as_ref();
            let part_ids = item_parts
                .filter(|parts| !parts.part_ids.is_empty())
                .map(|parts| parts.part_ids.as_slice())
                .or_else(|| info.map(|info| info.part_ids.as_slice()))
                .unwrap_or_default();
            let parts = part_ids
                .iter()
                .enumerate()
                .filter_map(|(index, part_id)| {
                    (*part_id > 0).then_some(ModulePartProfile {
                        part_id: *part_id,
                        initial_link_points: info
                            .and_then(|info| info.initial_link_points.get(index).copied()),
                    })
                })
                .collect();
            let upgrade_records = item_parts
                .filter(|parts| !parts.upgrade_records.is_empty())
                .map(|parts| parts.upgrade_records.as_slice())
                .or_else(|| info.map(|info| info.upgrade_records.as_slice()))
                .unwrap_or_default()
                .iter()
                .filter_map(|record| {
                    let part_id = positive_i32(record.part_id)?;
                    Some(ModuleUpgradeRecord {
                        part_id,
                        succeeded: record.succeeded,
                    })
                })
                .collect();
            let attributes = item.module_attributes.as_ref();
            Some(ModuleItemProfile {
                instance_id: instance_id.to_string(),
                config_id,
                count: item.count.filter(|count| *count >= 0),
                quality: item.quality,
                load_flag: attributes.and_then(|attributes| attributes.load_flag),
                module_type: attributes.and_then(|attributes| attributes.module_type),
                level: attributes
                    .and_then(|attributes| attributes.level)
                    .and_then(|level| u32::try_from(level).ok()),
                parts,
                upgrade_records,
                success_rate: info.and_then(|info| info.success_rate),
            })
        })
        .collect::<Vec<_>>();
    inventory.sort_unstable_by(|left, right| {
        left.config_id
            .cmp(&right.config_id)
            .then_with(|| left.instance_id.cmp(&right.instance_id))
    });

    Some(ModuleProfile {
        equipped_slots,
        inventory,
    })
}

fn container_enchantment(
    enchantment: &schema::EquipmentEnchantment,
) -> Option<EquipmentEnchantmentProfile> {
    let enchantment_id = positive_i32(enchantment.enchantment_id)?;
    Some(EquipmentEnchantmentProfile {
        enchantment_id: i64::from(enchantment_id),
        level: enchantment
            .level
            .and_then(|level| u32::try_from(level).ok()),
        enchantment_type: enchantment.enchantment_type,
    })
}

fn container_equipment_attributes(
    attributes: &schema::EquipmentAttributes,
) -> Option<EquipmentAttributeProfile> {
    let base = attributes
        .base
        .iter()
        .filter_map(|(key, value)| Some((i32::try_from(*key).ok()?, i64::from(*value))))
        .collect::<BTreeMap<_, _>>();
    let basic = profile_attribute_map(&attributes.basic);
    let advanced = profile_attribute_map(&attributes.advanced);
    let recast = profile_attribute_map(&attributes.recast);
    let rare_quality = profile_attribute_map(&attributes.rare_quality);
    let has_fields = !base.is_empty()
        || !basic.is_empty()
        || !advanced.is_empty()
        || !recast.is_empty()
        || !rare_quality.is_empty()
        || attributes.perfection_value.is_some()
        || attributes.perfection_level.is_some()
        || attributes.max_perfection_value.is_some()
        || attributes.recast_count.is_some()
        || attributes.total_recast_count.is_some()
        || attributes.breakthrough_level.is_some();
    has_fields.then_some(EquipmentAttributeProfile {
        base,
        basic,
        advanced,
        recast,
        rare_quality,
        perfection_value: attributes.perfection_value,
        perfection_level: attributes.perfection_level,
        max_perfection_value: attributes.max_perfection_value,
        recast_count: attributes.recast_count,
        total_recast_count: attributes.total_recast_count,
        breakthrough_count: attributes.breakthrough_level,
    })
}

fn profile_attribute_map(values: &std::collections::HashMap<i32, i32>) -> BTreeMap<i32, i64> {
    values
        .iter()
        .map(|(key, value)| (*key, i64::from(*value)))
        .collect()
}

fn container_talent_progress(
    professions: Option<&schema::ProfessionList>,
) -> Option<TalentProgressProfile> {
    let professions = professions?;
    (professions.total_talent_points.is_some() || professions.total_talent_reset_count.is_some())
        .then_some(TalentProgressProfile {
            total_points: professions.total_talent_points,
            total_reset_count: professions.total_talent_reset_count,
        })
}

type ProfessionContainerProjection = (
    Option<Vec<CombatProfessionProfile>>,
    Option<Vec<SkillLevel>>,
    Option<Vec<TalentLevel>>,
);

fn container_professions(
    professions: Option<&schema::ProfessionList>,
) -> ProfessionContainerProjection {
    let Some(professions) = professions else {
        return (None, None, None);
    };
    let current_id = positive_i32(professions.current_profession_id);
    let mut mapped = professions
        .professions
        .iter()
        .filter_map(|(map_id, profession)| {
            let profession_id = positive_i32(profession.profession_id)
                .or_else(|| (*map_id > 0).then_some(*map_id))?;
            let mut skills = profession
                .skills
                .iter()
                .filter_map(|(map_id, skill)| {
                    let skill_id = positive_i32(skill.skill_id)
                        .or_else(|| (*map_id > 0).then_some(*map_id))?;
                    Some(SkillLevel {
                        skill_id: i64::from(skill_id),
                        base_skill_id: positive_i32(skill.skill_id).map(i64::from),
                        level: skill.level.and_then(|level| u32::try_from(level).ok()),
                        remodel_level: skill
                            .remodel_level
                            .and_then(|level| u32::try_from(level).ok()),
                        skin_id: positive_i32(skill.current_skin_id).map(i64::from),
                        replacement_skill_ids: positive_i32_ids(&skill.replacement_skill_ids),
                        unlocked_skin_ids: enabled_ids(&skill.unlocked_skin_ids),
                    })
                })
                .collect::<Vec<_>>();
            skills.sort_unstable_by_key(|skill| skill.skill_id);
            let mut active_skill_ids = profession
                .active_skill_ids
                .iter()
                .copied()
                .filter(|skill_id| *skill_id > 0)
                .map(i64::from)
                .collect::<Vec<_>>();
            active_skill_ids.sort_unstable();
            active_skill_ids.dedup();
            let slotted_skill_ids = profession
                .slotted_skill_ids
                .iter()
                .filter_map(|(slot, skill_id)| {
                    (*skill_id > 0).then_some((*slot, i64::from(*skill_id)))
                })
                .collect();
            let talent_info = professions.talents.get(&profession_id);
            let mut talent_node_ids = talent_info
                .into_iter()
                .flat_map(|talent| talent.talent_node_ids.iter())
                .copied()
                .filter(|node_id| *node_id > 0)
                .map(i64::from)
                .collect::<Vec<_>>();
            talent_node_ids.sort_unstable();
            talent_node_ids.dedup();
            Some(CombatProfessionProfile {
                profession_id,
                level: profession.level.and_then(|level| u32::try_from(level).ok()),
                experience: profession.experience.filter(|experience| *experience >= 0),
                skills,
                active_skill_ids,
                slotted_skill_ids,
                weapon_skin_id: positive_i32(profession.weapon_skin_id).map(i64::from),
                talent_node_ids,
                talent_points_used: talent_info.and_then(|talent| talent.used_talent_points),
                talent_stage_config_id: talent_info
                    .and_then(|talent| positive_i32(talent.talent_stage_config_id)),
            })
        })
        .collect::<Vec<_>>();
    mapped.sort_unstable_by_key(|profession| profession.profession_id);

    let current = current_id.and_then(|current_id| {
        mapped
            .iter()
            .find(|profession| profession.profession_id == current_id)
    });
    let active_skills = current.map(|profession| {
        profession
            .skills
            .iter()
            .filter(|skill| profession.active_skill_ids.contains(&skill.skill_id))
            .cloned()
            .collect()
    });
    let talents = current.map(|profession| {
        profession
            .talent_node_ids
            .iter()
            .map(|talent_id| TalentLevel {
                talent_id: *talent_id,
                level: None,
            })
            .collect()
    });
    (Some(mapped), active_skills, talents)
}

fn container_specialization_id(
    class_id: i32,
    professions: Option<&[CombatProfessionProfile]>,
) -> Option<i32> {
    let profession = professions?
        .iter()
        .find(|profession| profession.profession_id == class_id)?;
    specialization_from_evidence(
        class_id,
        profession
            .active_skill_ids
            .iter()
            .copied()
            .chain(profession.slotted_skill_ids.values().copied()),
        profession.talent_node_ids.iter().copied(),
    )
    .ok()
    .flatten()
}

fn container_battle_imagine_skills(
    professions: Option<&schema::ProfessionList>,
    slots: Option<&schema::SlotList>,
) -> Option<Vec<BattleImagineSkill>> {
    let professions = professions?;
    let mut skills = professions
        .battle_imagine_skills
        .iter()
        .filter_map(|(map_id, skill)| {
            let skill_id = (*map_id > 0)
                .then_some(*map_id)
                .or_else(|| positive_i32(skill.skill_id))?;
            let equipped_slot = slots.and_then(|slots| {
                let mut matches = slots
                    .slots
                    .iter()
                    .filter_map(|(map_slot, slot)| {
                        let slot_skill_id = positive_i32(slot.skill_id)?;
                        (slot_skill_id == skill_id
                            || positive_i32(skill.skill_id) == Some(slot_skill_id)
                            || skill.replacement_skill_ids.contains(&slot_skill_id))
                        .then(|| positive_i32(slot.slot_id).unwrap_or(*map_slot))
                    })
                    .collect::<Vec<_>>();
                matches.sort_unstable();
                matches.into_iter().next()
            });
            Some(BattleImagineSkill {
                skill_id: i64::from(skill_id),
                base_skill_id: positive_i32(skill.skill_id).map(i64::from),
                level: skill.level.and_then(|level| u32::try_from(level).ok()),
                remodel_level: skill
                    .remodel_level
                    .and_then(|level| u32::try_from(level).ok()),
                skin_id: positive_i32(skill.current_skin_id).map(i64::from),
                replacement_skill_ids: positive_i32_ids(&skill.replacement_skill_ids),
                unlocked_skin_ids: enabled_ids(&skill.unlocked_skin_ids),
                equipped_slot,
            })
        })
        .collect::<Vec<_>>();
    skills.sort_unstable_by_key(|skill| {
        (
            skill.equipped_slot.is_none(),
            skill.equipped_slot,
            skill.skill_id,
        )
    });
    Some(skills)
}

fn container_action_slots(slots: Option<&schema::SlotList>) -> Option<Vec<EquippedActionSlot>> {
    let slots = slots?;
    let mut mapped = slots
        .slots
        .iter()
        .filter_map(|(map_slot, slot)| {
            let skill_id = positive_i32(slot.skill_id)?;
            Some(EquippedActionSlot {
                slot_id: slot.slot_id.unwrap_or(*map_slot),
                skill_id: i64::from(skill_id),
                auto_battle_disabled: slot.auto_battle_disabled,
            })
        })
        .collect::<Vec<_>>();
    mapped.sort_unstable_by_key(|slot| (slot.slot_id, slot.skill_id));
    mapped.dedup_by_key(|slot| (slot.slot_id, slot.skill_id));
    Some(mapped)
}

fn container_life_professions(
    professions: Option<&schema::LifeProfessionList>,
) -> Option<Vec<LifeProfessionProfile>> {
    let professions = professions?;
    let mut mapped = professions
        .professions
        .iter()
        .filter_map(|(map_id, profession)| {
            let profession_id = positive_i32(profession.profession_id)
                .or_else(|| (*map_id > 0).then_some(*map_id))?;
            let specialization_levels = profession
                .specializations
                .iter()
                .filter_map(|(map_id, specialization)| {
                    let specialization_id = positive_i32(specialization.specialization_id)
                        .or_else(|| (*map_id > 0).then_some(*map_id))?;
                    let level = specialization
                        .level
                        .and_then(|level| u32::try_from(level).ok())?;
                    Some((specialization_id, level))
                })
                .collect();
            Some(LifeProfessionProfile {
                profession_id,
                level: profession.level.and_then(|level| u32::try_from(level).ok()),
                experience: profession
                    .experience
                    .filter(|experience| *experience >= 0)
                    .map(i64::from),
                specialization_levels,
            })
        })
        .collect::<Vec<_>>();
    mapped.sort_unstable_by_key(|profession| profession.profession_id);
    Some(mapped)
}

#[allow(clippy::too_many_arguments)]
fn container_collection(
    fashion: Option<&schema::FashionManager>,
    collection_book: Option<&schema::CollectionBook>,
    personal_zone: Option<&schema::PersonalZone>,
    rides: Option<&schema::RideList>,
    emojis: Option<&schema::UnlockEmojiData>,
    handbook: Option<&schema::HandbookData>,
    vanity_pets: Option<&schema::VanityPetManager>,
    fantasy_atlas: Option<&schema::FantasyAtlasData>,
    achievements: Option<&schema::SeasonAchievementList>,
) -> Option<CollectionSummary> {
    if fashion.is_none()
        && collection_book.is_none()
        && personal_zone.is_none()
        && rides.is_none()
        && emojis.is_none()
        && handbook.is_none()
        && vanity_pets.is_none()
        && fantasy_atlas.is_none()
        && achievements.is_none()
    {
        return None;
    }

    let equipped_fashion_ids = fashion
        .into_iter()
        .flat_map(|fashion| fashion.equipped_fashion.iter())
        .filter_map(|(slot, fashion_id)| {
            (*fashion_id > 0).then_some((*slot, i64::from(*fashion_id)))
        })
        .collect();
    let mut ride_ids = rides
        .into_iter()
        .flat_map(|rides| rides.rides.iter())
        .filter_map(|(map_id, ride)| {
            positive_i32(ride.ride_id)
                .or_else(|| (*map_id > 0).then_some(*map_id))
                .map(i64::from)
        })
        .collect::<Vec<_>>();
    ride_ids.sort_unstable();
    ride_ids.dedup();
    let mut ride_skin_ids = rides
        .into_iter()
        .flat_map(|rides| rides.skins.iter())
        .filter_map(|(map_id, skin)| {
            positive_i32(skin.skin_id)
                .or_else(|| (*map_id > 0).then_some(*map_id))
                .map(i64::from)
        })
        .collect::<Vec<_>>();
    ride_skin_ids.sort_unstable();
    ride_skin_ids.dedup();

    let mut vanity_pet_ids = vanity_pets
        .into_iter()
        .flat_map(|pets| pets.pets.values())
        .filter_map(|pet| positive_i32(pet.pet_id).map(i64::from))
        .chain(
            vanity_pets
                .into_iter()
                .flat_map(|pets| enabled_ids(&pets.unlocked_pet_type_ids)),
        )
        .collect::<Vec<_>>();
    vanity_pet_ids.sort_unstable();
    vanity_pet_ids.dedup();
    let summoned_vanity_pet_id = vanity_pets.and_then(|pets| {
        let summoned_instance_id = pets.summon.as_ref()?.summoned_instance_id?;
        pets.pets
            .get(&summoned_instance_id)
            .or_else(|| {
                pets.pets
                    .values()
                    .find(|pet| pet.instance_id == Some(summoned_instance_id))
            })
            .and_then(|pet| positive_i32(pet.pet_id))
            .map(i64::from)
    });

    let fantasy_atlas_stages = fantasy_atlas
        .into_iter()
        .flat_map(|atlas| atlas.entries.iter())
        .filter_map(|(fantasy_id, entry)| {
            let fantasy_id = positive_i32(Some(*fantasy_id))?;
            let stage = entry
                .activated_stage
                .and_then(|stage| u32::try_from(stage).ok())?;
            Some((i64::from(fantasy_id), stage))
        })
        .collect();

    let mut photo_ids = personal_zone
        .into_iter()
        .flat_map(|zone| zone.photos.iter())
        .copied()
        .filter(|photo_id| *photo_id > 0)
        .map(i64::from)
        .collect::<Vec<_>>();
    photo_ids.sort_unstable();
    photo_ids.dedup();

    Some(CollectionSummary {
        fashion_points: fashion
            .and_then(|fashion| fashion.fashion_points)
            .or_else(|| personal_zone.and_then(|zone| zone.fashion_collection_points))
            .map(i64::from),
        mount_points: fashion
            .and_then(|fashion| fashion.mount_points)
            .or_else(|| personal_zone.and_then(|zone| zone.ride_collection_points))
            .map(i64::from),
        weapon_skin_points: fashion
            .and_then(|fashion| fashion.weapon_skin_points)
            .or_else(|| personal_zone.and_then(|zone| zone.weapon_skin_collection_points))
            .map(i64::from),
        equipped_fashion_ids,
        owned_fashion_ids: fashion
            .map(|fashion| enabled_ids(&fashion.owned_fashion))
            .unwrap_or_default(),
        owned_mount_ids: fashion
            .map(|fashion| enabled_ids(&fashion.owned_mounts))
            .unwrap_or_default(),
        owned_weapon_skin_ids: fashion
            .map(|fashion| enabled_ids(&fashion.owned_weapon_skins))
            .unwrap_or_default(),
        owned_dye_ids: fashion
            .map(|fashion| enabled_ids(&fashion.owned_dyes))
            .unwrap_or_default(),
        unlocked_module_ids: collection_book
            .map(|book| enabled_ids(&book.unlocked_module_ids))
            .unwrap_or_default(),
        ride_ids,
        ride_skin_ids,
        unlocked_emoji_ids: emojis
            .map(|emojis| enabled_ids(&emojis.unlocked_ids))
            .unwrap_or_default(),
        vanity_pet_ids,
        summoned_vanity_pet_id,
        fantasy_atlas_stages,
        handbook: handbook.map(container_handbook),
        photo_ids,
        photo_wall: personal_zone
            .into_iter()
            .flat_map(|zone| zone.photos_wall.iter())
            .filter_map(|(slot, photo_id)| {
                (*slot >= 0 && *photo_id > 0).then_some((*slot, i64::from(*photo_id)))
            })
            .collect(),
        achievements: achievements.map(container_achievements),
    })
}

fn container_achievements(source: &schema::SeasonAchievementList) -> AchievementProgressProfile {
    let convert = |entries: &std::collections::HashMap<u32, schema::Achievement>| {
        let mut achievements = entries
            .iter()
            .filter_map(|(map_id, value)| {
                (*map_id > 0).then_some(AchievementProgress {
                    achievement_id: *map_id,
                    finish_count: value.finish_count,
                    reward_claimed: value.reward_claimed,
                    begin_progress: value.begin_progress,
                })
            })
            .collect::<Vec<_>>();
        achievements.sort_unstable_by_key(|entry| entry.achievement_id);
        achievements
    };
    let general = source
        .seasons
        .get(&0)
        .map(|season| convert(&season.achievements))
        .unwrap_or_default();
    let mut seasons = source
        .seasons
        .iter()
        .filter(|(season_id, _)| **season_id > 0)
        .map(|(season_id, season)| SeasonAchievementProgress {
            season_id: *season_id,
            achievements: convert(&season.achievements),
        })
        .collect::<Vec<_>>();
    seasons.sort_unstable_by_key(|season| season.season_id);
    let mut initialized_season_ids = source
        .initialized_seasons
        .iter()
        .filter_map(|(season_id, initialized)| (*initialized).then_some(*season_id))
        .collect::<Vec<_>>();
    initialized_season_ids.sort_unstable();
    initialized_season_ids.dedup();
    AchievementProgressProfile {
        general,
        seasons,
        initialized_season_ids,
        version: source.version,
    }
}

fn container_handbook(handbook: &schema::HandbookData) -> HandbookProgress {
    HandbookProgress {
        important_people_ids: unlocked_handbook_ids(&handbook.important_people),
        reading_book_ids: unlocked_handbook_ids(&handbook.reading_books),
        dictionary_entry_ids: unlocked_handbook_ids(&handbook.dictionary),
        postcard_ids: unlocked_handbook_ids(&handbook.postcards),
        monthly_card_ids: unlocked_handbook_ids(&handbook.monthly_cards),
    }
}

fn unlocked_handbook_ids(
    entries: &std::collections::HashMap<i32, schema::HandbookEntry>,
) -> Vec<i64> {
    let mut ids = entries
        .iter()
        .filter_map(|(map_id, entry)| {
            entry
                .unlocked
                .unwrap_or(false)
                .then(|| positive_i32(entry.entry_id).or_else(|| positive_i32(Some(*map_id))))
                .flatten()
                .map(i64::from)
        })
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn container_activity_progress(
    challenge: Option<&schema::ChallengeDungeonInfo>,
    master_mode: Option<&schema::MasterModeDungeonInfo>,
    weekly_tower: Option<&schema::WeeklyTowerRecord>,
) -> Option<ActivityProgress> {
    if challenge.is_none() && master_mode.is_none() && weekly_tower.is_none() {
        return None;
    }
    let mut challenge_dungeons = challenge
        .into_iter()
        .flat_map(|challenge| challenge.dungeons.iter())
        .filter_map(|(map_id, dungeon)| profile_dungeon(*map_id, dungeon))
        .collect::<Vec<_>>();
    challenge_dungeons.sort_unstable_by_key(|dungeon| dungeon.dungeon_id);
    let mut challenge_targets = challenge
        .into_iter()
        .flat_map(|challenge| challenge.target_awards.iter())
        .flat_map(|(dungeon_id, awards)| {
            awards.targets.iter().filter_map(move |(map_id, target)| {
                let dungeon_id = positive_i32(Some(*dungeon_id))?;
                let target_id =
                    positive_i32(target.target_id).or_else(|| positive_i32(Some(*map_id)))?;
                Some(DungeonTargetProgress {
                    dungeon_id,
                    target_id,
                    progress: target.progress,
                    award_state: target.award_state,
                })
            })
        })
        .collect::<Vec<_>>();
    challenge_targets.sort_unstable_by_key(|target| (target.dungeon_id, target.target_id));
    let mut master_mode_dungeons = master_mode
        .into_iter()
        .flat_map(|master| master.seasons.iter())
        .flat_map(|(season_id, season)| {
            season
                .difficulties
                .iter()
                .flat_map(move |(difficulty_id, difficulty)| {
                    difficulty
                        .dungeons
                        .iter()
                        .filter_map(move |(map_id, dungeon)| {
                            Some(MasterModeDungeonProgress {
                                season_id: positive_i32(Some(*season_id))?,
                                difficulty_id: positive_i32(Some(*difficulty_id))?,
                                dungeon: profile_dungeon(*map_id, dungeon)?,
                            })
                        })
                })
        })
        .collect::<Vec<_>>();
    master_mode_dungeons.sort_unstable_by_key(|entry| {
        (
            entry.season_id,
            entry.difficulty_id,
            entry.dungeon.dungeon_id,
        )
    });
    let weekly_tower = weekly_tower.map(|tower| {
        let mut claimed_floor_ids = tower
            .claimed_floor_ids
            .iter()
            .copied()
            .filter(|floor_id| *floor_id > 0)
            .collect::<Vec<_>>();
        claimed_floor_ids.sort_unstable();
        claimed_floor_ids.dedup();
        WeeklyTowerProgress {
            rule_id: positive_i32(tower.rule_id),
            maximum_floor_id: positive_i32(tower.maximum_floor_id),
            previous_maximum_floor_id: positive_i32(tower.previous_maximum_floor_id),
            claimed_floor_ids,
            maximum_jump_reward_floor_id: positive_i32(tower.maximum_jump_reward_floor_id),
        }
    });
    Some(ActivityProgress {
        challenge_dungeons,
        challenge_targets,
        master_mode_dungeons,
        weekly_tower,
    })
}

fn profile_dungeon(map_id: i32, dungeon: &schema::DungeonProgress) -> Option<DungeonProgress> {
    let dungeon_id = positive_i32(dungeon.dungeon_id).or_else(|| positive_i32(Some(map_id)))?;
    Some(DungeonProgress {
        dungeon_id,
        completion_count: dungeon
            .completion_count
            .and_then(|count| u32::try_from(count).ok()),
        award_state: dungeon.award_state,
        score: dungeon.score,
        pass_time: dungeon.pass_time,
    })
}

fn container_season_medals(medals: Option<&schema::SeasonMedalInfo>) -> Option<SeasonMedalProfile> {
    let medals = medals?;
    let mut normal_holes = medals
        .normal_holes
        .iter()
        .filter_map(|(map_id, hole)| profile_medal_hole(*map_id, hole))
        .collect::<Vec<_>>();
    normal_holes.sort_unstable_by_key(|hole| hole.hole_id);
    let core_hole = medals
        .core_hole
        .as_ref()
        .and_then(|hole| profile_medal_hole(0, hole));
    let mut core_nodes = medals
        .core_nodes
        .iter()
        .filter_map(|(map_id, node)| {
            let node_id = node
                .node_id
                .filter(|node_id| *node_id > 0)
                .or_else(|| (*map_id > 0).then_some(*map_id))?;
            Some(SeasonMedalNode {
                node_id,
                level: node.level,
                selected: node.selected,
                slot_id: node.slot_id,
            })
        })
        .collect::<Vec<_>>();
    core_nodes.sort_unstable_by_key(|node| node.node_id);
    Some(SeasonMedalProfile {
        season_id: medals.season_id,
        normal_holes,
        core_hole,
        core_nodes,
    })
}

fn profile_medal_hole(map_id: u32, hole: &schema::MedalHole) -> Option<SeasonMedalHole> {
    let hole_id = hole
        .hole_id
        .filter(|hole_id| *hole_id > 0)
        .or_else(|| (map_id > 0).then_some(map_id))?;
    Some(SeasonMedalHole {
        hole_id,
        level: hole.level,
        current_experience: hole.current_experience,
    })
}

fn container_season_cultivation(
    cultivation: Option<&schema::SeasonCultivateLineData>,
) -> Option<Vec<SeasonCultivationProfile>> {
    let cultivation = cultivation?;
    let mut seasons = cultivation
        .seasons
        .iter()
        .filter_map(|(season_id, season)| {
            let season_id = positive_i32(Some(*season_id))?;
            let mut lines = season
                .lines
                .iter()
                .filter_map(|(line_type_id, line)| {
                    let line_type_id = positive_i32(Some(*line_type_id))?;
                    let mut area_ids = line
                        .area_ids
                        .iter()
                        .copied()
                        .filter(|area_id| *area_id > 0)
                        .collect::<Vec<_>>();
                    area_ids.sort_unstable();
                    area_ids.dedup();
                    let mut areas = line
                        .areas
                        .iter()
                        .filter_map(|(area_id, area)| {
                            let area_id = positive_i32(Some(*area_id))?;
                            let normal_node_levels = area
                                .normal_nodes
                                .iter()
                                .filter_map(|(node_id, node)| {
                                    Some((
                                        positive_i32(Some(*node_id))?,
                                        node.active_level
                                            .and_then(|level| u32::try_from(level).ok())?,
                                    ))
                                })
                                .collect();
                            let middle_node_item_ids = area
                                .middle_nodes
                                .iter()
                                .filter_map(|(node_id, node)| {
                                    Some((
                                        positive_i32(Some(*node_id))?,
                                        i64::from(positive_i32(node.item_id)?),
                                    ))
                                })
                                .collect();
                            let big_node_fantasy_ids = area
                                .big_nodes
                                .iter()
                                .filter_map(|(node_id, node)| {
                                    Some((
                                        positive_i32(Some(*node_id))?,
                                        i64::from(positive_i32(node.fantasy_id)?),
                                    ))
                                })
                                .collect();
                            Some(CultivationAreaProfile {
                                area_id,
                                active: area.active,
                                active_effect_score: area.active_effect_score,
                                normal_node_levels,
                                middle_node_item_ids,
                                big_node_fantasy_ids,
                            })
                        })
                        .collect::<Vec<_>>();
                    areas.sort_unstable_by_key(|area| area.area_id);
                    Some(CultivationLineProfile {
                        line_type_id,
                        area_ids,
                        areas,
                    })
                })
                .collect::<Vec<_>>();
            lines.sort_unstable_by_key(|line| line.line_type_id);
            Some(SeasonCultivationProfile { season_id, lines })
        })
        .collect::<Vec<_>>();
    seasons.sort_unstable_by_key(|season| season.season_id);
    Some(seasons)
}

fn container_reputations(
    reputations: Option<&schema::ReputationList>,
) -> Option<Vec<ReputationProgress>> {
    let reputations = reputations?;
    let mut mapped = reputations
        .reputations
        .iter()
        .filter_map(|(reputation_id, reputation)| {
            (*reputation_id > 0).then_some(ReputationProgress {
                reputation_id: *reputation_id,
                level: reputation.level.and_then(|level| u32::try_from(level).ok()),
                experience: reputation.experience,
            })
        })
        .collect::<Vec<_>>();
    mapped.sort_unstable_by_key(|reputation| reputation.reputation_id);
    Some(mapped)
}

fn container_personal_zone(zone: Option<&schema::PersonalZone>) -> Option<SocialDisplay> {
    let zone = zone?;
    let medal_slots = zone
        .medals
        .iter()
        .filter_map(|(slot, medal_id)| {
            positive_i32(Some(*medal_id)).map(|medal_id| (*slot, i64::from(medal_id)))
        })
        .collect::<BTreeMap<_, _>>();
    let mut medal_ids = medal_slots.values().copied().collect::<Vec<_>>();
    medal_ids.sort_unstable();
    medal_ids.dedup();
    let title_ids = positive_i32(zone.title_id)
        .map(|title_id| vec![i64::from(title_id)])
        .unwrap_or_default();
    let profile_theme_id = positive_i32(zone.theme_id).map(i64::from);
    (!title_ids.is_empty() || !medal_ids.is_empty() || profile_theme_id.is_some()).then_some(
        SocialDisplay {
            guild_id: None,
            guild_name: None,
            title_ids,
            medal_ids,
            medal_slots,
            profile_theme_id,
        },
    )
}

fn enabled_ids(values: &std::collections::HashMap<i32, bool>) -> Vec<i64> {
    let mut ids = values
        .iter()
        .filter_map(|(id, enabled)| (*enabled && *id > 0).then_some(i64::from(*id)))
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

fn positive_i32_ids(values: &[i32]) -> Vec<i64> {
    let mut ids = values
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .map(i64::from)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn apply_dirty_action_slots(
    profile: &mut CharacterProfilePatch,
    update: &dirty_blob_v1::DirtyActionSlotUpdate,
) {
    let mut slots = if update.replace {
        BTreeMap::new()
    } else {
        profile
            .equipped_action_slots
            .as_deref()
            .unwrap_or_default()
            .iter()
            .copied()
            .map(|slot| (slot.slot_id, slot))
            .collect::<BTreeMap<_, _>>()
    };

    for removed in &update.removals {
        slots.remove(removed);
    }
    for entry in &update.upserts {
        let slot_id = entry.slot_id.unwrap_or(entry.map_key);
        let previous = slots
            .remove(&entry.map_key)
            .or_else(|| slots.remove(&slot_id));
        let skill_id = entry
            .skill_id
            .map(i64::from)
            .or_else(|| previous.map(|slot| slot.skill_id));
        let Some(skill_id) = skill_id.filter(|skill_id| *skill_id > 0) else {
            continue;
        };
        slots.insert(
            slot_id,
            EquippedActionSlot {
                slot_id,
                skill_id,
                auto_battle_disabled: entry
                    .auto_battle_disabled
                    .or_else(|| previous.and_then(|slot| slot.auto_battle_disabled)),
            },
        );
    }
    profile.equipped_action_slots = Some(slots.into_values().collect());
}

fn apply_dirty_battle_imagine_skills(
    profile: &mut CharacterProfilePatch,
    update: &dirty_blob_v1::DirtyBattleImagineSkillUpdate,
) {
    let mut skills = if update.replace {
        BTreeMap::new()
    } else {
        profile
            .battle_imagine_skills
            .as_deref()
            .unwrap_or_default()
            .iter()
            .cloned()
            .map(|skill| (skill.skill_id, skill))
            .collect::<BTreeMap<_, _>>()
    };

    for removed in &update.removals {
        let removed = i64::from(*removed);
        skills
            .retain(|skill_id, skill| *skill_id != removed && skill.base_skill_id != Some(removed));
    }

    for entry in &update.upserts {
        let map_skill_id = (entry.map_key > 0).then_some(i64::from(entry.map_key));
        let nested_skill_id = entry
            .skill_id
            .filter(|skill_id| *skill_id > 0)
            .map(i64::from);
        let existing_key = map_skill_id
            .filter(|skill_id| skills.contains_key(skill_id))
            .or_else(|| nested_skill_id.filter(|skill_id| skills.contains_key(skill_id)))
            .or_else(|| {
                skills.iter().find_map(|(skill_id, skill)| {
                    (skill.base_skill_id == nested_skill_id).then_some(*skill_id)
                })
            });
        let previous = existing_key.and_then(|skill_id| skills.remove(&skill_id));
        let Some(skill_id) = map_skill_id
            .or_else(|| previous.as_ref().map(|skill| skill.skill_id))
            .or(nested_skill_id)
        else {
            continue;
        };

        let mut unlocked_skin_ids = previous
            .as_ref()
            .map(|skill| {
                skill
                    .unlocked_skin_ids
                    .iter()
                    .copied()
                    .map(|skin_id| (skin_id, true))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        if let Some(unlocked_update) = entry.unlocked_skin_ids.as_ref() {
            if unlocked_update.replace {
                unlocked_skin_ids.clear();
            }
            for removed in &unlocked_update.removals {
                unlocked_skin_ids.remove(&i64::from(*removed));
            }
            for (skin_id, enabled) in &unlocked_update.upserts {
                if *skin_id <= 0 {
                    continue;
                }
                if *enabled {
                    unlocked_skin_ids.insert(i64::from(*skin_id), true);
                } else {
                    unlocked_skin_ids.remove(&i64::from(*skin_id));
                }
            }
        }

        let previous_level = previous.as_ref().and_then(|skill| skill.level);
        let previous_remodel_level = previous.as_ref().and_then(|skill| skill.remodel_level);
        let previous_skin_id = previous.as_ref().and_then(|skill| skill.skin_id);
        let previous_replacements = previous
            .as_ref()
            .map(|skill| skill.replacement_skill_ids.clone())
            .unwrap_or_default();
        skills.insert(
            skill_id,
            BattleImagineSkill {
                skill_id,
                base_skill_id: nested_skill_id
                    .or_else(|| previous.as_ref().and_then(|skill| skill.base_skill_id)),
                level: entry
                    .level
                    .and_then(|level| u32::try_from(level).ok())
                    .or(previous_level),
                remodel_level: entry
                    .remodel_level
                    .and_then(|level| u32::try_from(level).ok())
                    .or(previous_remodel_level),
                skin_id: match entry.current_skin_id {
                    Some(skin_id) => positive_i32(Some(skin_id)).map(i64::from),
                    None => previous_skin_id,
                },
                replacement_skill_ids: entry
                    .replacement_skill_ids
                    .as_deref()
                    .map(positive_i32_ids)
                    .unwrap_or(previous_replacements),
                unlocked_skin_ids: unlocked_skin_ids.into_keys().collect(),
                equipped_slot: None,
            },
        );
    }

    profile.battle_imagine_skills = Some(skills.into_values().collect());
}

fn reproject_battle_imagine_slots(profile: &mut CharacterProfilePatch) {
    let action_slots = profile.equipped_action_slots.as_deref().unwrap_or_default();
    let Some(skills) = profile.battle_imagine_skills.as_mut() else {
        return;
    };
    for skill in skills.iter_mut() {
        skill.equipped_slot = action_slots
            .iter()
            .filter(|slot| {
                slot.skill_id == skill.skill_id
                    || skill.base_skill_id == Some(slot.skill_id)
                    || skill.replacement_skill_ids.contains(&slot.skill_id)
            })
            .map(|slot| slot.slot_id)
            .min();
    }
    skills.sort_unstable_by_key(|skill| {
        (
            skill.equipped_slot.is_none(),
            skill.equipped_slot,
            skill.skill_id,
        )
    });
}

fn decode_sync_container_dirty(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    tracker: &mut ProfileTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncContainerDirtyData::decode(payload)?;
    let Some(stream) = message.data else {
        return Ok(Vec::new());
    };
    let Some(buffer) = stream.buffer.filter(|buffer| !buffer.is_empty()) else {
        return Ok(Vec::new());
    };
    let update =
        dirty_blob_v1::decode_character_update(&buffer, stream.stream_type.unwrap_or_default())?;

    let identity = tracker.local_character.clone().filter(|identity| {
        update
            .character_id
            .is_none_or(|character_id| identity.character_id == character_id.to_string())
    });
    let profile = identity
        .as_ref()
        .filter(|_| update.has_public_profile_fields())
        .map(|character| {
            let appearance = if update.gender_id.is_some()
                || update.body_size_id.is_some()
                || update.avatar_id.is_some()
                || update.business_card_style_id.is_some()
                || update.avatar_frame_id.is_some()
            {
                Some(CharacterAppearance {
                    gender_id: update.gender_id,
                    body_size_id: update.body_size_id,
                    height: None,
                    voice_id: None,
                    face_options: BTreeMap::new(),
                    color_options: BTreeMap::new(),
                    avatar_id: positive_i32(update.avatar_id),
                    profile_image_url: None,
                    half_body_image_url: None,
                    business_card_style_id: positive_i32(update.business_card_style_id),
                    avatar_frame_id: positive_i32(update.avatar_frame_id),
                    unlocked_profile_image_ids: Vec::new(),
                    unlocked_face_item_ids: Vec::new(),
                    unlocked_voice_ids: Vec::new(),
                })
            } else {
                None
            };
            let progression = if update.current_experience.is_some()
                || update.previous_season_max_level.is_some()
            {
                Some(CharacterProgression {
                    current_experience: update.current_experience,
                    previous_season_max_level: update.previous_season_max_level,
                })
            } else {
                None
            };
            CharacterProfilePatch {
                character: character.clone(),
                display_name: clean_text(update.display_name.as_deref()),
                display_id: update
                    .display_id
                    .filter(|display_id| *display_id > 0)
                    .map(|display_id| display_id.to_string()),
                server_id: update
                    .server_id
                    .filter(|server_id| *server_id > 0)
                    .map(|server_id| server_id.to_string()),
                class_id: positive_i32(update.class_id),
                specialization_id: None,
                level: update.level,
                progression,
                combat_power: update.combat_power.filter(|combat_power| *combat_power > 0),
                combat_power_breakdown: None,
                season_strength: None,
                master_score: None,
                season: None,
                appearance,
                equipment: None,
                equipment_suit_entries: None,
                modules: None,
                owned_imagines: None,
                battle_imagine_skills: None,
                equipped_action_slots: None,
                active_skills: None,
                talents: None,
                talent_progress: None,
                combat_professions: None,
                life_professions: None,
                cosmetics: None,
                collection_summary: None,
                activity_progress: None,
                season_medals: None,
                season_cultivation: None,
                reputations: None,
                current_profession_project_id: None,
                social_display: None,
            }
        });

    let world = update.world.has_public_fields().then(|| {
        let map_id = update.world.map_id.or(update.world.level_map_id);
        WorldContext {
            scene_id: map_id
                .and_then(|map_id| i32::try_from(map_id).ok())
                .map(SceneId),
            map_id,
            line_id: update.world.line_id.or(update.world.channel_id),
            scene_instance_id: clean_text(update.world.scene_instance_id.as_deref()),
            dungeon_instance_id: clean_text(update.world.dungeon_instance_id.as_deref()),
        }
    });

    let mut drafts = Vec::with_capacity(4usize.saturating_add(update.cooldowns.len()));
    let cooldown_actor_uuid = tracker.local_entity_uuid.or_else(|| {
        tracker
            .local_character
            .as_ref()
            .and_then(|character| character.character_id.parse::<i64>().ok())
            .and_then(character_entity_uuid)
    });
    if let Some(actor_uuid) = cooldown_actor_uuid {
        let actor = entities.resolve(actor_uuid, Some(ENTITY_PLAYER))?.identity;
        if update.origin_energy_raw_bits.is_some()
            || !update.resource_ids.is_empty()
            || !update.resource_values.is_empty()
        {
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Resource(ResourceEvent {
                    actor,
                    update_kind: EntityAttributeUpdateKind::Delta,
                    origin_energy_raw_bits: update.origin_energy_raw_bits,
                    resource_ids: update.resource_ids.clone(),
                    resource_values: update.resource_values.clone(),
                    cooldowns: Vec::new(),
                }),
            ));
        }
        for cooldown in &update.cooldowns {
            let Some(skill_id) = cooldown.skill_level_id.filter(|skill_id| *skill_id > 0) else {
                continue;
            };
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Cooldown(CooldownEvent {
                    actor,
                    ability: AbilityId(i64::from(skill_id)),
                    begin_time_millis: cooldown.skill_begin_time,
                    duration_millis: cooldown.duration,
                    valid_duration_millis: None,
                    cooldown_type: cooldown
                        .cooldown_type
                        .and_then(|value| i32::try_from(value).ok()),
                    profession_hold_begin_time_millis: cooldown.profession_hold_begin_time,
                    charge_count: cooldown.charge_count,
                    valid_cooldown_time_millis: cooldown.valid_cooldown_time,
                    sub_cooldown_ratio_raw: cooldown.sub_cooldown_ratio,
                    sub_cooldown_fixed_raw: cooldown.sub_cooldown_fixed,
                    accelerate_cooldown_ratio_raw: cooldown.accelerate_cooldown_ratio,
                }),
            ));
        }
    }
    if update.has_loadout_fields()
        && let Some(cached_profile) = tracker.local_profile.as_mut()
        && identity
            .as_ref()
            .is_some_and(|identity| identity == &cached_profile.character)
    {
        if let Some(action_slots) = update.action_slots.as_ref() {
            apply_dirty_action_slots(cached_profile, action_slots);
        }
        if let Some(battle_imagine_skills) = update.battle_imagine_skills.as_ref() {
            apply_dirty_battle_imagine_skills(cached_profile, battle_imagine_skills);
        }
        reproject_battle_imagine_slots(cached_profile);
        let complete_profile = cached_profile.clone();
        let (primary_loadout, auxiliary_loadout) = project_actor_loadouts(&complete_profile);
        if let Some(actor_uuid) = cooldown_actor_uuid {
            let state = entities.resolve(actor_uuid, Some(ENTITY_PLAYER))?;
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Actor(actor_event_from_profile(
                    state.identity,
                    &complete_profile,
                    primary_loadout,
                    auxiliary_loadout,
                )),
            ));
        }
        tracker.last_dirty_profile = Some(complete_profile.clone());
        drafts.push(draft(
            metadata,
            EventSensitivity::PersonalGameplay,
            CanonicalEventDraftKind::CharacterProfileObserved {
                profile: Box::new(complete_profile.into_game_event()?),
            },
        ));
    }
    if let Some(profile) = profile
        && tracker.last_dirty_profile.as_ref() != Some(&profile)
    {
        tracker.last_dirty_profile = Some(profile.clone());
        drafts.push(draft(
            metadata,
            EventSensitivity::PersonalGameplay,
            CanonicalEventDraftKind::CharacterProfileObserved {
                profile: Box::new(profile.into_game_event()?),
            },
        ));
    }
    if let Some(world) = world
        && tracker.last_dirty_world.as_ref() != Some(&world)
    {
        tracker.last_dirty_world = Some(world.clone());
        drafts.push(draft(
            metadata,
            EventSensitivity::PublicGameplay,
            CanonicalEventDraftKind::WorldChanged(world),
        ));
    }
    Ok(drafts)
}

fn decode_sync_near_delta(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncNearDeltaInfo::decode(payload)?;
    let mut drafts = Vec::new();
    for (group_index, delta) in message.deltas.into_iter().enumerate() {
        decode_aoi_delta(
            delta,
            None,
            u32::try_from(group_index).ok(),
            metadata,
            entities,
            status_effects,
            &mut drafts,
        )?;
    }
    Ok(drafts)
}

fn decode_sync_to_me_delta(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
    profile: &mut ProfileTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncToMeDeltaInfo::decode(payload)?;
    let Some(delta) = message.delta else {
        return Ok(Vec::new());
    };
    let local_uuid = delta
        .uuid
        .or_else(|| delta.base_delta.as_ref().and_then(|base| base.uuid));
    let mut drafts = Vec::new();
    if let Some(base_delta) = delta.base_delta {
        decode_aoi_delta(
            base_delta,
            Some(ENTITY_PLAYER),
            Some(0),
            metadata,
            entities,
            status_effects,
            &mut drafts,
        )?;
    }
    if let Some(uuid) = local_uuid {
        profile.local_entity_uuid = Some(uuid);
        let actor = entities.resolve(uuid, Some(ENTITY_PLAYER))?.identity;
        if !delta.fight_resource_cooldowns.is_empty() {
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Resource(ResourceEvent {
                    actor,
                    update_kind: EntityAttributeUpdateKind::Delta,
                    origin_energy_raw_bits: None,
                    resource_ids: Vec::new(),
                    resource_values: Vec::new(),
                    cooldowns: delta
                        .fight_resource_cooldowns
                        .iter()
                        .map(|cooldown| ResourceCooldown {
                            resource_id: cooldown.resource_id,
                            begin_time_millis: cooldown.begin_time,
                            duration_millis: cooldown.duration,
                            valid_cooldown_time_millis: cooldown.valid_cooldown_time,
                            existence_time_millis: cooldown.existence_time,
                        })
                        .collect(),
                }),
            ));
        }
        for cooldown in delta.cooldowns {
            let Some(skill_id) = cooldown.skill_level_id else {
                continue;
            };
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Cooldown(CooldownEvent {
                    actor,
                    ability: AbilityId(i64::from(skill_id)),
                    begin_time_millis: cooldown.begin_time,
                    duration_millis: cooldown.duration,
                    valid_duration_millis: cooldown.valid_duration,
                    cooldown_type: cooldown.cooldown_type,
                    profession_hold_begin_time_millis: None,
                    charge_count: None,
                    valid_cooldown_time_millis: None,
                    sub_cooldown_ratio_raw: None,
                    sub_cooldown_fixed_raw: None,
                    accelerate_cooldown_ratio_raw: None,
                }),
            ));
        }
    }
    Ok(drafts)
}

fn decode_sync_client_use_skill(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    profile: &ProfileTracker,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncClientUseSkill::decode(payload)?;
    let Some(source_uuid) = profile.local_entity_uuid else {
        // The notification does not carry its source because it is implicitly
        // the local player. Refuse to guess until SyncToMeDelta has established
        // that exact scene entity.
        return Ok(Vec::new());
    };
    let Some(skill_level_id) = message.skill_level_id.filter(|value| *value > 0) else {
        return Ok(Vec::new());
    };
    let source = entities.resolve(source_uuid, Some(ENTITY_PLAYER))?.identity;
    let target = match message.skill_target_uuid.filter(|value| *value > 0) {
        Some(target_uuid) => Some(entities.resolve(target_uuid, None)?.identity),
        None => None,
    };
    Ok(vec![timeline_draft(
        metadata,
        TimelineEventKind::Cast(CastEvent {
            source,
            ability: AbilityId(i64::from(skill_level_id)),
            target,
            state: CastState::Started,
            action_timing: None,
        }),
    )])
}

fn decode_world_use_slot_skill_action(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    profile: &ProfileTracker,
    client_build: &str,
    scratch: &mut Vec<u8>,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let Some(action) =
        crate::decode_world_use_slot_skill_action_into(client_build, payload, scratch)?
    else {
        return Ok(Vec::new());
    };
    let Some(source_uuid) = profile.local_entity_uuid else {
        // UseSlot carries an implicit local source. Keep the route decoded but
        // emit nothing until an exact SyncToMeDelta entity establishes it.
        return Ok(Vec::new());
    };
    let source = entities.resolve(source_uuid, Some(ENTITY_PLAYER))?.identity;
    let target = match action.param.target_uuid {
        target_uuid if target_uuid > 0 => Some(entities.resolve(target_uuid, None)?.identity),
        _ => None,
    };
    let Some(cast) = action.canonical_cast_started(source, target) else {
        return Ok(Vec::new());
    };
    Ok(vec![timeline_draft(
        metadata,
        TimelineEventKind::Cast(cast),
    )])
}

fn decode_revive(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::NotifyReviveUser::decode(payload)?;
    let Some(uuid) = message.actor_uuid else {
        return Ok(Vec::new());
    };
    let actor = entities.resolve(uuid, None)?.identity;
    Ok(vec![timeline_draft(
        metadata,
        TimelineEventKind::Life {
            actor,
            state: LifeState::Revived,
        },
    )])
}

fn decode_entity(
    entity: schema::Entity,
    actor_state: ActorState,
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    let Some(uuid) = entity.uuid else {
        return Ok(());
    };
    let passive_skill_infos = entity.passive_skill_infos.as_ref();
    let buff_infos = entity.buff_infos.as_ref();
    let buff_effect = entity.buff_effect.as_ref();
    let raw_temp_attributes = entity.raw_temp_attributes.as_deref();
    let state = entities.resolve(uuid, entity.entity_type)?;
    let attributes_present = entity.attributes.is_some();
    let attributes = entity.attributes.unwrap_or_default();
    let name = attr_text(&attributes, ATTR_NAME);
    let monster_id = attr_integer(&attributes, ATTR_MONSTER_ID).map(MonsterId);
    let class_id =
        attr_integer(&attributes, ATTR_CLASS_ID).and_then(|value| i32::try_from(value).ok());
    let level = attr_integer(&attributes, ATTR_LEVEL).and_then(|value| u32::try_from(value).ok());
    let ability_score = attr_integer(&attributes, ATTR_COMBAT_POWER);
    let seasonal_score = attr_integer(&attributes, ATTR_SEASON_STRENGTH);
    let weapon_item_id = attr_weapon_item_id(&attributes);
    let skill_levels = attr_player_skill_levels(&attributes);
    let (primary_loadout, auxiliary_loadout, loadout_observation) =
        attr_player_loadouts(&attributes, &skill_levels);
    let mut state = entities
        .observe_combat_identity(uuid, class_id, None)
        .unwrap_or(state);
    if let Some(passive_skill_infos) = passive_skill_infos
        && let Some(observed) = entities.observe_specialization_passives(uuid, passive_skill_infos)
    {
        state = observed;
    }

    let kind = actor_kind(state.entity_type_id);
    drafts.push(timeline_draft(
        metadata,
        TimelineEventKind::Actor(ActorEvent {
            actor: state.identity,
            state: actor_state,
            entity_type_id: state.entity_type_id,
            kind,
            monster_id,
            character_id: (kind == ActorKind::Player)
                .then(|| character_id_from_entity_uuid(state.identity.entity_uuid.0))
                .flatten(),
            display_name: name,
            class_id: state.class_id.or(class_id),
            specialization_id: state.specialization_id,
            level,
            ability_score,
            weapon_item_id,
            weapon_breakthrough_count: None,
            seasonal_score,
            primary_loadout,
            auxiliary_loadout,
            loadout_observation,
        }),
    ));
    if attributes_present {
        emit_attributes(
            state.identity,
            &attributes,
            EntityAttributeUpdateKind::Snapshot,
            metadata,
            drafts,
        );
    }
    emit_position(state.identity, &attributes, metadata, drafts);
    emit_temporary_attributes(
        state.identity,
        raw_temp_attributes,
        EntityAttributeUpdateKind::Snapshot,
        metadata,
        drafts,
    )?;
    if let Some(buff_infos) = buff_infos {
        decode_status_snapshot(buff_infos, uuid, metadata, entities, status_effects, drafts)?;
    }
    if let Some(buff_effect) = buff_effect {
        decode_status_effects(
            buff_effect,
            uuid,
            metadata,
            entities,
            status_effects,
            drafts,
        )?;
    }
    Ok(())
}

fn decode_unresolved_fake_bullets(
    container_uuid: Option<i64>,
    raw_records: &[Vec<u8>],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    if raw_records.is_empty() {
        return Ok(());
    }

    let container = container_uuid
        .map(|uuid| entities.resolve(uuid, None).map(|state| state.identity))
        .transpose()?;

    for raw_payload in raw_records {
        let decoded = schema::FakeBulletInfo::decode(raw_payload.as_slice());
        let (target, action_instance_id, action_id, target_part_id, reason) = match decoded {
            Ok(info) => {
                let target = info
                    .target_id
                    .filter(|target_uuid| *target_uuid > 0)
                    .map(|target_uuid| {
                        entities
                            .resolve(target_uuid, None)
                            .map(|state| state.identity)
                    })
                    .transpose()?;
                let reason = if container.is_some() {
                    UnresolvedActionReason::ProviderOwnershipUnproven
                } else {
                    UnresolvedActionReason::MissingContainerIdentity
                };
                (
                    target,
                    info.uuid.map(i64::from),
                    info.bullet_id.map(i64::from),
                    info.part_id,
                    reason,
                )
            }
            Err(_) => (
                None,
                None,
                None,
                None,
                UnresolvedActionReason::PayloadDecodeFailed,
            ),
        };

        drafts.push(timeline_draft(
            metadata,
            TimelineEventKind::UnresolvedAction(UnresolvedActionEvent {
                container,
                target,
                action_instance_id,
                action_id,
                target_part_id,
                wire_action_type: Some(crate::BpsrDamageSourceKind::FakeBullet.protocol_id()),
                reason,
                raw_payload: raw_payload.clone(),
            }),
        ));
    }
    Ok(())
}

fn decode_aoi_delta(
    delta: schema::AoiSyncDelta,
    entity_type_id: Option<i32>,
    skill_effect_group_index: Option<u32>,
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    decode_unresolved_fake_bullets(
        delta.uuid,
        &delta.raw_fake_bullets,
        metadata,
        entities,
        drafts,
    )?;
    let Some(target_uuid) = delta.uuid else {
        return Ok(());
    };
    let mut target = entities.resolve(target_uuid, entity_type_id)?;
    let observed_class_id = delta.attributes.as_ref().and_then(|attributes| {
        attr_integer(attributes, ATTR_CLASS_ID).and_then(|value| i32::try_from(value).ok())
    });
    if observed_class_id.is_some() {
        target = entities
            .observe_combat_identity(target_uuid, observed_class_id, None)
            .unwrap_or(target);
    }
    if let Some(passive_skill_infos) = delta.passive_skill_infos.as_ref() {
        let actor_uuid = passive_skill_infos.actor_uuid.unwrap_or(target_uuid);
        entities.resolve(
            actor_uuid,
            (actor_uuid == target_uuid).then_some(target.entity_type_id),
        )?;
        if let Some(observed) =
            entities.observe_specialization_passives(actor_uuid, passive_skill_infos)
        {
            if actor_uuid != target_uuid || delta.attributes.is_none() {
                drafts.push(specialization_actor_draft(metadata, observed));
            }
            if actor_uuid == target_uuid {
                target = observed;
            }
        }
    }
    if let Some(attributes) = &delta.attributes {
        let weapon_item_id = attr_weapon_item_id(attributes);
        let skill_levels = attr_player_skill_levels(attributes);
        let (primary_loadout, auxiliary_loadout, loadout_observation) =
            attr_player_loadouts(attributes, &skill_levels);
        let target = entities
            .observe_combat_identity(target_uuid, observed_class_id, None)
            .unwrap_or(target);
        let kind = actor_kind(target.entity_type_id);
        drafts.push(timeline_draft(
            metadata,
            TimelineEventKind::Actor(ActorEvent {
                actor: target.identity,
                state: ActorState::Updated,
                entity_type_id: target.entity_type_id,
                kind,
                monster_id: attr_integer(attributes, ATTR_MONSTER_ID).map(MonsterId),
                character_id: (kind == ActorKind::Player)
                    .then(|| character_id_from_entity_uuid(target.identity.entity_uuid.0))
                    .flatten(),
                display_name: attr_text(attributes, ATTR_NAME),
                class_id: target.class_id.or(observed_class_id),
                specialization_id: target.specialization_id,
                level: attr_integer(attributes, ATTR_LEVEL)
                    .and_then(|value| u32::try_from(value).ok()),
                ability_score: attr_integer(attributes, ATTR_COMBAT_POWER),
                weapon_item_id,
                weapon_breakthrough_count: None,
                seasonal_score: attr_integer(attributes, ATTR_SEASON_STRENGTH),
                primary_loadout,
                auxiliary_loadout,
                loadout_observation,
            }),
        ));
        emit_attributes(
            target.identity,
            attributes,
            EntityAttributeUpdateKind::Delta,
            metadata,
            drafts,
        );
        emit_position(target.identity, attributes, metadata, drafts);
    }

    emit_temporary_attributes(
        target.identity,
        delta.raw_temp_attributes.as_deref(),
        EntityAttributeUpdateKind::Delta,
        metadata,
        drafts,
    )?;

    if let Some(buff_effect) = delta.buff_effect {
        decode_status_effects(
            &buff_effect,
            target_uuid,
            metadata,
            entities,
            status_effects,
            drafts,
        )?;
    }

    let Some(effect) = delta.skill_effects else {
        return Ok(());
    };
    let skill_effect_uuid = effect.uuid;
    let skill_effect_total_damage = effect.total_damage;
    let skill_effect_component_count = u32::try_from(effect.damage.len()).ok();
    for (component_index, damage) in effect.damage.into_iter().enumerate() {
        let Some(direct_uuid) = damage.attacker_uuid.or(damage.top_summoner_uuid) else {
            continue;
        };
        let attributed_uuid = damage.top_summoner_uuid.unwrap_or(direct_uuid);
        let source = entities.resolve(attributed_uuid, None)?.identity;
        let direct_source = if direct_uuid != attributed_uuid {
            Some(entities.resolve(direct_uuid, None)?.identity)
        } else {
            None
        };
        let amount = damage.value.or(damage.lucky_value).unwrap_or_default();
        let critical = damage
            .type_flags
            .map(|flags| flags & DAMAGE_FLAG_CRITICAL != 0)
            .or(damage.critical);
        let ability_id = damage.owner_id.map(i64::from);
        if let Some(state) = ability_id.and_then(|ability_id| {
            entities.observe_specialization_ability(attributed_uuid, ability_id)
        }) {
            drafts.push(specialization_actor_draft(metadata, state));
        }
        let ability = ability_id.map(AbilityId);

        if damage.damage_type == Some(crate::BpsrDamageType::Heal.protocol_id()) {
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Healing(HealingEvent {
                    source,
                    direct_source,
                    target: target.identity,
                    ability,
                    amount,
                    actual_amount: damage.actual_value,
                    hp_loss: damage.hp_loss,
                    shield_loss: damage.shield_loss,
                    hit_event_id: damage.hit_event_id,
                    damage_source: damage.damage_source,
                    damage_type: damage.damage_type,
                    effective_amount: None,
                    overheal: None,
                    critical,
                    periodic: None,
                    packet: rlogs_events::DamagePacketDetail {
                        attacker_uuid: damage.attacker_uuid,
                        top_summoner_uuid: damage.top_summoner_uuid,
                        owner_id: damage.owner_id,
                        dead: damage.dead,
                        missed: damage.missed,
                        reported_critical: damage.critical,
                        type_flags: damage.type_flags,
                        normal_value: damage.value,
                        lucky_value: damage.lucky_value,
                        owner_level: damage.owner_level,
                        owner_stage: damage.owner_stage,
                        normal_hit: damage.normal,
                        property: damage.property,
                        position: damage.damage_position.map(|position| {
                            rlogs_events::DamagePosition {
                                x: position.x,
                                y: position.y,
                                z: position.z,
                            }
                        }),
                        hit_parts: damage
                            .hit_parts
                            .into_iter()
                            .map(|part| rlogs_events::DamageHitPart {
                                part_id: part.part_id,
                                position: part.damage_position.map(|position| {
                                    rlogs_events::DamagePosition {
                                        x: position.x,
                                        y: position.y,
                                        z: position.z,
                                    }
                                }),
                                damage_value: part.damage_value,
                            })
                            .collect(),
                        damage_weight: damage.damage_weight.map(|weight| {
                            rlogs_events::DamagePosition {
                                x: weight.x,
                                y: weight.y,
                                z: None,
                            }
                        }),
                        passive_uuid: damage.passive_uuid,
                        rainbow: damage.rainbow,
                        damage_mode: damage.damage_mode,
                        skill_effect_uuid,
                        skill_effect_total_damage,
                        skill_effect_group_index,
                        skill_effect_component_index: u32::try_from(component_index).ok(),
                        skill_effect_component_count,
                    },
                }),
            ));
        } else {
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Damage(DamageEvent {
                    source,
                    direct_source,
                    target: target.identity,
                    ability,
                    amount,
                    actual_amount: damage.actual_value,
                    hp_loss: damage.hp_loss,
                    shield_loss: damage.shield_loss,
                    hit_event_id: damage.hit_event_id,
                    damage_source: damage.damage_source,
                    damage_type: damage.damage_type,
                    flags: DamageFlags {
                        critical,
                        lucky: match (damage.lucky_value, damage.value) {
                            (Some(value), _) => Some(value != 0),
                            (None, Some(_)) => Some(false),
                            (None, None) => None,
                        },
                        causes_lucky: damage
                            .type_flags
                            .map(|flags| flags & DAMAGE_FLAG_CAUSES_LUCKY != 0),
                        blocked: damage
                            .type_flags
                            .map(|flags| flags & DAMAGE_FLAG_BLOCKED != 0),
                        periodic: None,
                    },
                    packet: rlogs_events::DamagePacketDetail {
                        attacker_uuid: damage.attacker_uuid,
                        top_summoner_uuid: damage.top_summoner_uuid,
                        owner_id: damage.owner_id,
                        dead: damage.dead,
                        missed: damage.missed,
                        reported_critical: damage.critical,
                        type_flags: damage.type_flags,
                        normal_value: damage.value,
                        lucky_value: damage.lucky_value,
                        owner_level: damage.owner_level,
                        owner_stage: damage.owner_stage,
                        normal_hit: damage.normal,
                        property: damage.property,
                        position: damage.damage_position.map(|position| {
                            rlogs_events::DamagePosition {
                                x: position.x,
                                y: position.y,
                                z: position.z,
                            }
                        }),
                        hit_parts: damage
                            .hit_parts
                            .into_iter()
                            .map(|part| rlogs_events::DamageHitPart {
                                part_id: part.part_id,
                                position: part.damage_position.map(|position| {
                                    rlogs_events::DamagePosition {
                                        x: position.x,
                                        y: position.y,
                                        z: position.z,
                                    }
                                }),
                                damage_value: part.damage_value,
                            })
                            .collect(),
                        damage_weight: damage.damage_weight.map(|weight| {
                            rlogs_events::DamagePosition {
                                x: weight.x,
                                y: weight.y,
                                z: None,
                            }
                        }),
                        passive_uuid: damage.passive_uuid,
                        rainbow: damage.rainbow,
                        damage_mode: damage.damage_mode,
                        skill_effect_uuid,
                        skill_effect_total_damage,
                        skill_effect_group_index,
                        skill_effect_component_index: u32::try_from(component_index).ok(),
                        skill_effect_component_count,
                    },
                }),
            ));
            if damage.dead == Some(true) {
                drafts.push(timeline_draft(
                    metadata,
                    TimelineEventKind::Life {
                        actor: target.identity,
                        state: LifeState::Died,
                    },
                ));
            }
        }
    }
    Ok(())
}

fn decode_status_snapshot(
    snapshot: &schema::BuffInfoSync,
    fallback_target_uuid: i64,
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    for info in &snapshot.buff_infos {
        let target_uuid = snapshot
            .uuid
            .filter(|uuid| *uuid != 0)
            .or(info.host_uuid.filter(|uuid| *uuid != 0))
            .unwrap_or(fallback_target_uuid);
        let Some(buff_uuid) = info.buff_uuid else {
            emit_unresolved_status_event(
                target_uuid,
                None,
                Some(StatusState::Applied),
                None,
                None,
                info.fire_uuid.filter(|uuid| *uuid != 0),
                UnresolvedStatusReason::MissingInstanceId,
                info.encode_to_vec(),
                metadata,
                entities,
                drafts,
            )?;
            continue;
        };
        let Some(active) = active_status_from_info(info) else {
            emit_unresolved_status_event(
                target_uuid,
                Some(buff_uuid),
                Some(StatusState::Applied),
                None,
                None,
                info.fire_uuid.filter(|uuid| *uuid != 0),
                UnresolvedStatusReason::MissingEffectId,
                info.encode_to_vec(),
                metadata,
                entities,
                drafts,
            )?;
            continue;
        };
        let previous = status_effects.insert(target_uuid, buff_uuid, active);
        match previous {
            None => emit_status_event(
                target_uuid,
                buff_uuid,
                active,
                StatusState::Applied,
                metadata,
                entities,
                drafts,
            )?,
            Some(previous) if previous.base_id != active.base_id => {
                emit_status_event(
                    target_uuid,
                    buff_uuid,
                    previous,
                    StatusState::Removed,
                    metadata,
                    entities,
                    drafts,
                )?;
                emit_status_event(
                    target_uuid,
                    buff_uuid,
                    active,
                    StatusState::Applied,
                    metadata,
                    entities,
                    drafts,
                )?;
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn decode_status_effects(
    effects: &schema::BuffEffectSync,
    fallback_target_uuid: i64,
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    status_effects: &mut StatusEffectRegistry,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    let target_uuid = effects
        .uuid
        .filter(|uuid| *uuid != 0)
        .unwrap_or(fallback_target_uuid);

    for effect in &effects.buff_effects {
        let Some(buff_uuid) = effect.buff_uuid else {
            emit_unresolved_status_event(
                target_uuid,
                None,
                None,
                effect.event_type,
                None,
                None,
                UnresolvedStatusReason::MissingInstanceId,
                effect.encode_to_vec(),
                metadata,
                entities,
                drafts,
            )?;
            continue;
        };
        let event_type = effect.event_type.unwrap_or_default();
        let mut handled_change = false;
        let mut emitted_unresolved = false;

        for logic in &effect.logic_effects {
            let Some(raw) = logic.raw_data.as_deref() else {
                continue;
            };
            match logic.effect_type.unwrap_or_default() {
                BUFF_LOGIC_ADD_BUFF => {
                    let Ok(info) = schema::BuffInfo::decode(raw) else {
                        emit_unresolved_status_event(
                            target_uuid,
                            Some(buff_uuid),
                            Some(StatusState::Applied),
                            Some(event_type),
                            logic.effect_type,
                            None,
                            UnresolvedStatusReason::PayloadDecodeFailed,
                            raw.to_vec(),
                            metadata,
                            entities,
                            drafts,
                        )?;
                        emitted_unresolved = true;
                        continue;
                    };
                    let Some(active) = active_status_from_info(&info) else {
                        emit_unresolved_status_event(
                            target_uuid,
                            Some(buff_uuid),
                            Some(StatusState::Applied),
                            Some(event_type),
                            logic.effect_type,
                            info.fire_uuid.filter(|uuid| *uuid != 0),
                            UnresolvedStatusReason::MissingEffectId,
                            raw.to_vec(),
                            metadata,
                            entities,
                            drafts,
                        )?;
                        emitted_unresolved = true;
                        continue;
                    };
                    let previous = status_effects.insert(target_uuid, buff_uuid, active);
                    if let Some(previous) = previous
                        && previous.base_id != active.base_id
                    {
                        emit_status_event(
                            target_uuid,
                            buff_uuid,
                            previous,
                            StatusState::Removed,
                            metadata,
                            entities,
                            drafts,
                        )?;
                    }
                    let state = match previous {
                        None => StatusState::Applied,
                        Some(previous) if previous.base_id != active.base_id => {
                            StatusState::Applied
                        }
                        Some(previous) if active.stacks > previous.stacks => StatusState::Stacked,
                        Some(_) => StatusState::Refreshed,
                    };
                    emit_status_event(
                        target_uuid,
                        buff_uuid,
                        active,
                        state,
                        metadata,
                        entities,
                        drafts,
                    )?;
                    handled_change = true;
                }
                BUFF_LOGIC_CHANGE => {
                    let Ok(change) = schema::BuffChange::decode(raw) else {
                        emit_unresolved_status_event(
                            target_uuid,
                            Some(buff_uuid),
                            None,
                            Some(event_type),
                            logic.effect_type,
                            None,
                            UnresolvedStatusReason::PayloadDecodeFailed,
                            raw.to_vec(),
                            metadata,
                            entities,
                            drafts,
                        )?;
                        emitted_unresolved = true;
                        continue;
                    };
                    let Some(previous) = status_effects.get(target_uuid, buff_uuid) else {
                        emit_unresolved_status_event(
                            target_uuid,
                            Some(buff_uuid),
                            None,
                            Some(event_type),
                            logic.effect_type,
                            None,
                            UnresolvedStatusReason::MissingActiveEffectMapping,
                            raw.to_vec(),
                            metadata,
                            entities,
                            drafts,
                        )?;
                        emitted_unresolved = true;
                        continue;
                    };
                    let current = ActiveStatusEffect {
                        stacks: change
                            .layer
                            .and_then(|layer| u32::try_from(layer).ok())
                            .or(previous.stacks),
                        duration_millis: change
                            .duration
                            .and_then(|duration| u64::try_from(duration).ok())
                            .or(previous.duration_millis),
                        create_time_millis: change.create_time.or(previous.create_time_millis),
                        ..previous
                    };
                    status_effects.insert(target_uuid, buff_uuid, current);
                    // Raw event types 5 and 6 are not directional. Current-build
                    // packets use type 6 for increases, decreases, and unchanged
                    // layer updates. The decoded layer value is the authoritative
                    // resulting count whenever it is present.
                    let state = if current.stacks > previous.stacks {
                        StatusState::Stacked
                    } else if current.stacks < previous.stacks {
                        StatusState::Consumed
                    } else {
                        StatusState::Refreshed
                    };
                    emit_status_event(
                        target_uuid,
                        buff_uuid,
                        current,
                        state,
                        metadata,
                        entities,
                        drafts,
                    )?;
                    handled_change = true;
                }
                _ => {}
            }
        }

        if event_type == BUFF_EVENT_REMOVE {
            match status_effects.remove(target_uuid, buff_uuid) {
                Some(active) => emit_status_event(
                    target_uuid,
                    buff_uuid,
                    active,
                    StatusState::Removed,
                    metadata,
                    entities,
                    drafts,
                )?,
                None if !emitted_unresolved => emit_unresolved_status_event(
                    target_uuid,
                    Some(buff_uuid),
                    Some(StatusState::Removed),
                    Some(event_type),
                    None,
                    None,
                    UnresolvedStatusReason::MissingActiveEffectMapping,
                    Vec::new(),
                    metadata,
                    entities,
                    drafts,
                )?,
                None => {}
            }
        } else if !handled_change
            && matches!(
                event_type,
                BUFF_EVENT_LAYER_CHANGE_5 | BUFF_EVENT_LAYER_CHANGE_6
            )
        {
            // Without BuffChange.layer the packet does not reveal whether the
            // count increased, decreased, or remained unchanged. Preserve the
            // raw transition as a data gap instead of fabricating a direction.
            if !emitted_unresolved {
                emit_unresolved_status_event(
                    target_uuid,
                    Some(buff_uuid),
                    None,
                    Some(event_type),
                    None,
                    None,
                    UnresolvedStatusReason::AmbiguousTransition,
                    Vec::new(),
                    metadata,
                    entities,
                    drafts,
                )?;
            }
        }
    }
    Ok(())
}

fn active_status_from_info(info: &schema::BuffInfo) -> Option<ActiveStatusEffect> {
    Some(ActiveStatusEffect {
        base_id: info.base_id.filter(|base_id| *base_id > 0)?,
        source_uuid: info.fire_uuid.filter(|uuid| *uuid != 0),
        origin: info.fight_source_info.as_ref().and_then(|origin| {
            Some(StatusOrigin {
                source_type_id: origin.fight_source_type?,
                source_config_id: i64::from(origin.source_config_id.filter(|id| *id > 0)?),
            })
        }),
        stacks: info.layer.and_then(|layer| u32::try_from(layer).ok()),
        duration_millis: info
            .duration
            .and_then(|duration| u64::try_from(duration).ok()),
        create_time_millis: info.create_time,
        level: info.level,
        part_id: info.part_id,
        count: info.count,
    })
}

fn emit_status_event(
    target_uuid: i64,
    buff_uuid: i32,
    active: ActiveStatusEffect,
    state: StatusState,
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    let target = entities.resolve(target_uuid, None)?.identity;
    let source = active
        .source_uuid
        .map(|source_uuid| {
            entities
                .resolve(source_uuid, None)
                .map(|state| state.identity)
        })
        .transpose()?;
    drafts.push(timeline_draft(
        metadata,
        TimelineEventKind::Status(StatusEvent {
            source,
            target,
            effect: StatusEffectId(i64::from(active.base_id)),
            instance_id: Some(StatusEffectInstanceId(i64::from(buff_uuid))),
            origin: active.origin,
            state,
            stacks: active.stacks,
            duration_millis: active.duration_millis,
            level: active.level,
            part_id: active.part_id,
            count: active.count,
            created_at_millis: active.create_time_millis,
        }),
    ));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_unresolved_status_event(
    target_uuid: i64,
    buff_uuid: Option<i32>,
    state: Option<StatusState>,
    wire_event_type: Option<i32>,
    wire_logic_type: Option<i32>,
    source_uuid: Option<i64>,
    reason: UnresolvedStatusReason,
    raw_payload: Vec<u8>,
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    let target = entities.resolve(target_uuid, None)?.identity;
    let source = source_uuid
        .map(|uuid| entities.resolve(uuid, None).map(|state| state.identity))
        .transpose()?;
    drafts.push(timeline_draft(
        metadata,
        TimelineEventKind::UnresolvedStatus(UnresolvedStatusEvent {
            source,
            target,
            instance_id: buff_uuid.map(|value| StatusEffectInstanceId(i64::from(value))),
            state,
            wire_event_type,
            wire_logic_type,
            reason,
            raw_payload,
        }),
    ));
    Ok(())
}

fn emit_attributes(
    actor: EntityRef,
    attributes: &schema::AttrCollection,
    update_kind: EntityAttributeUpdateKind,
    metadata: &DecodeMetadata,
    drafts: &mut Vec<CanonicalEventDraft>,
) {
    let decoded = attributes
        .attributes
        .iter()
        .filter_map(|attribute| {
            let id = attribute.id?;
            let raw_value = attribute.raw_data.clone().unwrap_or_default();
            Some(EntityAttribute {
                attribute_id: id,
                decoded: decode_attribute_value(id, &raw_value),
                raw_value,
            })
        })
        .collect::<Vec<_>>();
    // The caller invokes snapshot mode only when the wire field was present.
    // Preserve an explicit empty collection as a state clear, while a missing
    // field remains no observation and never clears prior state.
    if !decoded.is_empty() || update_kind == EntityAttributeUpdateKind::Snapshot {
        let ownership = decode_actor_ownership(actor, &decoded);
        drafts.push(timeline_draft(
            metadata,
            TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                actor,
                update_kind,
                ownership,
                attributes: decoded,
            }),
        ));
    }
}

fn decode_actor_ownership(
    actor: EntityRef,
    attributes: &[EntityAttribute],
) -> Option<ActorOwnershipUpdate> {
    let owner_value = |attribute_id| {
        attributes
            .iter()
            .find(|attribute| attribute.attribute_id == attribute_id)
            .and_then(|attribute| match attribute.decoded {
                Some(EntityAttributeValue::Integer(value)) => Some(value),
                _ => None,
            })
    };
    let (Some(primary), Some(confirmation)) = (
        owner_value(ATTR_SUMMON_OWNER_PRIMARY),
        owner_value(ATTR_SUMMON_OWNER_CONFIRMATION),
    ) else {
        return None;
    };
    if primary == confirmation && primary > 0 && primary != actor.entity_uuid.0 {
        Some(ActorOwnershipUpdate::Confirmed {
            owner_entity_uuid: EntityUuid(primary),
        })
    } else {
        Some(ActorOwnershipUpdate::Cleared)
    }
}

fn emit_temporary_attributes(
    actor: EntityRef,
    raw_attributes: Option<&[u8]>,
    update_kind: EntityAttributeUpdateKind,
    metadata: &DecodeMetadata,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    let Some(raw_attributes) = raw_attributes else {
        return Ok(());
    };
    let collection = schema::TempAttrCollection::decode(raw_attributes)?;
    let attributes = collection
        .attributes
        .into_iter()
        .filter_map(|attribute| {
            Some(TemporaryAttribute {
                id: attribute.id?,
                value: attribute.value.unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    // An explicit empty AOI snapshot is state evidence: it clears every prior
    // temporary attribute for the actor. Preserve it in the canonical timeline
    // so replay can distinguish "proven empty" from "no packet observed".
    // Empty deltas remain no-ops because delta absence never means removal.
    if !attributes.is_empty() || update_kind == EntityAttributeUpdateKind::Snapshot {
        drafts.push(timeline_draft(
            metadata,
            TimelineEventKind::TemporaryAttributes(TemporaryAttributeEvent {
                actor,
                update_kind,
                attributes,
            }),
        ));
    }
    Ok(())
}

fn emit_position(
    actor: EntityRef,
    attributes: &schema::AttrCollection,
    metadata: &DecodeMetadata,
    drafts: &mut Vec<CanonicalEventDraft>,
) {
    let Some(position) = attr_position(attributes) else {
        return;
    };
    let (Some(x), Some(y), Some(z)) = (position.x, position.y, position.z) else {
        return;
    };
    drafts.push(timeline_draft(
        metadata,
        TimelineEventKind::Position(PositionEvent {
            actor,
            x,
            y,
            z,
            facing_radians: position.facing_radians,
        }),
    ));
}

fn attr_integer(attributes: &schema::AttrCollection, id: i32) -> Option<i64> {
    let raw = attributes
        .attributes
        .iter()
        .find(|attribute| attribute.id == Some(id))?
        .raw_data
        .as_deref()
        .unwrap_or_default();
    decode_varint(raw).and_then(|value| i64::try_from(value).ok())
}

fn attr_text(attributes: &schema::AttrCollection, id: i32) -> Option<String> {
    let raw = attributes
        .attributes
        .iter()
        .find(|attribute| attribute.id == Some(id))?
        .raw_data
        .as_deref()?;
    decode_length_prefixed_text(raw)
}

fn attr_raw(attributes: &schema::AttrCollection, id: i32) -> Option<&[u8]> {
    attributes
        .attributes
        .iter()
        .find(|attribute| attribute.id == Some(id))?
        .raw_data
        .as_deref()
}

fn attr_weapon_item_id(attributes: &schema::AttrCollection) -> Option<i64> {
    let equipment =
        schema::AttrEquipmentData::decode(attr_raw(attributes, ATTR_EQUIPMENT)?).ok()?;
    equipment
        .items
        .into_iter()
        .find(|item| item.slot_id == Some(WEAPON_SLOT_ID))?
        .item_id
        .filter(|item_id| *item_id > 0)
        .map(i64::from)
}

fn attr_player_skill_levels(attributes: &schema::AttrCollection) -> Vec<schema::AttrSkillLevel> {
    attr_raw(attributes, ATTR_ACTIVE_SKILL_LEVELS)
        .and_then(|raw| schema::AttrSkillLevelList::decode(raw).ok())
        .map_or_else(Vec::new, |skills| skills.skills)
}

fn attr_player_loadouts(
    attributes: &schema::AttrCollection,
    skills: &[schema::AttrSkillLevel],
) -> (
    Vec<ActorLoadoutSlot>,
    Vec<ActorLoadoutSlot>,
    ActorLoadoutObservation,
) {
    let tiers = skills
        .iter()
        .filter_map(|skill| {
            let skill_id = i64::from(skill.skill_id.filter(|skill_id| *skill_id > 0)?);
            let tier = observed_remodel_tier(skill.remodel_level);
            Some((skill_id, tier))
        })
        .collect::<BTreeMap<_, _>>();

    if let Some(slots) = attr_raw(attributes, ATTR_ACTION_SLOTS)
        .and_then(|raw| schema::AttrActionSlots::decode(raw).ok())
    {
        let mut ordered_slots = slots.slots.into_iter().collect::<Vec<_>>();
        ordered_slots.sort_unstable_by_key(|(map_slot, slot)| slot.slot_id.unwrap_or(*map_slot));

        let mut primary = Vec::with_capacity(2);
        let mut auxiliary = Vec::with_capacity(4);
        for (map_slot, slot) in ordered_slots {
            let slot_id = slot.slot_id.unwrap_or(map_slot);
            let Some(skill_id) = slot
                .skill_id
                .filter(|skill_id| *skill_id > 0)
                .map(i64::from)
            else {
                continue;
            };
            let tier = tiers.get(&skill_id).copied().flatten();
            if PRIMARY_LOADOUT_SLOTS.contains(&slot_id)
                && let Ok(Some(presentation)) = battle_imagine_presentation(skill_id)
            {
                primary.push(ActorLoadoutSlot {
                    slot_id,
                    ability_id: Some(skill_id),
                    item_id: Some(presentation.item_id),
                    tier,
                });
            } else if AUXILIARY_LOADOUT_SLOTS.contains(&slot_id)
                && let Ok(Some(presentation)) = auxiliary_action_presentation(skill_id)
            {
                let replacement =
                    presentation
                        .replacement_imagine_skill_id
                        .and_then(|replacement_skill_id| {
                            battle_imagine_presentation(replacement_skill_id)
                                .ok()
                                .flatten()
                                .map(|imagine| imagine.item_id)
                        });
                auxiliary.push(ActorLoadoutSlot {
                    slot_id,
                    ability_id: Some(skill_id),
                    item_id: replacement,
                    tier: replacement.and_then(|_| normalize_auxiliary_imagine_tier(tier)),
                });
            }
        }
        return (
            primary,
            auxiliary,
            ActorLoadoutObservation {
                primary: ActorLoadoutEvidence::ExactSlots,
                auxiliary: ActorLoadoutEvidence::ExactSlots,
            },
        );
    }

    let mut primary = Vec::with_capacity(2);
    for skill in skills.iter().take(64) {
        let Some(skill_id) = skill.skill_id.filter(|skill_id| *skill_id > 0) else {
            continue;
        };
        let skill_id = i64::from(skill_id);
        let tier = observed_remodel_tier(skill.remodel_level);

        if primary.len() < 2
            && !primary
                .iter()
                .any(|slot: &ActorLoadoutSlot| slot.ability_id == Some(skill_id))
            && let Ok(Some(presentation)) = battle_imagine_presentation(skill_id)
        {
            primary.push(ActorLoadoutSlot {
                slot_id: 7 + i32::try_from(primary.len()).unwrap_or_default(),
                ability_id: Some(skill_id),
                item_id: Some(presentation.item_id),
                tier,
            });
            continue;
        }
    }

    primary.sort_unstable_by_key(|slot| slot.ability_id);
    for (index, slot) in primary.iter_mut().enumerate() {
        slot.slot_id = 7 + i32::try_from(index).unwrap_or_default();
    }

    // Nearby-player attribute 116 proves the two primary Imagine identities
    // and tiers, but it does not carry the four auxiliary slot assignments.
    // Only attribute 226 contains those exact assignments, and the current
    // client sends it for the local owner rather than ordinary teammates.
    // Keep remote auxiliary positions unresolved instead of fabricating an
    // ordering from the shuffled active-skill list.
    let primary_evidence = if attr_raw(attributes, ATTR_ACTIVE_SKILL_LEVELS).is_some() {
        ActorLoadoutEvidence::ObservedSet
    } else {
        ActorLoadoutEvidence::Unobserved
    };
    (
        primary,
        Vec::new(),
        ActorLoadoutObservation {
            primary: primary_evidence,
            auxiliary: ActorLoadoutEvidence::Unobserved,
        },
    )
}

fn observed_remodel_tier(remodel_level: Option<i32>) -> Option<u32> {
    match remodel_level {
        Some(remodel_level) if remodel_level < 0 => None,
        Some(remodel_level) => u32::try_from(remodel_level).ok(),
        // Prost preserves field presence. The game omits a zero-valued
        // remodel level, which is the confirmed no-tier/T0 state rather than
        // missing evidence once the Imagine skill identity itself is present.
        None => Some(0),
    }
}

fn attr_position(attributes: &schema::AttrCollection) -> Option<schema::Position> {
    let raw = attributes
        .attributes
        .iter()
        .find(|attribute| attribute.id == Some(ATTR_POSITION))?
        .raw_data
        .as_deref()?;
    schema::Position::decode(raw).ok()
}

/// Decodes a retained entity-attribute payload with the same exact, ID-gated
/// table used by the live packet decoder.
///
/// This is intentionally not a generic varint fallback. Unknown attribute IDs
/// remain unresolved so offline replay, validation, and live projection cannot
/// silently reinterpret unproven packet evidence.
pub fn decode_known_entity_attribute_value(id: i32, raw: &[u8]) -> Option<EntityAttributeValue> {
    decode_attribute_value(id, raw)
}

fn decode_attribute_value(id: i32, raw: &[u8]) -> Option<EntityAttributeValue> {
    match id {
        ATTR_NAME => decode_length_prefixed_text(raw).map(EntityAttributeValue::Text),
        ATTR_POSITION | ATTR_TARGET_POSITION => {
            let position = schema::Position::decode(raw).ok()?;
            Some(EntityAttributeValue::Position {
                x: position.x?,
                y: position.y?,
                z: position.z?,
                facing_radians: position.facing_radians,
            })
        }
        // FightAttrTable declares these reviewed formula families as int32.
        // Negative protobuf int32 values are sign-extended to ten-byte
        // varints, so decoding them through u64 -> i64 rejects exact debuff
        // observations and can leave an older positive value in replay state.
        11010..=11014
        | 11020..=11024
        | 11030..=11034
        | ATTR_PHYSICAL_ATTACK
        | 11331..=11334
        | ATTR_MAGICAL_ATTACK
        | 11341..=11344 => decode_int32_varint(raw).map(EntityAttributeValue::Integer),
        ATTR_MONSTER_ID
        | ATTR_CLASS_ID
        | ATTR_LEVEL
        | ATTR_COMBAT_POWER
        | ATTR_SEASON_STRENGTH
        | ATTR_SCENE_ID
        | ATTR_SCENE_LINE
        | ATTR_BREAKING_STAGE
        | ATTR_SUMMON_OWNER_PRIMARY
        | ATTR_SUMMON_OWNER_CONFIRMATION
        | ATTR_CURRENT_HP
        | ATTR_MAX_HP_FINAL
        | ATTR_MAX_HP_TOTAL
        | ATTR_MAX_HP_ADD
        | ATTR_MAX_HP_EXTRA_ADD
        | ATTR_MAX_HP_PERCENT
        | ATTR_MAX_HP_EXTRA_PERCENT
        // Current-build Team Luck status/attribute proof closes these exact
        // positive scalar routes: 12510 is Crit Damage and 12530 is Lucky
        // Damage Increase. Keep the allowlist ID-specific; neighboring
        // attributes remain unresolved until independently reviewed.
        // Current-build Inspiration proof closes the exact retained scalar
        // routes consumed by its versioned projector. These include final and
        // raw-add Crit/Luck, Haste, Mastery, Versatility, external damage, and
        // the serialized Light-property damage lane. This is deliberately an
        // explicit set rather than a neighboring-ID range.
        | 11710
        | 11712
        // Current-build Inspire (31602) proof closes the exact final action-
        // speed lanes consumed by its packet-final throughput projector:
        // 11720 is normal action speed and 11730 is guide/cast speed.
        | 11720
        | 11730
        | 11780
        | 11782
        | 11840
        | 11930
        | 11942
        | 11950
        | 11952
        | 12510
        | 12530
        | 13110
        | 13120
        | 13130
        | 13140
        | 13150
        | 13160
        | 13170
        | 13180
        // Packet-observed all-element final/total/add/extra-add/percent/
        // extra-percent family. Fatal Spiral changes the first three together
        // (for example 237 -> 1237) while the remaining components stay
        // unchanged. These exact IDs are consumed by the versioned rDPS
        // runtime; keeping them in the strict allowlist lets canonical replay
        // retain the proven transition without enabling generic decoding.
        | 13100..=13105
        | ATTR_MASTERY
        | ATTR_CURRENT_ENERGY
        | ATTR_MAX_ENERGY_FINAL
        | ATTR_MAX_ENERGY_TOTAL
        | ATTR_MAX_ENERGY_ADD
        | ATTR_MAX_ENERGY_EXTRA_ADD
        | ATTR_MAX_ENERGY_PERCENT
        | ATTR_MAX_ENERGY_EXTRA_PERCENT => decode_varint(raw)
            .and_then(|value| i64::try_from(value).ok())
            .map(EntityAttributeValue::Integer),
        _ => None,
    }
}

fn decode_int32_varint(bytes: &[u8]) -> Option<i64> {
    let raw = decode_varint(bytes)?;
    let signed = raw as i64;
    let value = i32::try_from(signed).ok()?;
    (value as i64 as u64 == raw).then_some(i64::from(value))
}

fn decode_varint(bytes: &[u8]) -> Option<u64> {
    // Attr values are protobuf scalar payloads stored inside a bytes field.
    // Prost omits the scalar's zero byte, so an attribute row with an empty
    // payload is the exact value zero rather than missing or malformed data.
    if bytes.is_empty() {
        return Some(0);
    }
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn decode_length_prefixed_text(bytes: &[u8]) -> Option<String> {
    let length = decode_varint(bytes)?;
    let prefix_length = bytes.iter().position(|byte| byte & 0x80 == 0)? + 1;
    let length = usize::try_from(length).ok()?;
    let value = bytes.get(prefix_length..prefix_length.checked_add(length)?)?;
    String::from_utf8(value.to_vec()).ok()
}

fn actor_kind(entity_type_id: i32) -> ActorKind {
    match entity_type_id {
        1 => ActorKind::Monster,
        2 => ActorKind::Npc,
        3 => ActorKind::SceneObject,
        5 => ActorKind::Zone,
        6 | 7 => ActorKind::Projectile,
        8 => ActorKind::Pet,
        10 => ActorKind::Player,
        11 => ActorKind::TrainingDummy,
        12 => ActorKind::Drop,
        14 => ActorKind::Field,
        15 => ActorKind::Trap,
        16 => ActorKind::Collection,
        18 => ActorKind::StaticObject,
        19 => ActorKind::Vehicle,
        20 => ActorKind::Toy,
        21 | 22 => ActorKind::Housing,
        unknown => ActorKind::Unknown(unknown),
    }
}

fn draft(
    metadata: &DecodeMetadata,
    sensitivity: EventSensitivity,
    kind: CanonicalEventDraftKind,
) -> CanonicalEventDraft {
    CanonicalEventDraft {
        time: metadata.time,
        provenance: metadata.provenance.clone(),
        sensitivity,
        kind,
    }
}

fn timeline_draft(metadata: &DecodeMetadata, kind: TimelineEventKind) -> CanonicalEventDraft {
    draft(
        metadata,
        EventSensitivity::PublicGameplay,
        CanonicalEventDraftKind::Timeline(kind),
    )
}

fn specialization_actor_draft(
    metadata: &DecodeMetadata,
    state: EntityState,
) -> CanonicalEventDraft {
    timeline_draft(
        metadata,
        TimelineEventKind::Actor(ActorEvent {
            actor: state.identity,
            state: ActorState::Updated,
            entity_type_id: state.entity_type_id,
            kind: actor_kind(state.entity_type_id),
            monster_id: None,
            character_id: (state.entity_type_id == ENTITY_PLAYER)
                .then(|| character_id_from_entity_uuid(state.identity.entity_uuid.0))
                .flatten(),
            display_name: None,
            class_id: state.class_id,
            specialization_id: state.specialization_id,
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: ActorLoadoutObservation::default(),
        }),
    )
}

fn decode_gap_draft(
    record: &CaptureRecord,
    connection_id: u64,
    stream_id: u64,
    detail: &str,
) -> CanonicalEventDraft {
    CanonicalEventDraft {
        time: EventTime {
            observed_micros: record.observed_micros,
            game_time_millis: None,
        },
        provenance: EventProvenance::wire(record.sequence, connection_id, stream_id),
        sensitivity: EventSensitivity::PublicGameplay,
        kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::DataGap(DataGapEvent {
            kind: DataGapKind::DecodeFailure,
            connection_id: Some(connection_id),
            stream_id: Some(stream_id),
            detail: detail.to_owned(),
        })),
    }
}

#[derive(Debug, Error)]
enum ProtocolMessageError {
    #[error("protobuf decode failed")]
    Protobuf(#[from] prost::DecodeError),

    #[error("could not serialize the privacy-reviewed profile payload")]
    ProfileSerialization(#[from] serde_json::Error),

    #[error("dirty-container decode failed")]
    DirtyBlob(#[from] dirty_blob_v1::DirtyBlobError),

    #[error("current-build client skill action decode failed")]
    UseSkillAction(#[from] crate::UseSkillActionDecodeError),

    #[error("entity registry limit {0} exceeded")]
    EntityLimitExceeded(usize),

    #[error("actor sequence space is exhausted")]
    ActorSequenceExhausted,
}

#[derive(Debug, Error)]
pub enum ProtocolRuntimeError {
    #[error("selected protocol pack does not match the capture build")]
    PackBuildMismatch,

    #[error("resolved region does not match the capture build")]
    RegionBuildMismatch,

    #[error("protocol runtime limits must be non-zero")]
    InvalidConfig,

    #[error("decoder emitted {count} events, exceeding the per-packet limit {limit}")]
    EventLimitExceeded { count: usize, limit: usize },

    #[error(transparent)]
    EventSequence(#[from] rlogs_events::EventSequenceError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{
        CompressionState, FragmentKind, MappingConfidence, PROTOCOL_PACK_SCHEMA_VERSION,
        PacketDirection, PacketEnvelope, PacketPayload, ProtocolPackDefinition, ProtocolPackRoute,
        ProtocolPackRouteDisposition, ProtocolPackTarget, RouteKey, RoutedMessage,
    };

    const WORLD_SERVICE: u64 = 0x6333_5342;
    const WORLD_LOGIN_SERVICE: u64 = 78_136_601;
    const SOCIAL_SERVICE: u64 = 625_772_963;
    const TEAM_SERVICE: u64 = 966_773_353;

    #[test]
    fn exact_target_position_attribute_uses_the_position_wire_shape() {
        let raw = schema::Position {
            x: Some(1.25),
            y: Some(-2.5),
            z: Some(3.75),
            facing_radians: Some(0.5),
        }
        .encode_to_vec();

        assert_eq!(
            decode_known_entity_attribute_value(ATTR_TARGET_POSITION, &raw),
            Some(EntityAttributeValue::Position {
                x: 1.25,
                y: -2.5,
                z: 3.75,
                facing_radians: Some(0.5),
            })
        );
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullNotifyEnterWorld {
        #[prost(message, optional, tag = "1")]
        request: Option<FullNotifyEnterWorldRequest>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullNotifyEnterWorldRequest {
        #[prost(string, optional, tag = "1")]
        account_id: Option<String>,
        #[prost(string, optional, tag = "2")]
        token: Option<String>,
        #[prost(string, optional, tag = "3")]
        scene_host: Option<String>,
        #[prost(int32, optional, tag = "4")]
        scene_port: Option<i32>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullProfileEnvelope {
        #[prost(message, optional, tag = "1")]
        character: Option<FullCharacterSerialize>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullCharacterSerialize {
        #[prost(int64, optional, tag = "1")]
        character_id: Option<i64>,
        #[prost(message, optional, tag = "2")]
        base: Option<FullCharacterBase>,
        #[prost(message, optional, tag = "22")]
        role_level: Option<schema::RoleLevel>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullCharacterBase {
        #[prost(int64, optional, tag = "1")]
        character_id: Option<i64>,
        #[prost(string, optional, tag = "2")]
        account_id: Option<String>,
        #[prost(int64, optional, tag = "3")]
        display_id: Option<i64>,
        #[prost(uint32, optional, tag = "4")]
        server_id: Option<u32>,
        #[prost(string, optional, tag = "5")]
        display_name: Option<String>,
        #[prost(string, optional, tag = "27")]
        open_id: Option<String>,
        #[prost(int32, optional, tag = "31")]
        initial_class_id: Option<i32>,
        #[prost(int32, optional, tag = "35")]
        combat_power: Option<i32>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullNotifySocialData {
        #[prost(message, optional, tag = "1")]
        request: Option<FullNotifySocialDataRequest>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullNotifySocialDataRequest {
        #[prost(message, optional, tag = "1")]
        data: Option<FullSocialData>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullSocialData {
        #[prost(int64, optional, tag = "1")]
        character_id: Option<i64>,
        #[prost(string, optional, tag = "2")]
        account_id: Option<String>,
        #[prost(message, optional, tag = "3")]
        basic: Option<schema::SocialBasicData>,
        #[prost(message, optional, tag = "4")]
        avatar: Option<schema::SocialAvatarInfo>,
        #[prost(message, optional, tag = "6")]
        profession: Option<schema::SocialProfessionData>,
        #[prost(message, optional, tag = "7")]
        equipment: Option<schema::SocialEquipmentData>,
        #[prost(message, optional, tag = "10")]
        scene: Option<schema::SceneData>,
        #[prost(message, optional, tag = "11")]
        user_attributes: Option<schema::SocialUserAttributes>,
        #[prost(message, optional, tag = "13")]
        guild: Option<schema::SocialGuildData>,
        #[prost(string, optional, tag = "14")]
        account_data: Option<String>,
        #[prost(message, optional, tag = "16")]
        personal_zone: Option<schema::SocialPersonalZone>,
        #[prost(message, optional, tag = "22")]
        master_mode_dungeon: Option<schema::MasterModeDungeonData>,
    }

    #[derive(Debug)]
    struct FixtureObjectiveCatalog;

    impl ObjectiveCatalogResolver for FixtureObjectiveCatalog {
        fn resolve(
            &self,
            objective_id: i64,
        ) -> Result<Option<DungeonObjectiveCatalogReference>, crate::ObjectiveCatalogError>
        {
            let known_objectives = BTreeSet::from([9_001]);
            Ok(known_objectives
                .contains(&objective_id)
                .then(|| DungeonObjectiveCatalogReference {
                    resolution: DungeonObjectiveCatalogResolution::ResolvedCurrentBuild,
                    activity_target_key: Some(format!("activity-target.{objective_id}")),
                    scene_event_keys: vec!["scene-event.77".into()],
                }))
        }
    }

    fn encode<M: Message>(message: M) -> Vec<u8> {
        message.encode_to_vec()
    }

    fn safe_dirty_scalar(value: i32) -> Vec<u8> {
        let mut bytes = value.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    fn safe_dirty_object(fields: Vec<(i32, Vec<u8>)>) -> Vec<u8> {
        let mut body = Vec::new();
        for (field, value) in fields {
            body.extend(safe_dirty_scalar(field));
            body.extend(value);
        }
        let mut bytes = safe_dirty_scalar(-2);
        bytes.extend(safe_dirty_scalar(i32::try_from(body.len()).unwrap()));
        bytes.extend(body);
        bytes.extend(safe_dirty_scalar(-3));
        bytes
    }

    fn int_attr(id: i32, value: u64) -> schema::Attr {
        let mut bytes = Vec::new();
        let mut remaining = value;
        loop {
            let mut byte = (remaining & 0x7f) as u8;
            remaining >>= 7;
            if remaining != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if remaining == 0 {
                break;
            }
        }
        schema::Attr {
            id: Some(id),
            raw_data: Some(bytes),
        }
    }

    fn text_attr(id: i32, value: &str) -> schema::Attr {
        let mut raw = vec![value.len() as u8];
        raw.extend_from_slice(value.as_bytes());
        schema::Attr {
            id: Some(id),
            raw_data: Some(raw),
        }
    }

    fn route(method_id: u32, decoder: DecoderKind) -> ProtocolPackRoute {
        route_for(WORLD_SERVICE, method_id, decoder)
    }

    fn route_for(service_id: u64, method_id: u32, decoder: DecoderKind) -> ProtocolPackRoute {
        ProtocolPackRoute {
            route: RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                service_id,
                method_id,
            ),
            service_name: match service_id {
                WORLD_LOGIN_SERVICE => "WorldLoginNtf",
                SOCIAL_SERVICE => "SocialNtf",
                TEAM_SERVICE => "GrpcTeamNtf",
                _ => "WorldNtf",
            }
            .into(),
            method_name: format!("method-{method_id}"),
            message_name: None,
            confidence: MappingConfidence::Verified,
            provenance: Vec::new(),
            features: Vec::new(),
            disposition: ProtocolPackRouteDisposition::Allowed {
                domain: decoder.domain(),
                decoder,
            },
        }
    }

    fn pack() -> ProtocolPack {
        ProtocolPack::build(ProtocolPackDefinition {
            schema_version: PROTOCOL_PACK_SCHEMA_VERSION,
            pack_id: "test-global".into(),
            target: ProtocolPackTarget {
                deployment_id: "global".into(),
                region_id: None,
                channel: "steam".into(),
                build_id: "build-1".into(),
                executable_version: None,
            },
            acquisition: Default::default(),
            provenance: Vec::new(),
            routes: vec![
                route_for(WORLD_LOGIN_SERVICE, 3, DecoderKind::NotifyEnterWorldV1),
                route_for(SOCIAL_SERVICE, 1, DecoderKind::NotifySocialDataV1),
                route_for(TEAM_SERVICE, 2, DecoderKind::NotifyTeamMemberInfoV1),
                route_for(TEAM_SERVICE, 3, DecoderKind::NotifyJoinTeamV1),
                route_for(TEAM_SERVICE, 4, DecoderKind::NotifyLeaveTeamV1),
                route_for(TEAM_SERVICE, 13, DecoderKind::NoticeTeamDissolveV1),
                route(3, DecoderKind::EnterSceneV1),
                route(4, DecoderKind::NotifyLoadSceneEndV1),
                route(6, DecoderKind::SyncNearEntitiesV1),
                route(23, DecoderKind::SyncDungeonDataV1),
                route(24, DecoderKind::SyncDungeonDirtyDataV1),
                route(27, DecoderKind::SyncSeasonV1),
                route(43, DecoderKind::SyncServerTimeV1),
                route(0x15, DecoderKind::SyncContainerDataV1),
                route(0x16, DecoderKind::SyncContainerDirtyDataV1),
                route(0x2d, DecoderKind::SyncNearDeltaV1),
                route(0x2e, DecoderKind::SyncToMeDeltaV1),
                route(0x27, DecoderKind::NotifyReviveV1),
                route(0x43, DecoderKind::SyncClientUseSkillV1),
            ],
        })
        .unwrap()
    }

    fn build() -> GameBuild {
        GameBuild {
            deployment_id: "global".into(),
            region_id: Some("north-america".into()),
            channel: "steam".into(),
            build_id: "build-1".into(),
            executable_version: None,
        }
    }

    fn runtime(pack: &ProtocolPack) -> ProtocolRuntime<'_> {
        ProtocolRuntime::new(
            pack,
            "capture-1",
            &build(),
            RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: None,
            },
            vec![RegionEvidence {
                kind: RegionEvidenceKind::ConnectionEndpoint,
                reference: "na-endpoint-group".into(),
            }],
            ProtocolRuntimeConfig::default(),
        )
        .unwrap()
    }

    fn record(sequence: u64, method_id: u32, payload: Vec<u8>) -> CaptureRecord {
        record_for(WORLD_SERVICE, sequence, method_id, payload)
    }

    fn record_for(
        service_id: u64,
        sequence: u64,
        method_id: u32,
        payload: Vec<u8>,
    ) -> CaptureRecord {
        record_for_route(
            sequence,
            RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                service_id,
                method_id,
            ),
            payload,
        )
    }

    fn record_for_route(sequence: u64, route: RouteKey, payload: Vec<u8>) -> CaptureRecord {
        let direction = route.direction;
        let fragment = route.fragment;
        CaptureRecord {
            sequence,
            observed_micros: sequence * 100,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 7,
                stream_id: 8,
                source: None,
                destination: None,
                direction,
                fragment: Some(fragment),
                route: Some(RoutedMessage {
                    key: route,
                    stub_id: 0,
                    call_id: None,
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: payload.clone(),
                    application_bytes: Some(payload),
                },
            }),
        }
    }

    fn current_build_use_slot_pack(client_service_id: u64) -> ProtocolPack {
        let use_slot_route = RouteKey::new(
            PacketDirection::ClientToServer,
            FragmentKind::Call,
            client_service_id,
            0x3d002,
        );
        ProtocolPack::build(ProtocolPackDefinition {
            schema_version: PROTOCOL_PACK_SCHEMA_VERSION,
            pack_id: "test-global-current-use-slot".into(),
            target: ProtocolPackTarget {
                deployment_id: "global".into(),
                region_id: None,
                channel: "steam".into(),
                build_id: crate::BPSR_USE_SKILL_ATTR_BUILD.into(),
                executable_version: None,
            },
            acquisition: Default::default(),
            provenance: Vec::new(),
            routes: vec![
                route(0x2e, DecoderKind::SyncToMeDeltaV1),
                ProtocolPackRoute {
                    route: use_slot_route,
                    service_name: "WorldProxy".into(),
                    method_name: "UseSlot".into(),
                    message_name: None,
                    confidence: MappingConfidence::Verified,
                    provenance: Vec::new(),
                    features: Vec::new(),
                    disposition: ProtocolPackRouteDisposition::Allowed {
                        domain: DecoderKind::WorldUseSlotV1.domain(),
                        decoder: DecoderKind::WorldUseSlotV1,
                    },
                },
            ],
        })
        .unwrap()
    }

    fn current_build_runtime(pack: &ProtocolPack) -> ProtocolRuntime<'_> {
        let mut build = build();
        build.build_id = crate::BPSR_USE_SKILL_ATTR_BUILD.into();
        ProtocolRuntime::new(
            pack,
            "capture-current-use-slot",
            &build,
            RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: None,
            },
            vec![RegionEvidence {
                kind: RegionEvidenceKind::ConnectionEndpoint,
                reference: "na-endpoint-group".into(),
            }],
            ProtocolRuntimeConfig::default(),
        )
        .unwrap()
    }

    fn blob_i32(value: i32) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn blob_u32(value: u32) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn blob_i64(value: i64) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn blob_u64(value: u64) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn blob_string(value: &str) -> Vec<u8> {
        let mut bytes = blob_u32(u32::try_from(value.len()).unwrap());
        bytes.extend_from_slice(value.as_bytes());
        bytes
    }

    fn blob_object(fields: Vec<(i32, Vec<u8>)>) -> Vec<u8> {
        let mut body = Vec::new();
        for (field, value) in fields {
            body.extend_from_slice(&field.to_le_bytes());
            body.extend(value);
        }
        let mut bytes = blob_i32(-2);
        bytes.extend(blob_i32(i32::try_from(body.len()).unwrap()));
        bytes.extend(body);
        bytes.extend(blob_i32(-3));
        bytes
    }

    #[test]
    fn unchanged_actor_and_cooldown_echoes_are_bounded_and_gap_aware() {
        let metadata = DecodeMetadata {
            time: EventTime {
                observed_micros: 10,
                game_time_millis: Some(20),
            },
            provenance: EventProvenance::wire(1, 2, 3),
            region: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: None,
            },
        };
        let actor = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid(70),
        };
        let actor_event = ActorEvent {
            actor,
            state: ActorState::Updated,
            entity_type_id: ENTITY_PLAYER,
            kind: ActorKind::Player,
            monster_id: None,
            character_id: Some("70".into()),
            display_name: Some("Recorder".into()),
            class_id: Some(5),
            specialization_id: Some(110),
            level: Some(60),
            ability_score: Some(65_000),
            weapon_item_id: Some(123),
            weapon_breakthrough_count: Some(4),
            seasonal_score: Some(4_825),
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: ActorLoadoutObservation::default(),
        };
        let cooldown_event = CooldownEvent {
            actor,
            ability: AbilityId(2_903_521),
            begin_time_millis: Some(100),
            duration_millis: Some(5_000),
            valid_duration_millis: Some(4_500),
            cooldown_type: Some(1),
            profession_hold_begin_time_millis: None,
            charge_count: Some(1),
            valid_cooldown_time_millis: Some(4_500),
            sub_cooldown_ratio_raw: None,
            sub_cooldown_fixed_raw: None,
            accelerate_cooldown_ratio_raw: None,
        };
        let actor_draft = timeline_draft(&metadata, TimelineEventKind::Actor(actor_event.clone()));
        let cooldown_draft = timeline_draft(
            &metadata,
            TimelineEventKind::Cooldown(cooldown_event.clone()),
        );
        let mut deduplicator = CanonicalStateDeduplicator::new(1);

        assert!(deduplicator.retain(&actor_draft));
        assert!(!deduplicator.retain(&actor_draft));
        assert!(deduplicator.retain(&cooldown_draft));
        assert!(!deduplicator.retain(&cooldown_draft));

        let status_draft = timeline_draft(
            &metadata,
            TimelineEventKind::Status(StatusEvent {
                source: Some(actor),
                target: actor,
                effect: StatusEffectId(3_003_052),
                instance_id: Some(StatusEffectInstanceId(42)),
                origin: None,
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: Some(10_000),
                level: Some(1),
                part_id: None,
                count: None,
                created_at_millis: Some(20),
            }),
        );
        assert!(deduplicator.retain(&status_draft));
        assert!(deduplicator.retain(&status_draft));

        let mut second_actor = actor_event.clone();
        second_actor.actor = EntityRef {
            actor_id: ActorId(8),
            entity_uuid: EntityUuid(80),
        };
        assert!(deduplicator.retain(&timeline_draft(
            &metadata,
            TimelineEventKind::Actor(second_actor),
        )));
        // Capacity pressure clears the optimization cache, so the evicted
        // assertion is emitted again instead of risking lost evidence.
        assert!(deduplicator.retain(&actor_draft));
        assert!(!deduplicator.retain(&actor_draft));

        let mut changed_cooldown = cooldown_event.clone();
        changed_cooldown.begin_time_millis = Some(200);
        assert!(deduplicator.retain(&timeline_draft(
            &metadata,
            TimelineEventKind::Cooldown(changed_cooldown),
        )));

        let gap = timeline_draft(
            &metadata,
            TimelineEventKind::DataGap(DataGapEvent {
                kind: DataGapKind::TcpGap,
                connection_id: Some(2),
                stream_id: Some(3),
                detail: "test gap".into(),
            }),
        );
        assert!(deduplicator.retain(&gap));
        assert!(deduplicator.retain(&actor_draft));
        assert!(deduplicator.retain(&cooldown_draft));

        let mut despawned = actor_event;
        despawned.state = ActorState::Despawned;
        assert!(deduplicator.retain(&timeline_draft(
            &metadata,
            TimelineEventKind::Actor(despawned),
        )));
        assert!(deduplicator.retain(&actor_draft));
        assert!(deduplicator.retain(&cooldown_draft));
    }

    #[test]
    fn world_entry_exposes_only_the_server_endpoint_for_local_realm_resolution() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let payload = encode(FullNotifyEnterWorld {
            request: Some(FullNotifyEnterWorldRequest {
                account_id: Some("private-account-value".into()),
                token: Some("private-login-token".into()),
                scene_host: Some("gamesvr.playbpsr.com".into()),
                scene_port: Some(10_099),
            }),
        });

        let batch = runtime
            .process(&record_for(WORLD_LOGIN_SERVICE, 1, 3, payload))
            .unwrap();

        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        assert!(batch.events.is_empty());
        assert_eq!(
            batch.announced_server,
            Some(AnnouncedServerEndpoint {
                host: "gamesvr.playbpsr.com".into(),
                port: Some(10_099),
            })
        );
        let json = serde_json::to_string(&batch.announced_server).unwrap();
        assert!(!json.contains("account"));
        assert!(!json.contains("token"));
        assert!(!json.contains("private-account-value"));
        assert!(!json.contains("private-login-token"));
    }

    #[test]
    fn authoritative_server_time_stamps_subsequent_events() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let clock = runtime
            .process(&record(
                1,
                43,
                encode(schema::SyncServerTime {
                    client_milliseconds: None,
                    server_milliseconds: Some(1_785_287_823_171),
                }),
            ))
            .unwrap();
        assert_eq!(
            clock.server_clock,
            Some(ServerClockObservation {
                client_milliseconds: None,
                server_milliseconds: 1_785_287_823_171,
            })
        );
        assert!(clock.events.is_empty());

        let scene = runtime
            .process(&record(
                11,
                3,
                encode(schema::EnterScene {
                    enter_scene_info: Some(schema::EnterSceneInfo {
                        scene_attrs: None,
                        player_entity: None,
                        scene_instance_id: Some("clocked-scene".into()),
                    }),
                }),
            ))
            .unwrap();
        assert!(
            scene
                .events
                .iter()
                .all(|event| event.time.game_time_millis == Some(1_785_287_823_172))
        );
    }

    #[test]
    fn empty_dungeon_snapshot_does_not_invent_a_run_boundary() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let batch = runtime
            .process(&record(
                1,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(1),
                        flow_info: Some(schema::DungeonFlowInfo::default()),
                        ..schema::DungeonSyncData::default()
                    }),
                }),
            ))
            .unwrap();

        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        assert!(batch.events.is_empty());
    }

    #[test]
    fn objective_map_key_never_stands_in_for_a_missing_target_id() {
        let pack = pack();
        let mut runtime = runtime(&pack).with_objective_catalog(Arc::new(FixtureObjectiveCatalog));
        let batch = runtime
            .process(&record(
                1,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        target: Some(schema::DungeonTarget {
                            target_data: [(
                                9_001,
                                schema::DungeonTargetData {
                                    target_id: None,
                                    value: Some(2),
                                    complete: Some(0),
                                },
                            )]
                            .into_iter()
                            .collect(),
                        }),
                        ..schema::DungeonSyncData::default()
                    }),
                }),
            ))
            .unwrap();

        assert!(batch.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::ObjectiveUpdated,
                objective_map_key: Some(9_001),
                objective_id: None,
                objective_catalog: None,
                ..
            })
        )));
    }

    #[test]
    fn dungeon_snapshot_emits_exact_started_ended_and_objective_timeline() {
        let pack = pack();
        let mut runtime = runtime(&pack).with_objective_catalog(Arc::new(FixtureObjectiveCatalog));

        let ready = runtime
            .process(&record(
                1,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(555),
                        flow_info: Some(schema::DungeonFlowInfo {
                            state: Some(DUNGEON_STATE_READY),
                            ..schema::DungeonFlowInfo::default()
                        }),
                        scene_info: Some(schema::DungeonSceneInfo {
                            difficulty: Some(7),
                        }),
                        ..schema::DungeonSyncData::default()
                    }),
                }),
            ))
            .unwrap();
        assert!(ready.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Entered,
                instance_id: Some(instance_id),
                difficulty_id: Some(7),
                ..
            }) if instance_id == "555"
        )));

        let playing = runtime
            .process(&record(
                2,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(555),
                        flow_info: Some(schema::DungeonFlowInfo {
                            state: Some(DUNGEON_STATE_PLAYING),
                            ..schema::DungeonFlowInfo::default()
                        }),
                        target: Some(schema::DungeonTarget {
                            target_data: [(
                                41,
                                schema::DungeonTargetData {
                                    target_id: Some(9_001),
                                    value: Some(2),
                                    complete: Some(0),
                                },
                            )]
                            .into_iter()
                            .collect(),
                        }),
                        scene_info: Some(schema::DungeonSceneInfo {
                            difficulty: Some(7),
                        }),
                    }),
                }),
            ))
            .unwrap();
        assert!(playing.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Started,
                ..
            })
        )));
        assert!(playing.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(
                    event.kind,
                    TimelineEventKind::RunBoundary {
                        state: RunState::Started,
                        ..
                    }
                )
        )));
        assert!(playing.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::ObjectiveUpdated,
                instance_id: Some(instance_id),
                difficulty_id: Some(7),
                objective_id: Some(9_001),
                objective_value: Some(2),
                objective_complete: Some(false),
                objective_catalog: Some(objective_catalog),
                ..
            }) if instance_id == "555"
                && objective_catalog.resolution
                    == DungeonObjectiveCatalogResolution::ResolvedCurrentBuild
                && objective_catalog.activity_target_key.as_deref()
                    == Some("activity-target.9001")
                && objective_catalog.scene_event_keys == ["scene-event.77"]
        )));

        let duplicate = runtime
            .process(&record(
                3,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(555),
                        flow_info: Some(schema::DungeonFlowInfo {
                            state: Some(DUNGEON_STATE_PLAYING),
                            ..schema::DungeonFlowInfo::default()
                        }),
                        ..schema::DungeonSyncData::default()
                    }),
                }),
            ))
            .unwrap();
        assert!(duplicate.events.is_empty());

        let ended = runtime
            .process(&record(
                4,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(555),
                        flow_info: Some(schema::DungeonFlowInfo {
                            state: Some(DUNGEON_STATE_END),
                            ..schema::DungeonFlowInfo::default()
                        }),
                        ..schema::DungeonSyncData::default()
                    }),
                }),
            ))
            .unwrap();
        assert!(ended.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Ended,
                ..
            })
        )));
        assert!(ended.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(
                    event.kind,
                    TimelineEventKind::RunBoundary {
                        state: RunState::Ended,
                        ..
                    }
                )
        )));
    }

    #[test]
    fn dungeon_flow_snapshot_preserves_raw_values_and_promotes_verified_success_result() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let flow = schema::DungeonFlowInfo {
            state: Some(DUNGEON_STATE_READY),
            active_time: Some(-17),
            ready_time: Some(1_700_000_001),
            play_time: Some(123_456),
            end_time: Some(1_700_000_999),
            settlement_time: Some(45),
            dungeon_times: Some(3),
            result: Some(91),
        };

        let first = runtime
            .process(&record(
                1,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(808),
                        flow_info: Some(flow),
                        scene_info: Some(schema::DungeonSceneInfo {
                            difficulty: Some(20),
                        }),
                        ..schema::DungeonSyncData::default()
                    }),
                }),
            ))
            .unwrap();
        let decoded = first
            .events
            .iter()
            .find_map(|event| match &event.event {
                rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                    kind: DungeonEventKind::Entered,
                    flow: Some(flow),
                    ..
                }) => Some(flow),
                _ => None,
            })
            .expect("entered event with exact flow evidence");
        assert_eq!(decoded.state_id, Some(DUNGEON_STATE_READY));
        assert_eq!(decoded.phase, Some(DungeonFlowPhase::Ready));
        assert_eq!(decoded.active_time_raw, Some(-17));
        assert_eq!(decoded.ready_time_raw, Some(1_700_000_001));
        assert_eq!(decoded.play_time_raw, Some(123_456));
        assert_eq!(decoded.end_time_raw, Some(1_700_000_999));
        assert_eq!(decoded.settlement_time_raw, Some(45));
        assert_eq!(decoded.dungeon_times_raw, Some(3));
        assert_eq!(decoded.result_id, Some(91));

        let update = runtime
            .process(&record(
                2,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(808),
                        flow_info: Some(schema::DungeonFlowInfo {
                            state: Some(DUNGEON_STATE_READY),
                            play_time: Some(123_457),
                            ..schema::DungeonFlowInfo::default()
                        }),
                        ..schema::DungeonSyncData::default()
                    }),
                }),
            ))
            .unwrap();
        assert!(update.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::FlowUpdated,
                flow: Some(DungeonFlowSnapshot {
                    play_time_raw: Some(123_457),
                    ..
                }),
                ..
            })
        )));

        let ended = runtime
            .process(&record(
                3,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(808),
                        flow_info: Some(schema::DungeonFlowInfo {
                            state: Some(DUNGEON_STATE_END),
                            result: Some(1),
                            ..schema::DungeonFlowInfo::default()
                        }),
                        ..schema::DungeonSyncData::default()
                    }),
                }),
            ))
            .unwrap();
        assert!(ended.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Completed,
                flow: Some(DungeonFlowSnapshot {
                    result_id: Some(1),
                    ..
                }),
                ..
            })
        )));
        assert!(ended.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(
                    event.kind,
                    TimelineEventKind::RunBoundary {
                        state: RunState::Completed,
                        ..
                    }
                )
        )));
    }

    #[test]
    fn a_new_instance_restarts_flow_deduplication_even_when_state_is_unchanged() {
        let pack = pack();
        let mut runtime = runtime(&pack);

        for (capture_sequence, scene_uuid) in [(1, 1001), (2, 2002)] {
            let batch = runtime
                .process(&record(
                    capture_sequence,
                    23,
                    encode(schema::SyncDungeonData {
                        dungeon: Some(schema::DungeonSyncData {
                            scene_uuid: Some(scene_uuid),
                            flow_info: Some(schema::DungeonFlowInfo {
                                state: Some(DUNGEON_STATE_PLAYING),
                                ..schema::DungeonFlowInfo::default()
                            }),
                            ..schema::DungeonSyncData::default()
                        }),
                    }),
                ))
                .unwrap();
            assert!(batch.events.iter().any(|event| matches!(
                &event.event,
                rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                    kind: DungeonEventKind::Started,
                    instance_id: Some(instance_id),
                    ..
                }) if instance_id == &scene_uuid.to_string()
            )));
        }
    }

    #[test]
    fn identical_dungeon_objective_snapshots_are_emitted_once() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let snapshot = || schema::SyncDungeonData {
            dungeon: Some(schema::DungeonSyncData {
                scene_uuid: Some(303),
                target: Some(schema::DungeonTarget {
                    target_data: [(
                        9,
                        schema::DungeonTargetData {
                            target_id: Some(44),
                            value: Some(2),
                            complete: Some(0),
                        },
                    )]
                    .into_iter()
                    .collect(),
                }),
                ..schema::DungeonSyncData::default()
            }),
        };

        let first = runtime.process(&record(1, 23, encode(snapshot()))).unwrap();
        let duplicate = runtime.process(&record(2, 23, encode(snapshot()))).unwrap();
        assert!(first.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::ObjectiveUpdated,
                ..
            })
        )));
        assert!(duplicate.events.is_empty());
    }

    #[test]
    fn dungeon_dirty_patch_merges_flow_and_objectives_without_losing_snapshot_identity() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        runtime
            .process(&record(
                1,
                23,
                encode(schema::SyncDungeonData {
                    dungeon: Some(schema::DungeonSyncData {
                        scene_uuid: Some(555),
                        flow_info: Some(schema::DungeonFlowInfo {
                            state: Some(DUNGEON_STATE_PLAYING),
                            ..schema::DungeonFlowInfo::default()
                        }),
                        target: Some(schema::DungeonTarget {
                            target_data: [(
                                41,
                                schema::DungeonTargetData {
                                    target_id: Some(9_001),
                                    value: Some(2),
                                    complete: Some(0),
                                },
                            )]
                            .into_iter()
                            .collect(),
                        }),
                        scene_info: Some(schema::DungeonSceneInfo {
                            difficulty: Some(7),
                        }),
                    }),
                }),
            ))
            .unwrap();

        let flow = safe_dirty_object(vec![(4, safe_dirty_scalar(321))]);
        let objective =
            safe_dirty_object(vec![(2, safe_dirty_scalar(3)), (3, safe_dirty_scalar(1))]);
        let mut objective_map = safe_dirty_scalar(0);
        objective_map.extend(safe_dirty_scalar(0));
        objective_map.extend(safe_dirty_scalar(1));
        objective_map.extend(safe_dirty_scalar(41));
        objective_map.extend(objective);
        let targets = safe_dirty_object(vec![(1, objective_map)]);
        let patch = safe_dirty_object(vec![(2, flow), (4, targets)]);
        let updated = runtime
            .process(&record(
                2,
                24,
                encode(schema::SyncDungeonDirtyData {
                    data: Some(schema::BufferStream {
                        buffer: Some(patch),
                        stream_type: Some(0),
                    }),
                }),
            ))
            .unwrap();

        assert!(updated.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::FlowUpdated,
                instance_id: Some(instance_id),
                difficulty_id: Some(7),
                flow: Some(DungeonFlowSnapshot {
                    state_id: Some(DUNGEON_STATE_PLAYING),
                    play_time_raw: Some(321),
                    ..
                }),
                ..
            }) if instance_id == "555"
        )));
        assert!(updated.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::ObjectiveUpdated,
                objective_map_key: Some(41),
                objective_id: Some(9_001),
                objective_value: Some(3),
                objective_complete: Some(true),
                objective_catalog: Some(objective_catalog),
                ..
            }) if objective_catalog.resolution
                == DungeonObjectiveCatalogResolution::CatalogNotConfigured
        )));

        let mut remove_map = safe_dirty_scalar(0);
        remove_map.extend(safe_dirty_scalar(1));
        remove_map.extend(safe_dirty_scalar(0));
        remove_map.extend(safe_dirty_scalar(41));
        let removed = runtime
            .process(&record(
                3,
                24,
                encode(schema::SyncDungeonDirtyData {
                    data: Some(schema::BufferStream {
                        buffer: Some(safe_dirty_object(vec![(
                            4,
                            safe_dirty_object(vec![(1, remove_map)]),
                        )])),
                        stream_type: Some(0),
                    }),
                }),
            ))
            .unwrap();
        assert!(removed.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::ObjectiveRemoved,
                objective_map_key: Some(41),
                objective_id: Some(9_001),
                ..
            })
        )));
    }

    #[test]
    fn enter_scene_keeps_exact_uuid_and_assigns_log_local_actor_id() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let payload = encode(schema::EnterScene {
            enter_scene_info: Some(schema::EnterSceneInfo {
                scene_attrs: Some(schema::AttrCollection {
                    uuid: None,
                    attributes: vec![int_attr(ATTR_SCENE_ID, 12_000)],
                    map_attributes: Vec::new(),
                }),
                player_entity: Some(schema::Entity {
                    uuid: Some(0x1234_5678_9abc),
                    entity_type: Some(ENTITY_PLAYER),
                    raw_temp_attributes: None,
                    passive_skill_infos: None,
                    buff_infos: None,
                    buff_effect: None,
                    attributes: Some(schema::AttrCollection {
                        uuid: None,
                        attributes: vec![
                            text_attr(ATTR_NAME, "RLogs Tester"),
                            int_attr(ATTR_CLASS_ID, 8),
                            int_attr(ATTR_LEVEL, 60),
                            int_attr(ATTR_SEASON_STRENGTH, 3_505),
                            schema::Attr {
                                id: Some(ATTR_EQUIPMENT),
                                raw_data: Some(encode(schema::AttrEquipmentData {
                                    items: vec![schema::AttrEquipmentItem {
                                        slot_id: Some(WEAPON_SLOT_ID),
                                        item_id: Some(2_000_631),
                                    }],
                                })),
                            },
                            schema::Attr {
                                id: Some(ATTR_ACTIVE_SKILL_LEVELS),
                                raw_data: Some(encode(schema::AttrSkillLevelList {
                                    skills: vec![
                                        schema::AttrSkillLevel {
                                            skill_id: Some(3_948),
                                            current_level: Some(1),
                                            remodel_level: Some(5),
                                        },
                                        schema::AttrSkillLevel {
                                            skill_id: Some(3_969),
                                            current_level: Some(1),
                                            remodel_level: Some(4),
                                        },
                                        schema::AttrSkillLevel {
                                            skill_id: Some(3_011),
                                            current_level: Some(1),
                                            remodel_level: Some(0),
                                        },
                                        schema::AttrSkillLevel {
                                            skill_id: Some(3_022),
                                            current_level: Some(1),
                                            remodel_level: Some(3),
                                        },
                                        schema::AttrSkillLevel {
                                            skill_id: Some(3_021),
                                            current_level: Some(1),
                                            remodel_level: Some(5),
                                        },
                                        schema::AttrSkillLevel {
                                            skill_id: Some(3_012),
                                            current_level: Some(1),
                                            remodel_level: None,
                                        },
                                    ],
                                })),
                            },
                            schema::Attr {
                                id: Some(ATTR_ACTION_SLOTS),
                                raw_data: Some(encode(schema::AttrActionSlots {
                                    slots: std::collections::HashMap::from([
                                        (
                                            8,
                                            schema::AttrActionSlot {
                                                slot_id: Some(8),
                                                skill_id: Some(3_969),
                                            },
                                        ),
                                        (
                                            7,
                                            schema::AttrActionSlot {
                                                slot_id: Some(7),
                                                skill_id: Some(3_948),
                                            },
                                        ),
                                        (
                                            24,
                                            schema::AttrActionSlot {
                                                slot_id: Some(24),
                                                skill_id: Some(3_012),
                                            },
                                        ),
                                        (
                                            22,
                                            schema::AttrActionSlot {
                                                slot_id: Some(22),
                                                skill_id: Some(3_021),
                                            },
                                        ),
                                        (
                                            21,
                                            schema::AttrActionSlot {
                                                slot_id: Some(21),
                                                skill_id: Some(3_022),
                                            },
                                        ),
                                    ]),
                                })),
                            },
                        ],
                        map_attributes: Vec::new(),
                    }),
                }),
                scene_instance_id: Some("scene-instance".into()),
            }),
        });

        let batch = runtime.process(&record(1, 3, payload)).unwrap();

        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        let actor = batch.events.iter().find_map(|event| {
            let rlogs_events::CanonicalEvent::Timeline(event) = &event.event else {
                return None;
            };
            let TimelineEventKind::Actor(actor) = &event.kind else {
                return None;
            };
            Some(actor)
        });
        let actor = actor.expect("actor event");
        assert_eq!(actor.actor.actor_id, ActorId(1));
        assert_eq!(actor.actor.entity_uuid, EntityUuid(0x1234_5678_9abc));
        assert_eq!(actor.display_name.as_deref(), Some("RLogs Tester"));
        assert_eq!(actor.class_id, Some(8));
        assert_eq!(actor.level, Some(60));
        assert_eq!(actor.seasonal_score, Some(3_505));
        assert_eq!(actor.weapon_item_id, Some(2_000_631));
        assert_eq!(actor.primary_loadout.len(), 2);
        assert_eq!(
            actor.loadout_observation,
            ActorLoadoutObservation {
                primary: ActorLoadoutEvidence::ExactSlots,
                auxiliary: ActorLoadoutEvidence::ExactSlots,
            }
        );
        assert_eq!(actor.primary_loadout[0].ability_id, Some(3_948));
        assert_eq!(actor.primary_loadout[0].tier, Some(5));
        assert_eq!(actor.primary_loadout[1].ability_id, Some(3_969));
        assert_eq!(actor.primary_loadout[1].tier, Some(4));
        assert_eq!(actor.auxiliary_loadout.len(), 3);
        assert_eq!(actor.auxiliary_loadout[0].slot_id, 21);
        assert_eq!(actor.auxiliary_loadout[0].ability_id, Some(3_022));
        assert_eq!(actor.auxiliary_loadout[0].item_id, Some(3_000_025));
        assert_eq!(actor.auxiliary_loadout[0].tier, Some(3));
        assert_eq!(actor.auxiliary_loadout[1].slot_id, 22);
        assert_eq!(actor.auxiliary_loadout[1].ability_id, Some(3_021));
        assert_eq!(actor.auxiliary_loadout[1].item_id, Some(3_000_009));
        assert_eq!(actor.auxiliary_loadout[1].tier, None);
        assert_eq!(actor.auxiliary_loadout[2].slot_id, 24);
        assert_eq!(actor.auxiliary_loadout[2].ability_id, Some(3_012));
        assert_eq!(actor.auxiliary_loadout[2].tier, None);
    }

    #[test]
    fn remote_primary_imagines_ignore_auxiliary_imagine_replacements() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let remote_uuid = 257_187_840_640_i64;
        let payload = encode(schema::SyncNearEntities {
            appeared: vec![schema::Entity {
                uuid: Some(remote_uuid),
                entity_type: Some(ENTITY_PLAYER),
                raw_temp_attributes: None,
                buff_infos: None,
                buff_effect: None,
                passive_skill_infos: Some(schema::SeqPassiveSkillInfo {
                    actor_uuid: Some(remote_uuid),
                    passive_infos: vec![schema::PassiveSkillInfo {
                        skill_id: Some(2_203_290),
                        ..schema::PassiveSkillInfo::default()
                    }],
                }),
                attributes: Some(schema::AttrCollection {
                    uuid: None,
                    attributes: vec![schema::Attr {
                        id: Some(ATTR_ACTIVE_SKILL_LEVELS),
                        raw_data: Some(encode(schema::AttrSkillLevelList {
                            // This is the current-build shape observed for a
                            // remote player: four native role actions, one
                            // Imagine replacement action, and two independent
                            // primary Battle Imagine skills. Attribute 226 is
                            // deliberately absent because it is owner-only in
                            // the reviewed captures.
                            skills: vec![
                                schema::AttrSkillLevel {
                                    skill_id: Some(3_011),
                                    current_level: Some(1),
                                    remodel_level: None,
                                },
                                schema::AttrSkillLevel {
                                    skill_id: Some(3_012),
                                    current_level: Some(1),
                                    remodel_level: None,
                                },
                                schema::AttrSkillLevel {
                                    skill_id: Some(3_013),
                                    current_level: Some(1),
                                    remodel_level: None,
                                },
                                schema::AttrSkillLevel {
                                    skill_id: Some(3_014),
                                    current_level: Some(1),
                                    remodel_level: None,
                                },
                                schema::AttrSkillLevel {
                                    skill_id: Some(3_027),
                                    current_level: Some(1),
                                    remodel_level: Some(5),
                                },
                                schema::AttrSkillLevel {
                                    skill_id: Some(3_921),
                                    current_level: Some(1),
                                    remodel_level: Some(3),
                                },
                                schema::AttrSkillLevel {
                                    skill_id: Some(3_968),
                                    current_level: Some(1),
                                    remodel_level: None,
                                },
                            ],
                        })),
                    }],
                    map_attributes: Vec::new(),
                }),
            }],
            disappeared: Vec::new(),
        });

        let batch = runtime.process(&record(1, 6, payload)).unwrap();
        let actor = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::Actor(actor) if actor.actor.entity_uuid.0 == remote_uuid => {
                    Some(actor)
                }
                _ => None,
            },
            _ => None,
        });
        let actor = actor.expect("remote actor event");

        assert_eq!(actor.class_id, Some(11));
        assert_eq!(actor.specialization_id, Some(117));
        assert_eq!(
            actor
                .primary_loadout
                .iter()
                .filter_map(|slot| slot.ability_id)
                .collect::<Vec<_>>(),
            vec![3_921, 3_968]
        );
        assert_eq!(actor.primary_loadout[0].tier, Some(3));
        assert_eq!(actor.primary_loadout[1].tier, Some(0));
        assert!(actor.auxiliary_loadout.is_empty());
        assert!(
            actor
                .primary_loadout
                .iter()
                .all(|slot| slot.ability_id != Some(3_027))
        );
        assert_eq!(
            actor.loadout_observation,
            ActorLoadoutObservation {
                primary: ActorLoadoutEvidence::ObservedSet,
                auxiliary: ActorLoadoutEvidence::Unobserved,
            }
        );
    }

    #[test]
    fn exact_action_slot_snapshot_can_explicitly_clear_both_loadouts() {
        let attributes = schema::AttrCollection {
            uuid: None,
            attributes: vec![schema::Attr {
                id: Some(ATTR_ACTION_SLOTS),
                raw_data: Some(encode(schema::AttrActionSlots {
                    slots: Default::default(),
                })),
            }],
            map_attributes: Vec::new(),
        };

        let (primary, auxiliary, observation) = attr_player_loadouts(&attributes, &[]);

        assert!(primary.is_empty());
        assert!(auxiliary.is_empty());
        assert_eq!(
            observation,
            ActorLoadoutObservation {
                primary: ActorLoadoutEvidence::ExactSlots,
                auxiliary: ActorLoadoutEvidence::ExactSlots,
            }
        );
    }

    #[test]
    fn remote_known_skill_levels_do_not_choose_a_twin_striker_specialization() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let remote_uuid = 220_633_561_600_i64;
        let payload = encode(schema::SyncNearEntities {
            appeared: vec![schema::Entity {
                uuid: Some(remote_uuid),
                entity_type: Some(ENTITY_PLAYER),
                raw_temp_attributes: None,
                buff_infos: None,
                buff_effect: None,
                passive_skill_infos: None,
                attributes: Some(schema::AttrCollection {
                    uuid: None,
                    attributes: vec![
                        int_attr(ATTR_CLASS_ID, 3),
                        schema::Attr {
                            id: Some(ATTR_ACTIVE_SKILL_LEVELS),
                            raw_data: Some(encode(schema::AttrSkillLevelList {
                                skills: vec![schema::AttrSkillLevel {
                                    // Learned/known skills are not proof that
                                    // this action belongs to the selected spec.
                                    skill_id: Some(1_606),
                                    current_level: Some(1),
                                    remodel_level: None,
                                }],
                            })),
                        },
                    ],
                    map_attributes: Vec::new(),
                }),
            }],
            disappeared: Vec::new(),
        });

        let batch = runtime.process(&record(1, 6, payload)).unwrap();
        let actor = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::Actor(actor) if actor.actor.entity_uuid.0 == remote_uuid => {
                    Some(actor)
                }
                _ => None,
            },
            _ => None,
        });
        let actor = actor.expect("remote Twin Striker actor event");
        assert_eq!(actor.class_id, Some(3));
        assert_eq!(actor.specialization_id, None);
    }

    #[test]
    fn twin_striker_primary_observation_outranks_supporting_proc_family() {
        let mut entities = EntityRegistry::new(8);
        entities.resolve(100, Some(ENTITY_PLAYER)).unwrap();
        entities.observe_combat_identity(100, Some(3), None);

        let supporting = entities
            .observe_specialization_ability(100, 35_107)
            .expect("Formless supporting evidence");
        assert_eq!(supporting.specialization_id, Some(128));

        let primary = entities
            .observe_specialization_ability(100, 1_606)
            .expect("Crimson primary evidence");
        assert_eq!(primary.specialization_id, Some(129));
        assert_eq!(
            primary.specialization_evidence,
            Some(SpecializationEvidenceStrength::Primary)
        );

        assert!(
            entities
                .observe_specialization_ability(100, 35_108)
                .is_none(),
            "supporting Formless proc must not overwrite primary Crimson evidence"
        );
    }

    #[test]
    fn exact_passive_selector_locks_the_selected_twin_striker_spec() {
        let mut entities = EntityRegistry::new(8);
        entities.resolve(100, Some(ENTITY_PLAYER)).unwrap();
        entities.observe_combat_identity(100, Some(3), None);
        entities.observe_specialization_ability(100, 35_107);
        entities.observe_specialization_passives(
            100,
            &schema::SeqPassiveSkillInfo {
                actor_uuid: Some(100),
                passive_infos: vec![schema::PassiveSkillInfo {
                    skill_id: Some(2_208_130),
                    ..schema::PassiveSkillInfo::default()
                }],
            },
        );

        assert!(
            entities
                .observe_specialization_ability(100, 1_606)
                .is_none()
        );
        let state = entities.by_uuid.get(&100).expect("tracked Twin Striker");
        assert_eq!(state.specialization_id, Some(128));
        assert_eq!(
            state.specialization_evidence,
            Some(SpecializationEvidenceStrength::Authoritative)
        );
    }

    #[test]
    fn load_scene_end_retains_the_exact_scene_id_and_instance_uuid() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let batch = runtime
            .process(&record(
                1,
                4,
                encode(schema::NotifyLoadSceneEnd {
                    response: Some(schema::NotifyLoadSceneEndResponse {
                        scene_id: Some(8),
                        scene_instance_id: Some("1cf2cfad-fd4b-4e4f-819e-982e6d2ec9e0".into()),
                    }),
                }),
            ))
            .unwrap();

        let world = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::WorldChanged(world) => Some(world),
            _ => None,
        });
        let world = world.expect("world-load completion event");
        assert_eq!(world.scene_id, Some(SceneId(8)));
        assert_eq!(world.map_id, Some(8));
        assert_eq!(
            world.scene_instance_id.as_deref(),
            Some("1cf2cfad-fd4b-4e4f-819e-982e6d2ec9e0")
        );
    }

    #[test]
    fn damage_heal_death_and_summoner_attribution_are_distinct() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let target_uuid = 200;
        let attacker_uuid = 300;
        let owner_uuid = 400;
        let payload = encode(schema::SyncNearDeltaInfo {
            deltas: vec![schema::AoiSyncDelta {
                uuid: Some(target_uuid),
                passive_skill_infos: None,
                attributes: None,
                buff_effect: None,
                skill_effects: Some(schema::SkillEffect {
                    uuid: Some(target_uuid),
                    damage: vec![
                        schema::DamageInfo {
                            damage_source: Some(1),
                            critical: Some(true),
                            damage_type: Some(0),
                            type_flags: Some(DAMAGE_FLAG_CAUSES_LUCKY),
                            value: Some(10_000),
                            actual_value: Some(9_500),
                            hp_loss: Some(9_000),
                            shield_loss: Some(1_000),
                            attacker_uuid: Some(attacker_uuid),
                            top_summoner_uuid: Some(owner_uuid),
                            owner_id: Some(1_234),
                            hit_event_id: Some(77),
                            dead: Some(true),
                            ..schema::DamageInfo::default()
                        },
                        schema::DamageInfo {
                            damage_source: Some(2),
                            damage_type: Some(crate::BpsrDamageType::Heal.protocol_id()),
                            value: Some(2_000),
                            lucky_value: Some(2_400),
                            actual_value: Some(1_850),
                            hp_loss: Some(-1_850),
                            shield_loss: Some(25),
                            attacker_uuid: Some(owner_uuid),
                            owner_id: Some(2_345),
                            owner_level: Some(60),
                            owner_stage: Some(4),
                            hit_event_id: Some(88),
                            property: Some(3),
                            passive_uuid: Some(55_228),
                            ..schema::DamageInfo::default()
                        },
                    ],
                    total_damage: Some(10_000),
                }),
                ..schema::AoiSyncDelta::default()
            }],
        });

        let batch = runtime.process(&record(1, 0x2d, payload)).unwrap();
        let timeline = batch
            .events
            .iter()
            .filter_map(|event| match &event.event {
                rlogs_events::CanonicalEvent::Timeline(event) => Some(&event.kind),
                _ => None,
            })
            .collect::<Vec<_>>();

        let damage = timeline.iter().find_map(|event| match event {
            TimelineEventKind::Damage(damage) => Some(damage),
            _ => None,
        });
        let damage = damage.expect("damage event");
        assert_eq!(damage.source.entity_uuid, EntityUuid(owner_uuid));
        assert_eq!(
            damage.direct_source.map(|source| source.entity_uuid),
            Some(EntityUuid(attacker_uuid))
        );
        assert_eq!(damage.ability, Some(AbilityId(1_234)));
        assert_eq!(damage.hp_loss, Some(9_000));
        assert_eq!(damage.flags.critical, Some(false));
        assert_eq!(damage.flags.lucky, Some(false));
        assert_eq!(damage.flags.causes_lucky, Some(true));
        assert_eq!(damage.packet.reported_critical, Some(true));
        assert_eq!(damage.packet.attacker_uuid, Some(attacker_uuid));
        assert_eq!(damage.packet.top_summoner_uuid, Some(owner_uuid));
        assert_eq!(damage.packet.owner_id, Some(1_234));
        assert_eq!(damage.packet.dead, Some(true));
        assert_eq!(damage.packet.normal_value, Some(10_000));
        assert_eq!(damage.packet.lucky_value, None);
        assert_eq!(damage.packet.type_flags, Some(DAMAGE_FLAG_CAUSES_LUCKY));
        let healing = timeline.iter().find_map(|event| match event {
            TimelineEventKind::Healing(healing) => Some(healing),
            _ => None,
        });
        let healing = healing.expect("healing event");
        assert_eq!(healing.amount, 2_000);
        assert_eq!(healing.actual_amount, Some(1_850));
        assert_eq!(healing.hp_loss, Some(-1_850));
        assert_eq!(healing.shield_loss, Some(25));
        assert_eq!(healing.hit_event_id, Some(88));
        assert_eq!(healing.damage_source, Some(2));
        assert_eq!(healing.damage_type, Some(2));
        assert_eq!(healing.packet.normal_value, Some(2_000));
        assert_eq!(healing.packet.lucky_value, Some(2_400));
        assert_eq!(healing.packet.attacker_uuid, Some(owner_uuid));
        assert_eq!(healing.packet.top_summoner_uuid, None);
        assert_eq!(healing.packet.owner_id, Some(2_345));
        assert_eq!(healing.packet.dead, None);
        assert_eq!(healing.packet.owner_level, Some(60));
        assert_eq!(healing.packet.owner_stage, Some(4));
        assert_eq!(healing.packet.property, Some(3));
        assert_eq!(healing.packet.passive_uuid, Some(55_228));
        assert_eq!(healing.packet.skill_effect_component_index, Some(1));
        assert_eq!(healing.packet.skill_effect_component_count, Some(2));
        assert!(timeline.iter().any(|event| matches!(
            event,
            TimelineEventKind::Life {
                state: LifeState::Died,
                ..
            }
        )));
    }

    #[test]
    fn fake_bullet_lifecycle_preserves_join_keys_without_inventing_provider_ownership() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let container_uuid = 200;
        let target_uuid = 300;
        let valid_record = encode(schema::FakeBulletInfo {
            uuid: Some(44),
            bullet_id: Some(220_101),
            target_id: Some(target_uuid),
            part_id: Some(7),
            offset: Some(schema::FakeBulletVector3 {
                x: Some(1.25),
                y: Some(-2.5),
                z: Some(3.75),
            }),
            rotate: Some(schema::FakeBulletVector3 {
                x: Some(0.0),
                y: Some(90.0),
                z: Some(0.0),
            }),
            skin_id: Some(12),
        });
        let malformed_record = vec![0x0f];
        let payload = encode(schema::SyncNearDeltaInfo {
            deltas: vec![schema::AoiSyncDelta {
                uuid: Some(container_uuid),
                raw_fake_bullets: vec![valid_record.clone(), malformed_record.clone()],
                ..schema::AoiSyncDelta::default()
            }],
        });

        let batch = runtime.process(&record(1, 0x2d, payload)).unwrap();
        let actions = batch
            .events
            .iter()
            .filter_map(|event| match &event.event {
                rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                    TimelineEventKind::UnresolvedAction(action) => Some(action),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(actions.len(), 2);
        let action = actions[0];
        assert_eq!(
            action.container.map(|entity| entity.entity_uuid),
            Some(EntityUuid(container_uuid))
        );
        assert_eq!(
            action.target.map(|entity| entity.entity_uuid),
            Some(EntityUuid(target_uuid))
        );
        assert_eq!(action.action_instance_id, Some(44));
        assert_eq!(action.action_id, Some(220_101));
        assert_eq!(action.target_part_id, Some(7));
        assert_eq!(
            action.wire_action_type,
            Some(crate::BpsrDamageSourceKind::FakeBullet.protocol_id())
        );
        assert_eq!(
            action.reason,
            UnresolvedActionReason::ProviderOwnershipUnproven
        );
        assert_eq!(action.raw_payload, valid_record);

        let malformed = actions[1];
        assert_eq!(
            malformed.reason,
            UnresolvedActionReason::PayloadDecodeFailed
        );
        assert_eq!(malformed.raw_payload, malformed_record);
        assert!(!batch.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(event.kind, TimelineEventKind::Damage(_))
        )));
    }

    #[test]
    fn temporary_attributes_preserve_exact_signed_snapshot_and_delta_values() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let actor_uuid = 200;

        let snapshot_attributes = encode(schema::TempAttrCollection {
            attributes: vec![
                schema::TempAttr {
                    id: Some(90_224),
                    value: Some(3_000),
                },
                schema::TempAttr {
                    id: Some(90_135),
                    value: Some(-2_500),
                },
            ],
        });
        let snapshot = runtime
            .process(&record(
                1,
                6,
                encode(schema::SyncNearEntities {
                    appeared: vec![schema::Entity {
                        uuid: Some(actor_uuid),
                        entity_type: Some(ENTITY_PLAYER),
                        raw_temp_attributes: Some(snapshot_attributes),
                        ..schema::Entity::default()
                    }],
                    disappeared: Vec::new(),
                }),
            ))
            .unwrap();
        let snapshot = snapshot.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::TemporaryAttributes(attributes) => Some(attributes),
                _ => None,
            },
            _ => None,
        });
        let snapshot = snapshot.expect("temporary attribute snapshot");
        assert_eq!(snapshot.actor.entity_uuid, EntityUuid(actor_uuid));
        assert_eq!(snapshot.update_kind, EntityAttributeUpdateKind::Snapshot);
        assert_eq!(
            snapshot.attributes,
            vec![
                TemporaryAttribute {
                    id: 90_224,
                    value: 3_000,
                },
                TemporaryAttribute {
                    id: 90_135,
                    value: -2_500,
                },
            ]
        );

        let delta_attributes = encode(schema::TempAttrCollection {
            attributes: vec![
                schema::TempAttr {
                    id: Some(90_224),
                    value: Some(5_000),
                },
                schema::TempAttr {
                    id: Some(90_135),
                    value: Some(0),
                },
            ],
        });
        let delta = runtime
            .process(&record(
                2,
                0x2d,
                encode(schema::SyncNearDeltaInfo {
                    deltas: vec![schema::AoiSyncDelta {
                        uuid: Some(actor_uuid),
                        raw_temp_attributes: Some(delta_attributes),
                        ..schema::AoiSyncDelta::default()
                    }],
                }),
            ))
            .unwrap();
        let delta = delta.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::TemporaryAttributes(attributes) => Some(attributes),
                _ => None,
            },
            _ => None,
        });
        let delta = delta.expect("temporary attribute delta");
        assert_eq!(delta.actor.entity_uuid, EntityUuid(actor_uuid));
        assert_eq!(delta.update_kind, EntityAttributeUpdateKind::Delta);
        assert_eq!(
            delta.attributes,
            vec![
                TemporaryAttribute {
                    id: 90_224,
                    value: 5_000,
                },
                TemporaryAttribute {
                    id: 90_135,
                    value: 0,
                },
            ]
        );
    }

    #[test]
    fn temporary_attributes_preserve_explicit_empty_snapshot_as_state_clear() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let actor_uuid = 200;
        let empty_snapshot = encode(schema::TempAttrCollection {
            attributes: Vec::new(),
        });

        let decoded = runtime
            .process(&record(
                1,
                6,
                encode(schema::SyncNearEntities {
                    appeared: vec![schema::Entity {
                        uuid: Some(actor_uuid),
                        entity_type: Some(ENTITY_PLAYER),
                        raw_temp_attributes: Some(empty_snapshot),
                        ..schema::Entity::default()
                    }],
                    disappeared: Vec::new(),
                }),
            ))
            .unwrap();

        let snapshot = decoded.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::TemporaryAttributes(attributes) => Some(attributes),
                _ => None,
            },
            _ => None,
        });
        let snapshot = snapshot.expect("explicit empty temporary attribute snapshot");
        assert_eq!(snapshot.actor.entity_uuid, EntityUuid(actor_uuid));
        assert_eq!(snapshot.update_kind, EntityAttributeUpdateKind::Snapshot);
        assert!(snapshot.attributes.is_empty());
    }

    #[test]
    fn entity_attributes_preserve_present_empty_snapshot_but_not_missing_field() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let explicit_actor_uuid = 200;
        let missing_actor_uuid = 300;

        let decoded = runtime
            .process(&record(
                1,
                6,
                encode(schema::SyncNearEntities {
                    appeared: vec![
                        schema::Entity {
                            uuid: Some(explicit_actor_uuid),
                            entity_type: Some(ENTITY_PLAYER),
                            attributes: Some(schema::AttrCollection {
                                uuid: None,
                                attributes: Vec::new(),
                                map_attributes: Vec::new(),
                            }),
                            ..schema::Entity::default()
                        },
                        schema::Entity {
                            uuid: Some(missing_actor_uuid),
                            entity_type: Some(ENTITY_PLAYER),
                            attributes: None,
                            ..schema::Entity::default()
                        },
                    ],
                    disappeared: Vec::new(),
                }),
            ))
            .unwrap();

        let snapshots = decoded
            .events
            .iter()
            .filter_map(|event| match &event.event {
                rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                    TimelineEventKind::EntityAttributes(attributes) => Some(attributes),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].actor.entity_uuid,
            EntityUuid(explicit_actor_uuid)
        );
        assert_eq!(
            snapshots[0].update_kind,
            EntityAttributeUpdateKind::Snapshot
        );
        assert!(snapshots[0].attributes.is_empty());
    }

    #[test]
    fn status_snapshots_and_live_lifecycle_events_keep_exact_effect_identity() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let target_uuid = 200;
        let source_uuid = 300;
        let buff_uuid = 44;
        let effect_id = 9_001;

        let snapshot = encode(schema::SyncNearEntities {
            appeared: vec![schema::Entity {
                uuid: Some(target_uuid),
                entity_type: Some(ENTITY_PLAYER),
                buff_infos: Some(schema::BuffInfoSync {
                    uuid: Some(target_uuid),
                    buff_infos: vec![schema::BuffInfo {
                        buff_uuid: Some(buff_uuid),
                        base_id: Some(effect_id),
                        host_uuid: Some(target_uuid),
                        fire_uuid: Some(source_uuid),
                        layer: Some(2),
                        duration: Some(15_000),
                        ..schema::BuffInfo::default()
                    }],
                }),
                ..schema::Entity::default()
            }],
            disappeared: Vec::new(),
        });
        let applied = runtime.process(&record(1, 6, snapshot.clone())).unwrap();
        let applied = applied.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::Status(status) => Some(status),
                _ => None,
            },
            _ => None,
        });
        let applied = applied.expect("initial status application");
        assert_eq!(applied.effect, StatusEffectId(i64::from(effect_id)));
        assert_eq!(
            applied.instance_id,
            Some(StatusEffectInstanceId(i64::from(buff_uuid)))
        );
        assert_eq!(applied.state, StatusState::Applied);
        assert_eq!(
            applied.source.map(|source| source.entity_uuid),
            Some(EntityUuid(source_uuid))
        );
        assert_eq!(applied.target.entity_uuid, EntityUuid(target_uuid));
        assert_eq!(applied.stacks, Some(2));
        assert_eq!(applied.duration_millis, Some(15_000));

        let repeated = runtime.process(&record(2, 6, snapshot)).unwrap();
        assert!(!repeated.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(event.kind, TimelineEventKind::Status(_))
        )));

        let change = schema::BuffEffectSync {
            uuid: Some(target_uuid),
            buff_effects: vec![schema::BuffEffect {
                event_type: Some(BUFF_EVENT_LAYER_CHANGE_6),
                buff_uuid: Some(buff_uuid),
                logic_effects: vec![schema::BuffEffectLogicInfo {
                    effect_type: Some(BUFF_LOGIC_CHANGE),
                    raw_data: Some(encode(schema::BuffChange {
                        layer: Some(3),
                        duration: Some(20_000),
                        create_time: Some(1_234),
                    })),
                    is_loop: None,
                }],
                ..schema::BuffEffect::default()
            }],
        };
        let stacked = runtime
            .process(&record(
                3,
                0x2d,
                encode(schema::SyncNearDeltaInfo {
                    deltas: vec![schema::AoiSyncDelta {
                        uuid: Some(target_uuid),
                        buff_effect: Some(change),
                        ..schema::AoiSyncDelta::default()
                    }],
                }),
            ))
            .unwrap();
        let stacked = stacked.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::Status(status) => Some(status),
                _ => None,
            },
            _ => None,
        });
        let stacked = stacked.expect("stack status event");
        assert_eq!(stacked.effect, StatusEffectId(i64::from(effect_id)));
        assert_eq!(
            stacked.instance_id,
            Some(StatusEffectInstanceId(i64::from(buff_uuid)))
        );
        assert_eq!(stacked.state, StatusState::Stacked);
        assert_eq!(stacked.stacks, Some(3));
        assert_eq!(stacked.duration_millis, Some(20_000));

        let direction_unknown = runtime
            .process(&record(
                4,
                0x2d,
                encode(schema::SyncNearDeltaInfo {
                    deltas: vec![schema::AoiSyncDelta {
                        uuid: Some(target_uuid),
                        buff_effect: Some(schema::BuffEffectSync {
                            uuid: Some(target_uuid),
                            buff_effects: vec![schema::BuffEffect {
                                event_type: Some(BUFF_EVENT_LAYER_CHANGE_6),
                                buff_uuid: Some(buff_uuid),
                                ..schema::BuffEffect::default()
                            }],
                        }),
                        ..schema::AoiSyncDelta::default()
                    }],
                }),
            ))
            .unwrap();
        assert!(direction_unknown.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(
                    &event.kind,
                    TimelineEventKind::UnresolvedStatus(UnresolvedStatusEvent {
                        instance_id: Some(StatusEffectInstanceId(44)),
                        state: None,
                        reason: UnresolvedStatusReason::AmbiguousTransition,
                        ..
                    })
                )
        )));
        assert!(!direction_unknown.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(&event.kind, TimelineEventKind::Status(_))
        )));

        let removed = runtime
            .process(&record(
                5,
                0x2d,
                encode(schema::SyncNearDeltaInfo {
                    deltas: vec![schema::AoiSyncDelta {
                        uuid: Some(target_uuid),
                        buff_effect: Some(schema::BuffEffectSync {
                            uuid: Some(target_uuid),
                            buff_effects: vec![schema::BuffEffect {
                                event_type: Some(BUFF_EVENT_REMOVE),
                                buff_uuid: Some(buff_uuid),
                                ..schema::BuffEffect::default()
                            }],
                        }),
                        ..schema::AoiSyncDelta::default()
                    }],
                }),
            ))
            .unwrap();
        assert!(removed.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(
                    &event.kind,
                    TimelineEventKind::Status(StatusEvent {
                        effect: StatusEffectId(id),
                        instance_id: Some(StatusEffectInstanceId(instance_id)),
                        state: StatusState::Removed,
                        ..
                    }) if *id == i64::from(effect_id) && *instance_id == i64::from(buff_uuid)
                )
        )));
    }

    #[test]
    fn unknown_status_instance_is_preserved_as_unresolved_lifecycle_without_guessing() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let batch = runtime
            .process(&record(
                1,
                0x2d,
                encode(schema::SyncNearDeltaInfo {
                    deltas: vec![schema::AoiSyncDelta {
                        uuid: Some(200),
                        buff_effect: Some(schema::BuffEffectSync {
                            uuid: Some(200),
                            buff_effects: vec![schema::BuffEffect {
                                event_type: Some(BUFF_EVENT_REMOVE),
                                buff_uuid: Some(999),
                                ..schema::BuffEffect::default()
                            }],
                        }),
                        ..schema::AoiSyncDelta::default()
                    }],
                }),
            ))
            .unwrap();

        assert!(batch.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(
                    &event.kind,
                    TimelineEventKind::UnresolvedStatus(UnresolvedStatusEvent {
                        target: EntityRef { entity_uuid: EntityUuid(200), .. },
                        instance_id: Some(StatusEffectInstanceId(999)),
                        state: Some(StatusState::Removed),
                        wire_event_type: Some(BUFF_EVENT_REMOVE),
                        reason: UnresolvedStatusReason::MissingActiveEffectMapping,
                        ..
                    })
                )
        )));
        assert!(!batch.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(&event.kind, TimelineEventKind::DataGap(_))
        )));
        assert!(!batch.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(&event.kind, TimelineEventKind::Status(_))
        )));
    }

    #[test]
    fn incomplete_status_snapshot_rows_preserve_target_source_and_raw_evidence() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let target_uuid = 200;
        let source_uuid = 300;
        let batch = runtime
            .process(&record(
                1,
                6,
                encode(schema::SyncNearEntities {
                    appeared: vec![schema::Entity {
                        uuid: Some(target_uuid),
                        entity_type: Some(ENTITY_PLAYER),
                        buff_infos: Some(schema::BuffInfoSync {
                            uuid: Some(target_uuid),
                            buff_infos: vec![
                                schema::BuffInfo {
                                    base_id: Some(9_001),
                                    host_uuid: Some(target_uuid),
                                    fire_uuid: Some(source_uuid),
                                    ..schema::BuffInfo::default()
                                },
                                schema::BuffInfo {
                                    buff_uuid: Some(444),
                                    host_uuid: Some(target_uuid),
                                    fire_uuid: Some(source_uuid),
                                    ..schema::BuffInfo::default()
                                },
                            ],
                        }),
                        ..schema::Entity::default()
                    }],
                    disappeared: Vec::new(),
                }),
            ))
            .unwrap();

        let unresolved = batch
            .events
            .iter()
            .filter_map(|event| match &event.event {
                rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                    TimelineEventKind::UnresolvedStatus(status) => Some(status),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(unresolved.len(), 2);
        assert!(unresolved.iter().all(|status| {
            status.target.entity_uuid == EntityUuid(target_uuid)
                && status.source.map(|source| source.entity_uuid) == Some(EntityUuid(source_uuid))
                && !status.raw_payload.is_empty()
        }));
        assert!(unresolved.iter().any(|status| {
            status.instance_id.is_none()
                && status.reason == UnresolvedStatusReason::MissingInstanceId
        }));
        assert!(unresolved.iter().any(|status| {
            status.instance_id == Some(StatusEffectInstanceId(444))
                && status.reason == UnresolvedStatusReason::MissingEffectId
        }));
        assert!(!batch.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Timeline(event)
                if matches!(
                    &event.kind,
                    TimelineEventKind::Status(_) | TimelineEventKind::DataGap(_)
                )
        )));
    }

    #[test]
    fn teammate_combat_skill_emits_specialization_before_its_damage() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let teammate_uuid = 400;
        runtime
            .process(&record(
                1,
                6,
                encode(schema::SyncNearEntities {
                    appeared: vec![schema::Entity {
                        uuid: Some(teammate_uuid),
                        entity_type: Some(ENTITY_PLAYER),
                        raw_temp_attributes: None,
                        passive_skill_infos: None,
                        buff_infos: None,
                        buff_effect: None,
                        attributes: Some(schema::AttrCollection {
                            uuid: None,
                            attributes: vec![int_attr(ATTR_CLASS_ID, 5)],
                            map_attributes: Vec::new(),
                        }),
                    }],
                    disappeared: Vec::new(),
                }),
            ))
            .unwrap();

        let batch = runtime
            .process(&record(
                2,
                0x2d,
                encode(schema::SyncNearDeltaInfo {
                    deltas: vec![schema::AoiSyncDelta {
                        uuid: Some(200),
                        passive_skill_infos: None,
                        attributes: None,
                        buff_effect: None,
                        skill_effects: Some(schema::SkillEffect {
                            uuid: Some(200),
                            damage: vec![schema::DamageInfo {
                                value: Some(10_000),
                                attacker_uuid: Some(teammate_uuid),
                                owner_id: Some(1_541),
                                ..schema::DamageInfo::default()
                            }],
                            total_damage: Some(10_000),
                        }),
                        ..schema::AoiSyncDelta::default()
                    }],
                }),
            ))
            .unwrap();

        let timeline = batch
            .events
            .iter()
            .filter_map(|event| match &event.event {
                rlogs_events::CanonicalEvent::Timeline(event) => Some(&event.kind),
                _ => None,
            })
            .collect::<Vec<_>>();
        let actor_index = timeline
            .iter()
            .position(|event| {
                matches!(
                    event,
                    TimelineEventKind::Actor(ActorEvent {
                        actor: EntityRef {
                            entity_uuid: EntityUuid(400),
                            ..
                        },
                        class_id: Some(5),
                        specialization_id: Some(110),
                        ..
                    })
                )
            })
            .expect("teammate specialization actor update");
        let damage_index = timeline
            .iter()
            .position(|event| matches!(event, TimelineEventKind::Damage(_)))
            .expect("teammate damage event");
        assert!(actor_index < damage_index);
    }

    #[test]
    fn character_decoder_never_needs_account_fields() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let payload = encode(FullProfileEnvelope {
            character: Some(FullCharacterSerialize {
                character_id: Some(987_654),
                base: Some(FullCharacterBase {
                    character_id: Some(987_654),
                    account_id: Some("private-account-value".into()),
                    display_id: Some(123_456),
                    server_id: Some(7),
                    display_name: Some("Profile Name".into()),
                    open_id: Some("private-open-id-value".into()),
                    initial_class_id: Some(8),
                    combat_power: Some(42_000),
                }),
                role_level: Some(schema::RoleLevel {
                    level: Some(60),
                    ..schema::RoleLevel::default()
                }),
            }),
        });

        let batch = runtime.process(&record(1, 0x15, payload)).unwrap();
        let profile = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::CharacterProfileObserved { profile } => Some(profile),
            _ => None,
        });
        let profile = profile.expect("profile event");
        let profile = CharacterProfilePatch::from_game_event(profile).unwrap();
        assert_eq!(profile.character.character_id, "987654");
        assert_eq!(profile.display_name.as_deref(), Some("Profile Name"));
        assert_eq!(profile.level, Some(60));
        assert_eq!(profile.class_id, Some(8));
        assert_eq!(profile.combat_power, Some(42_000));

        let json = serde_json::to_string(&batch.events).unwrap();
        assert!(!json.contains("account_id"));
        assert!(!json.contains("open_id"));
        assert!(!json.contains("token"));
        assert!(!json.contains("private-account-value"));
        assert!(!json.contains("private-open-id-value"));
    }

    #[test]
    fn character_decoder_maps_safe_profile_subtrees() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let item_uuid = 77_i64;
        let module_uuid = 88_i64;
        let payload = encode(schema::SyncContainerData {
            character: Some(schema::CharacterSerialize {
                character_id: Some(987_654),
                base: Some(schema::CharacterBase {
                    character_id: Some(987_654),
                    gender_id: Some(2),
                    face: Some(schema::FaceData {
                        options: [(10, 20)].into_iter().collect(),
                        colors: [(
                            30,
                            schema::IntVec3 {
                                x: Some(40),
                                y: Some(50),
                                z: Some(60),
                            },
                        )]
                        .into_iter()
                        .collect(),
                        height: Some(1.25),
                        body_size_id: Some(1),
                        voice_id: Some(70),
                    }),
                    avatar: Some(schema::CharacterAvatarInfo {
                        avatar_id: Some(41),
                        profile: Some(schema::PictureInfo {
                            url: Some("https://images.example/local-profile.webp".into()),
                        }),
                        half_body: Some(schema::PictureInfo {
                            url: Some("https://images.example/local-half-body.webp".into()),
                        }),
                        business_card_style_id: Some(42),
                        avatar_frame_id: Some(43),
                    }),
                    ..schema::CharacterBase::default()
                }),
                item_package: Some(schema::ItemPackage {
                    packages: [
                        (
                            1,
                            schema::ItemPackageSection {
                                items: [(
                                    item_uuid,
                                    schema::ItemRecord {
                                        uuid: Some(item_uuid),
                                        item_id: Some(10_001),
                                        count: None,
                                        quality: Some(4),
                                        equipment_attributes: Some(schema::EquipmentAttributes {
                                            base: [(1, 100)].into_iter().collect(),
                                            basic: [(2, 200)].into_iter().collect(),
                                            advanced: [(3, 300)].into_iter().collect(),
                                            recast: [(4, 400)].into_iter().collect(),
                                            rare_quality: [(5, 500)].into_iter().collect(),
                                            perfection_value: Some(90),
                                            perfection_level: Some(8),
                                            max_perfection_value: Some(100),
                                            recast_count: Some(2),
                                            total_recast_count: Some(3),
                                            breakthrough_level: Some(1),
                                        }),
                                        module_attributes: None,
                                        module_parts: None,
                                    },
                                )]
                                .into_iter()
                                .collect(),
                            },
                        ),
                        (
                            MODULE_PACKAGE_ID,
                            schema::ItemPackageSection {
                                items: [(
                                    module_uuid,
                                    schema::ItemRecord {
                                        uuid: Some(module_uuid),
                                        item_id: Some(20_001),
                                        count: Some(1),
                                        quality: Some(5),
                                        equipment_attributes: None,
                                        module_attributes: Some(schema::ModuleAttributes {
                                            load_flag: Some(1),
                                            module_type: Some(2),
                                            level: Some(4),
                                        }),
                                        module_parts: Some(schema::ModuleParts {
                                            part_ids: vec![301, 302],
                                            upgrade_records: vec![
                                                schema::ModulePartUpgradeRecord {
                                                    part_id: Some(301),
                                                    succeeded: Some(true),
                                                },
                                            ],
                                        }),
                                    },
                                )]
                                .into_iter()
                                .collect(),
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                }),
                equipment: Some(schema::EquipmentList {
                    equipped: [(
                        7,
                        schema::EquippedItem {
                            slot_id: Some(7),
                            item_uuid: Some(item_uuid as u64),
                            refinement_level: Some(5),
                            refinement_failed_count: Some(2),
                        },
                    )]
                    .into_iter()
                    .collect(),
                    aggregate_attributes: None,
                    recast_attributes: std::collections::HashMap::new(),
                    enchantments: [(
                        item_uuid,
                        schema::EquipmentEnchantment {
                            enchantment_id: Some(20_001),
                            level: Some(3),
                            enchantment_type: Some(2),
                        },
                    )]
                    .into_iter()
                    .collect(),
                    suit_entries: [(
                        102,
                        schema::EquipmentSuitInfo {
                            attributes: [(5_002, 1)].into_iter().collect(),
                            attribute_type: Some(2),
                        },
                    )]
                    .into_iter()
                    .collect(),
                }),
                modules: Some(schema::ModuleData {
                    equipped_slots: [(1, module_uuid)].into_iter().collect(),
                    module_infos: [(
                        module_uuid,
                        schema::ModuleInfo {
                            part_ids: vec![301, 302],
                            upgrade_records: Vec::new(),
                            success_rate: Some(75),
                            initial_link_points: vec![2, 3],
                        },
                    )]
                    .into_iter()
                    .collect(),
                }),
                fashion: Some(schema::FashionManager {
                    equipped_fashion: [(1, 30_001)].into_iter().collect(),
                    owned_fashion: [(30_001, true), (30_002, false)].into_iter().collect(),
                    owned_mounts: [(40_001, true)].into_iter().collect(),
                    owned_weapon_skins: [(50_001, true)].into_iter().collect(),
                    fashion_points: Some(11),
                    mount_points: Some(12),
                    weapon_skin_points: Some(13),
                    owned_dyes: [(60_001, true)].into_iter().collect(),
                }),
                profile_list: Some(schema::ProfileList {
                    unlocked_profile_ids: [(70_001, true)].into_iter().collect(),
                }),
                role_level: Some(schema::RoleLevel {
                    level: Some(60),
                    current_experience: Some(12_345),
                    previous_season_max_level: Some(55),
                }),
                role_face: Some(schema::RoleFace {
                    unlocked_item_ids: [(80_001, true)].into_iter().collect(),
                    unlocked_voice_ids: vec![70, 71],
                }),
                season_center: Some(schema::SeasonCenter { season_id: Some(3) }),
                season_achievements: Some(schema::SeasonAchievementList {
                    seasons: [
                        (
                            0,
                            schema::SeasonAchievement {
                                achievements: [(
                                    10_001,
                                    schema::Achievement {
                                        finish_count: Some(1),
                                        reward_claimed: Some(true),
                                        begin_progress: Some(100),
                                    },
                                )]
                                .into_iter()
                                .collect(),
                            },
                        ),
                        (
                            3,
                            schema::SeasonAchievement {
                                achievements: [(
                                    30_001,
                                    schema::Achievement {
                                        finish_count: Some(7),
                                        reward_claimed: Some(false),
                                        begin_progress: Some(200),
                                    },
                                )]
                                .into_iter()
                                .collect(),
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                    initialized_seasons: [(0, true), (3, true)].into_iter().collect(),
                    version: Some(9),
                }),
                personal_zone: Some(schema::PersonalZone {
                    medals: [(1, 501)].into_iter().collect(),
                    theme_id: Some(2),
                    business_card_style_id: Some(42),
                    avatar_frame_id: Some(43),
                    title_id: Some(3),
                    fashion_collection_points: Some(11),
                    photos: vec![701, 702],
                    ride_collection_points: Some(12),
                    weapon_skin_collection_points: Some(13),
                    photos_wall: [(1, 702)].into_iter().collect(),
                }),
                professions: Some(schema::ProfessionList {
                    current_profession_id: Some(5),
                    professions: [(
                        5,
                        schema::ProfessionInfo {
                            profession_id: Some(5),
                            level: Some(60),
                            experience: Some(54_321),
                            skills: [(
                                101,
                                schema::ProfessionSkillInfo {
                                    skill_id: Some(101),
                                    level: Some(10),
                                    replacement_skill_ids: vec![102],
                                    remodel_level: Some(2),
                                    current_skin_id: Some(7_001),
                                    unlocked_skin_ids: [(7_001, true)].into_iter().collect(),
                                },
                            )]
                            .into_iter()
                            .collect(),
                            active_skill_ids: vec![101],
                            slotted_skill_ids: [(1, 101)].into_iter().collect(),
                            weapon_skin_id: Some(8_001),
                        },
                    )]
                    .into_iter()
                    .collect(),
                    battle_imagine_skills: [
                        (
                            3_948,
                            schema::ProfessionSkillInfo {
                                skill_id: Some(3_948),
                                level: Some(1),
                                replacement_skill_ids: Vec::new(),
                                remodel_level: Some(5),
                                current_skin_id: None,
                                unlocked_skin_ids: std::collections::HashMap::new(),
                            },
                        ),
                        (
                            3_969,
                            schema::ProfessionSkillInfo {
                                skill_id: Some(3_969),
                                level: Some(1),
                                replacement_skill_ids: Vec::new(),
                                remodel_level: Some(5),
                                current_skin_id: None,
                                unlocked_skin_ids: std::collections::HashMap::new(),
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                    total_talent_points: Some(10),
                    total_talent_reset_count: Some(1),
                    talents: [(
                        5,
                        schema::ProfessionTalentInfo {
                            used_talent_points: Some(4),
                            talent_node_ids: vec![901],
                            talent_stage_config_id: Some(123),
                        },
                    )]
                    .into_iter()
                    .collect(),
                }),
                life_professions: Some(schema::LifeProfessionList {
                    professions: [(
                        3,
                        schema::LifeProfessionInfo {
                            profession_id: Some(3),
                            level: Some(4),
                            experience: Some(500),
                            specializations: [(
                                8,
                                schema::LifeProfessionSpecialization {
                                    specialization_id: Some(8),
                                    level: Some(2),
                                },
                            )]
                            .into_iter()
                            .collect(),
                        },
                    )]
                    .into_iter()
                    .collect(),
                }),
                fight_power: Some(schema::FightPower {
                    total: Some(42_000),
                    components: [(
                        1,
                        schema::FightPowerComponent {
                            function_type_id: Some(1),
                            total_points: Some(12_000),
                            points: Some(11_000),
                            subcomponents: [(
                                2,
                                schema::FightPowerSubcomponent {
                                    function_type_id: Some(2),
                                    root_function_type_id: Some(1),
                                    points: Some(3_000),
                                },
                            )]
                            .into_iter()
                            .collect(),
                        },
                    )]
                    .into_iter()
                    .collect(),
                }),
                season_role_levels: Some(schema::SeasonRoleLevelData {
                    levels: [(
                        3,
                        schema::SeasonRoleLevel {
                            level: Some(7),
                            current_experience: Some(777),
                        },
                    )]
                    .into_iter()
                    .collect(),
                }),
                slots: Some(schema::SlotList {
                    slots: [
                        (
                            7,
                            schema::SlotInfo {
                                slot_id: Some(7),
                                skill_id: Some(3_948),
                                auto_battle_disabled: None,
                            },
                        ),
                        (
                            8,
                            schema::SlotInfo {
                                slot_id: Some(8),
                                skill_id: Some(3_969),
                                auto_battle_disabled: None,
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                }),
                ..schema::CharacterSerialize::default()
            }),
        });

        let batch = runtime.process(&record(1, 0x15, payload)).unwrap();
        let profile = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::CharacterProfileObserved { profile } => Some(profile),
            _ => None,
        });
        let profile = CharacterProfilePatch::from_game_event(profile.expect("profile event"))
            .expect("valid BPSR profile patch");

        assert_eq!(profile.class_id, Some(5));
        assert_eq!(profile.level, Some(60));
        let equipped_imagines = profile
            .battle_imagine_skills
            .as_ref()
            .expect("Battle Imagine skills")
            .iter()
            .filter(|skill| skill.equipped_slot.is_some())
            .collect::<Vec<_>>();
        assert_eq!(equipped_imagines.len(), 2);
        assert_eq!(equipped_imagines[0].skill_id, 3_948);
        assert_eq!(equipped_imagines[1].skill_id, 3_969);
        let actor = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::Actor(actor) if !actor.primary_loadout.is_empty() => Some(actor),
                _ => None,
            },
            _ => None,
        });
        let actor = actor.expect("local actor loadout");
        assert_eq!(actor.primary_loadout.len(), 2);
        assert_eq!(actor.primary_loadout[0].item_id, Some(3_000_101));
        assert_eq!(actor.primary_loadout[0].tier, Some(5));
        assert_eq!(actor.primary_loadout[1].item_id, Some(3_000_121));
        assert_eq!(
            profile
                .progression
                .as_ref()
                .and_then(|progression| progression.current_experience),
            Some(12_345)
        );
        let appearance = profile.appearance.as_ref().expect("appearance");
        assert_eq!(appearance.face_options.get(&10), Some(&20));
        assert_eq!(
            appearance.color_options.get(&30),
            Some(&RgbColor {
                red: 40,
                green: 50,
                blue: 60,
            })
        );
        assert_eq!(appearance.avatar_id, Some(41));
        assert_eq!(
            appearance.profile_image_url.as_deref(),
            Some("https://images.example/local-profile.webp")
        );
        assert_eq!(
            appearance.half_body_image_url.as_deref(),
            Some("https://images.example/local-half-body.webp")
        );
        assert_eq!(appearance.voice_id, Some(70));
        assert_eq!(appearance.unlocked_profile_image_ids, vec![70_001]);
        assert_eq!(appearance.unlocked_face_item_ids, vec![80_001]);
        assert_eq!(appearance.unlocked_voice_ids, vec![70, 71]);
        let equipment = profile.equipment.as_ref().expect("equipment");
        assert_eq!(equipment.len(), 1);
        assert_eq!(equipment[0].item_id, 10_001);
        assert_eq!(equipment[0].instance_id.as_deref(), Some("77"));
        assert_eq!(equipment[0].refinement_level, Some(5));
        assert_eq!(
            equipment[0]
                .attributes
                .as_ref()
                .and_then(|attributes| attributes.advanced.get(&3)),
            Some(&300)
        );
        assert_eq!(equipment[0].enchantment_ids, vec![20_001]);
        assert_eq!(equipment[0].enchantments[0].level, Some(3));
        assert_eq!(
            profile.equipment_suit_entries,
            Some(vec![EquipmentSuitEntryProfile {
                map_key: 102,
                attribute_type: Some(2),
                attributes: [(5_002, 1)].into_iter().collect(),
            }])
        );
        let modules = profile.modules.as_ref().expect("modules");
        assert_eq!(
            modules.equipped_slots.get(&1).map(String::as_str),
            Some("88")
        );
        assert_eq!(modules.inventory.len(), 1);
        assert_eq!(modules.inventory[0].config_id, 20_001);
        assert_eq!(modules.inventory[0].level, Some(4));
        assert_eq!(
            modules.inventory[0].parts,
            vec![
                ModulePartProfile {
                    part_id: 301,
                    initial_link_points: Some(2),
                },
                ModulePartProfile {
                    part_id: 302,
                    initial_link_points: Some(3),
                },
            ]
        );
        assert_eq!(
            modules.inventory[0].upgrade_records,
            vec![ModuleUpgradeRecord {
                part_id: 301,
                succeeded: Some(true),
            }]
        );
        let power = profile
            .combat_power_breakdown
            .as_ref()
            .expect("combat-power breakdown");
        assert_eq!(power.total, Some(42_000));
        assert_eq!(power.components[0].function_type_id, 1);
        assert_eq!(power.components[0].subcomponents[0].points, Some(3_000));
        assert_eq!(
            profile.season,
            Some(SeasonProfile {
                season_id: Some(3),
                level: Some(7),
                experience: Some(777),
                power: None,
                strength: None,
            })
        );
        let professions = profile
            .combat_professions
            .as_ref()
            .expect("combat professions");
        assert_eq!(professions[0].skills[0].skill_id, 101);
        assert_eq!(professions[0].talent_node_ids, vec![901]);
        assert_eq!(professions[0].talent_points_used, Some(4));
        assert_eq!(professions[0].talent_stage_config_id, Some(123));
        assert_eq!(
            profile.talent_progress,
            Some(TalentProgressProfile {
                total_points: Some(10),
                total_reset_count: Some(1),
            })
        );
        assert_eq!(
            profile
                .active_skills
                .as_ref()
                .and_then(|skills| skills.first())
                .map(|skill| skill.skill_id),
            Some(101)
        );
        assert_eq!(
            profile
                .life_professions
                .as_ref()
                .and_then(|professions| professions.first())
                .and_then(|profession| profession.specialization_levels.get(&8)),
            Some(&2)
        );
        let collection = profile
            .collection_summary
            .as_ref()
            .expect("collection summary");
        assert_eq!(collection.owned_fashion_ids, vec![30_001]);
        assert_eq!(collection.owned_mount_ids, vec![40_001]);
        assert_eq!(collection.owned_weapon_skin_ids, vec![50_001]);
        assert_eq!(collection.owned_dye_ids, vec![60_001]);
        assert_eq!(collection.photo_ids, vec![701, 702]);
        assert_eq!(collection.photo_wall.get(&1), Some(&702));
        let achievements = collection.achievements.as_ref().expect("achievements");
        assert_eq!(achievements.general[0].achievement_id, 10_001);
        assert_eq!(achievements.general[0].reward_claimed, Some(true));
        assert_eq!(achievements.seasons[0].season_id, 3);
        assert_eq!(
            achievements.seasons[0].achievements[0].achievement_id,
            30_001
        );
        assert_eq!(achievements.initialized_season_ids, vec![0, 3]);
        assert_eq!(achievements.version, Some(9));
    }

    #[test]
    fn dirty_profile_decoder_is_selective_private_and_deduplicated() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let owner_snapshot = encode(FullProfileEnvelope {
            character: Some(FullCharacterSerialize {
                character_id: Some(987_654),
                base: None,
                role_level: None,
            }),
        });
        runtime.process(&record(1, 0x15, owner_snapshot)).unwrap();

        let avatar = blob_object(vec![(1, blob_i32(42)), (4, blob_i32(8)), (5, blob_i32(9))]);
        let base = blob_object(vec![
            (1, blob_i64(987_654)),
            (2, blob_string("private-account-value")),
            (3, blob_i64(123_456)),
            (4, blob_u32(7)),
            (5, blob_string("Updated Name")),
            (22, blob_i32(1)),
            (25, avatar),
            (26, blob_u64(10_252_790)),
            (27, blob_string("private-open-id-value")),
            (31, blob_i32(5)),
            (32, blob_u64(1_785_287_883)),
        ]);
        let scene = blob_object(vec![
            (1, blob_u32(8)),
            (2, blob_u32(4)),
            (13, blob_string("scene-instance")),
            (15, blob_u32(5)),
        ]);
        let level = blob_object(vec![
            (1, blob_i32(60)),
            (2, blob_i64(12_345)),
            (11, blob_i32(55)),
        ]);
        let fight_point = blob_object(vec![(1, blob_i32(43_000))]);
        let dirty_blob = blob_object(vec![
            (2, base),
            (3, scene),
            (22, level),
            (96, fight_point),
            (104, blob_i64(137_201)),
        ]);
        let payload = encode(schema::SyncContainerDirtyData {
            data: Some(schema::BufferStream {
                buffer: Some(dirty_blob),
                stream_type: Some(1),
            }),
        });

        let batch = runtime.process(&record(2, 0x16, payload.clone())).unwrap();
        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        assert_eq!(batch.events.len(), 2);
        let profile_event = batch.events.iter().find(|event| {
            matches!(
                event.event,
                rlogs_events::CanonicalEvent::CharacterProfileObserved { .. }
            )
        });
        assert_eq!(
            profile_event.map(|event| event.sensitivity),
            Some(EventSensitivity::PersonalGameplay)
        );
        let profile = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::CharacterProfileObserved { profile } => Some(profile),
            _ => None,
        });
        let profile = CharacterProfilePatch::from_game_event(profile.expect("dirty profile"))
            .expect("valid BPSR profile patch");
        assert_eq!(profile.character.character_id, "987654");
        assert_eq!(profile.display_name.as_deref(), Some("Updated Name"));
        assert_eq!(profile.display_id.as_deref(), Some("123456"));
        assert_eq!(profile.server_id.as_deref(), Some("7"));
        assert_eq!(profile.class_id, Some(5));
        assert_eq!(profile.level, Some(60));
        assert_eq!(profile.combat_power, Some(43_000));
        assert_eq!(
            profile
                .progression
                .as_ref()
                .and_then(|progression| progression.current_experience),
            Some(12_345)
        );
        let world = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::WorldChanged(world) => Some(world),
            _ => None,
        });
        assert_eq!(world.and_then(|world| world.map_id), Some(8));
        assert_eq!(world.and_then(|world| world.line_id), Some(5));
        let json = serde_json::to_string(&batch.events).unwrap();
        assert!(!json.contains("account_id"));
        assert!(!json.contains("open_id"));
        assert!(!json.contains("private-account-value"));
        assert!(!json.contains("private-open-id-value"));
        assert!(!json.contains("total_online"));
        assert!(!json.contains("save_serial"));

        let duplicate = runtime.process(&record(3, 0x16, payload)).unwrap();
        assert_eq!(duplicate.status, ProtocolDecodeStatus::Decoded);
        assert!(duplicate.events.is_empty());
    }

    #[test]
    fn dirty_action_slot_swap_immediately_reprojects_the_local_imagine_loadout() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let imagine = |skill_id, remodel_level| schema::ProfessionSkillInfo {
            skill_id: Some(skill_id),
            level: Some(1),
            replacement_skill_ids: Vec::new(),
            remodel_level: Some(remodel_level),
            current_skin_id: None,
            unlocked_skin_ids: std::collections::HashMap::new(),
        };
        let payload = encode(schema::SyncContainerData {
            character: Some(schema::CharacterSerialize {
                character_id: Some(987_654),
                professions: Some(schema::ProfessionList {
                    current_profession_id: Some(5),
                    battle_imagine_skills: [
                        (3_948, imagine(3_948, 5)),
                        (3_969, imagine(3_969, 5)),
                        (3_982, imagine(3_982, 4)),
                    ]
                    .into_iter()
                    .collect(),
                    ..schema::ProfessionList::default()
                }),
                slots: Some(schema::SlotList {
                    slots: [
                        (
                            7,
                            schema::SlotInfo {
                                slot_id: Some(7),
                                skill_id: Some(3_948),
                                auto_battle_disabled: None,
                            },
                        ),
                        (
                            8,
                            schema::SlotInfo {
                                slot_id: Some(8),
                                skill_id: Some(3_969),
                                auto_battle_disabled: None,
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                }),
                ..schema::CharacterSerialize::default()
            }),
        });
        runtime.process(&record(1, 0x15, payload)).unwrap();

        let changed_slot = blob_object(vec![(2, blob_i32(3_982))]);
        let mut changes = blob_i32(0); // additions
        changes.extend(blob_i32(0)); // removals
        changes.extend(blob_i32(1)); // updates
        changes.extend(blob_i32(8));
        changes.extend(changed_slot);
        let dirty_payload = encode(schema::SyncContainerDirtyData {
            data: Some(schema::BufferStream {
                buffer: Some(blob_object(vec![(55, blob_object(vec![(1, changes)]))])),
                stream_type: Some(1),
            }),
        });

        let batch = runtime.process(&record(2, 0x16, dirty_payload)).unwrap();
        let actor = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::Actor(actor) if !actor.primary_loadout.is_empty() => Some(actor),
                _ => None,
            },
            _ => None,
        });
        let actor = actor.expect("updated local actor loadout");
        assert_eq!(
            actor
                .primary_loadout
                .iter()
                .map(|item| (item.item_id, item.tier))
                .collect::<Vec<_>>(),
            vec![(Some(3_000_101), Some(5)), (Some(3_001_001), Some(4))]
        );
        assert!(
            actor
                .primary_loadout
                .iter()
                .all(|item| item.item_id != Some(3_000_121))
        );

        let profile = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::CharacterProfileObserved { profile } => Some(profile),
            _ => None,
        });
        let profile = CharacterProfilePatch::from_game_event(profile.expect("updated profile"))
            .expect("valid updated BPSR profile");
        assert_eq!(
            profile
                .equipped_action_slots
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|slot| slot.slot_id == 8)
                .map(|slot| slot.skill_id),
            Some(3_982)
        );
    }

    #[test]
    fn dirty_imagine_tier_update_immediately_reprojects_the_local_loadout() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let payload = encode(schema::SyncContainerData {
            character: Some(schema::CharacterSerialize {
                character_id: Some(987_654),
                professions: Some(schema::ProfessionList {
                    current_profession_id: Some(5),
                    battle_imagine_skills: [(
                        3_982,
                        schema::ProfessionSkillInfo {
                            skill_id: Some(3_982),
                            level: Some(1),
                            replacement_skill_ids: vec![4_001],
                            remodel_level: Some(2),
                            current_skin_id: Some(71),
                            unlocked_skin_ids: [(71, true)].into_iter().collect(),
                        },
                    )]
                    .into_iter()
                    .collect(),
                    ..schema::ProfessionList::default()
                }),
                slots: Some(schema::SlotList {
                    slots: [(
                        8,
                        schema::SlotInfo {
                            slot_id: Some(8),
                            skill_id: Some(3_982),
                            auto_battle_disabled: None,
                        },
                    )]
                    .into_iter()
                    .collect(),
                }),
                ..schema::CharacterSerialize::default()
            }),
        });
        runtime.process(&record(1, 0x15, payload)).unwrap();

        let changed_skill = blob_object(vec![(4, blob_i32(5))]);
        let mut changes = blob_i32(0); // additions
        changes.extend(blob_i32(0)); // removals
        changes.extend(blob_i32(1)); // updates
        changes.extend(blob_i32(3_982));
        changes.extend(changed_skill);
        let dirty_payload = encode(schema::SyncContainerDirtyData {
            data: Some(schema::BufferStream {
                buffer: Some(blob_object(vec![(61, blob_object(vec![(7, changes)]))])),
                stream_type: Some(1),
            }),
        });

        let batch = runtime.process(&record(2, 0x16, dirty_payload)).unwrap();
        let actor = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(event) => match &event.kind {
                TimelineEventKind::Actor(actor) if !actor.primary_loadout.is_empty() => Some(actor),
                _ => None,
            },
            _ => None,
        });
        assert_eq!(
            actor
                .expect("updated local actor loadout")
                .primary_loadout
                .iter()
                .map(|item| (item.item_id, item.tier))
                .collect::<Vec<_>>(),
            vec![(Some(3_001_001), Some(5))]
        );

        let profile = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::CharacterProfileObserved { profile } => Some(profile),
            _ => None,
        });
        let profile = CharacterProfilePatch::from_game_event(profile.expect("updated profile"))
            .expect("valid updated BPSR profile");
        let lucy = profile
            .battle_imagine_skills
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|skill| skill.skill_id == 3_982)
            .expect("Lucy skill record");
        assert_eq!(lucy.remodel_level, Some(5));
        assert_eq!(lucy.equipped_slot, Some(8));
        assert_eq!(lucy.skin_id, Some(71));
        assert_eq!(lucy.replacement_skill_ids, vec![4_001]);
        assert_eq!(lucy.unlocked_skin_ids, vec![71]);
    }

    #[test]
    fn dirty_private_only_update_decodes_without_publishing_an_event() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let private_base = blob_object(vec![
            (26, blob_u64(10_252_790)),
            (32, blob_u64(1_785_287_883)),
        ]);
        let payload = encode(schema::SyncContainerDirtyData {
            data: Some(schema::BufferStream {
                buffer: Some(blob_object(vec![
                    (2, private_base),
                    (104, blob_i64(137_201)),
                ])),
                stream_type: Some(1),
            }),
        });

        let batch = runtime.process(&record(1, 0x16, payload)).unwrap();
        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        assert!(batch.events.is_empty());
    }

    #[test]
    fn social_profile_exposes_public_character_fields_once_and_skips_secrets() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let payload = encode(FullNotifySocialData {
            request: Some(FullNotifySocialDataRequest {
                data: Some(FullSocialData {
                    character_id: Some(987_654),
                    account_id: Some("private-social-account".into()),
                    basic: Some(schema::SocialBasicData {
                        character_id: Some(987_654),
                        display_id: Some(123_456),
                        display_name: Some("Profile Name".into()),
                        gender_id: Some(2),
                        body_size_id: Some(1),
                        level: Some(60),
                        scene_id: Some(8),
                        scene_instance_id: Some("social-scene-instance".into()),
                        season_level: Some(7),
                    }),
                    avatar: Some(schema::SocialAvatarInfo {
                        avatar_id: Some(42),
                        profile: Some(schema::PictureInfo {
                            url: Some("https://images.example/profile.png".into()),
                        }),
                        half_body: Some(schema::PictureInfo {
                            url: Some("https://images.example/half-body.png".into()),
                        }),
                        business_card_style_id: Some(8),
                        avatar_frame_id: Some(9),
                    }),
                    profession: Some(schema::SocialProfessionData {
                        profession_id: Some(5),
                        weapon_skin_id: Some(7001),
                    }),
                    equipment: Some(schema::SocialEquipmentData {
                        items: vec![
                            schema::SocialEquipmentItem {
                                slot_id: Some(1),
                                item_id: Some(10_001),
                            },
                            schema::SocialEquipmentItem {
                                slot_id: Some(2),
                                item_id: Some(0),
                            },
                        ],
                    }),
                    scene: Some(schema::SceneData {
                        map_id: Some(8),
                        channel_id: Some(4),
                        scene_instance_id: Some("social-scene-instance".into()),
                        ..schema::SceneData::default()
                    }),
                    user_attributes: Some(schema::SocialUserAttributes {
                        combat_power: Some(42_000),
                        season_strength: Some(321),
                    }),
                    master_mode_dungeon: Some(schema::MasterModeDungeonData {
                        season_score: Some(765_432),
                        visible: Some(true),
                    }),
                    guild: Some(schema::SocialGuildData {
                        guild_id: Some(7654),
                        guild_name: Some("Public Guild".into()),
                    }),
                    account_data: Some("private-account-data-subtree".into()),
                    personal_zone: Some(schema::SocialPersonalZone {
                        medals: [(0, 101), (1, 101), (2, 202)].into_iter().collect(),
                        business_card_style_id: None,
                        avatar_frame_id: None,
                        title_id: Some(303),
                    }),
                }),
            }),
        });

        let batch = runtime
            .process(&record_for(SOCIAL_SERVICE, 1, 1, payload.clone()))
            .unwrap();
        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        assert_eq!(batch.events.len(), 2);
        let profile = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::CharacterProfileObserved { profile } => Some(profile),
            _ => None,
        });
        let profile = CharacterProfilePatch::from_game_event(profile.expect("social profile"))
            .expect("valid BPSR profile patch");
        assert_eq!(profile.character.character_id, "987654");
        assert_eq!(profile.display_name.as_deref(), Some("Profile Name"));
        assert_eq!(profile.display_id.as_deref(), Some("123456"));
        assert_eq!(profile.level, Some(60));
        assert_eq!(profile.class_id, Some(5));
        assert_eq!(profile.combat_power, Some(42_000));
        assert_eq!(profile.season_strength, Some(321));
        assert_eq!(profile.master_score, Some(765_432));
        assert_eq!(
            profile
                .appearance
                .as_ref()
                .and_then(|appearance| appearance.avatar_id),
            Some(42)
        );
        assert_eq!(
            profile.equipment.as_ref().map(|equipment| equipment.len()),
            Some(1)
        );
        assert_eq!(
            profile
                .social_display
                .as_ref()
                .and_then(|social| social.guild_name.as_deref()),
            Some("Public Guild")
        );
        assert_eq!(
            profile
                .social_display
                .as_ref()
                .map(|social| social.medal_ids.as_slice()),
            Some([101, 202].as_slice())
        );
        assert!(batch.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::WorldChanged(WorldContext {
                map_id: Some(8),
                line_id: Some(4),
                ..
            })
        )));

        let json = serde_json::to_string(&batch.events).unwrap();
        assert!(!json.contains("account_id"));
        assert!(!json.contains("account_data"));
        assert!(!json.contains("private-social-account"));
        assert!(!json.contains("private-account-data-subtree"));
        assert!(json.contains("https://images.example/profile.png"));
        assert!(json.contains("https://images.example/half-body.png"));
        assert!(json.contains("profile_image_url"));
        assert!(json.contains("half_body_image_url"));

        let duplicate = runtime
            .process(&record_for(SOCIAL_SERVICE, 2, 1, payload))
            .unwrap();
        assert_eq!(duplicate.status, ProtocolDecodeStatus::Decoded);
        assert!(duplicate.events.is_empty());
    }

    #[test]
    fn team_member_update_emits_public_progression_for_each_character_once() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let member = |character_id, name: &str, class_id, level, strength| schema::TeamMemberData {
            character_id: Some(character_id),
            enter_time: Some(100),
            talent_id: Some(1),
            online_status: Some(1),
            scene_id: Some(1_631),
            group_id: Some(2),
            social: Some(schema::TeamMemberSocialData {
                basic: Some(schema::SocialBasicData {
                    character_id: Some(character_id),
                    display_id: Some(character_id + 10_000),
                    display_name: Some(name.into()),
                    gender_id: None,
                    body_size_id: None,
                    level: Some(level),
                    scene_id: Some(1_631),
                    scene_instance_id: None,
                    season_level: Some(81),
                }),
                profession: Some(schema::SocialProfessionData {
                    profession_id: Some(class_id),
                    weapon_skin_id: None,
                }),
                equipment: Some(schema::SocialEquipmentData {
                    items: vec![schema::SocialEquipmentItem {
                        slot_id: Some(200),
                        item_id: Some(2_000_631),
                    }],
                }),
                user_attributes: Some(schema::SocialUserAttributes {
                    combat_power: Some(57_280),
                    season_strength: Some(strength),
                }),
            }),
        };
        let payload = encode(schema::NoticeUpdateTeamMemberInfo {
            request: Some(schema::NoticeUpdateTeamMemberInfoRequest {
                members: vec![
                    member(3_296_036, "MarieRose", 11, 60, 3_505),
                    member(9_876_543, "Party Member", 5, 60, 3_211),
                ],
            }),
        });

        let batch = runtime
            .process(&record_for(TEAM_SERVICE, 1, 2, payload.clone()))
            .unwrap();
        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        assert_eq!(batch.events.len(), 5);
        let roster = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::PartyRosterObserved(event) => Some(event),
            _ => None,
        });
        assert!(matches!(
            roster.map(|event| &event.observation),
            Some(PartyRosterObservation::MembersObserved { members })
                if members.iter().map(|member| member.character.character_id.as_str()).collect::<Vec<_>>()
                    == vec!["3296036", "9876543"]
        ));
        let profiles = batch
            .events
            .iter()
            .filter_map(|event| match &event.event {
                rlogs_events::CanonicalEvent::CharacterProfileObserved { profile } => {
                    Some(CharacterProfilePatch::from_game_event(profile).unwrap())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(profiles[0].character.character_id, "3296036");
        assert_eq!(profiles[0].display_name.as_deref(), Some("MarieRose"));
        assert_eq!(profiles[0].class_id, Some(11));
        assert_eq!(profiles[0].specialization_id, None);
        assert_eq!(profiles[0].level, Some(60));
        assert_eq!(profiles[0].combat_power, Some(57_280));
        assert_eq!(profiles[0].season_strength, Some(3_505));
        assert_eq!(profiles[0].equipment.as_ref().unwrap()[0].slot_id, 200);
        assert_eq!(
            profiles[0].equipment.as_ref().unwrap()[0].item_id,
            2_000_631
        );
        assert_eq!(profiles[1].character.character_id, "9876543");
        assert_eq!(profiles[1].class_id, Some(5));
        let actor_updates = batch
            .events
            .iter()
            .filter_map(|event| match &event.event {
                rlogs_events::CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                    TimelineEventKind::Actor(actor) => Some(actor),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(actor_updates.len(), 2);
        assert_eq!(actor_updates[0].actor.entity_uuid.0, 3_296_036_i64 << 16);
        assert_eq!(actor_updates[0].ability_score, Some(57_280));
        assert_eq!(actor_updates[0].seasonal_score, Some(3_505));

        let duplicate = runtime
            .process(&record_for(TEAM_SERVICE, 2, 2, payload))
            .unwrap();
        assert_eq!(duplicate.status, ProtocolDecodeStatus::Decoded);
        assert_eq!(duplicate.events.len(), 1);
        assert!(matches!(
            &duplicate.events[0].event,
            rlogs_events::CanonicalEvent::PartyRosterObserved(PartyRosterEvent {
                observation: PartyRosterObservation::MembersObserved { members },
            }) if members.len() == 2
        ));
    }

    #[test]
    fn team_join_snapshot_reuses_the_privacy_reviewed_member_projection() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let payload = encode(schema::NotifyJoinTeam {
            request: Some(schema::NotifyJoinTeamRequest {
                base_info: Some(schema::TeamBaseInfo {
                    team_id: Some(8_765_432_109),
                }),
                members: vec![schema::TeamMemberData {
                    character_id: Some(9_876_543),
                    enter_time: Some(100),
                    talent_id: Some(1),
                    online_status: Some(1),
                    scene_id: Some(1_621),
                    group_id: Some(3),
                    social: Some(schema::TeamMemberSocialData {
                        basic: Some(schema::SocialBasicData {
                            character_id: Some(9_876_543),
                            display_id: Some(98_765_543),
                            display_name: Some("Party Member".into()),
                            gender_id: None,
                            body_size_id: None,
                            level: Some(60),
                            scene_id: Some(1_621),
                            scene_instance_id: None,
                            season_level: Some(81),
                        }),
                        profession: Some(schema::SocialProfessionData {
                            profession_id: Some(5),
                            weapon_skin_id: None,
                        }),
                        equipment: Some(schema::SocialEquipmentData {
                            items: vec![schema::SocialEquipmentItem {
                                slot_id: Some(200),
                                item_id: Some(2_000_551),
                            }],
                        }),
                        user_attributes: Some(schema::SocialUserAttributes {
                            combat_power: Some(59_106),
                            season_strength: Some(3_462),
                        }),
                    }),
                }],
            }),
        });

        let batch = runtime
            .process(&record_for(TEAM_SERVICE, 1, 3, payload))
            .unwrap();
        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        let roster = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::PartyRosterObserved(event) => Some(event),
            _ => None,
        });
        assert!(matches!(
            roster.map(|event| &event.observation),
            Some(PartyRosterObservation::FullSnapshot {
                party_id: Some(party_id),
                members,
            }) if party_id == "8765432109"
                && members.len() == 1
                && members[0].character.character_id == "9876543"
                && members[0].group_id == Some(3)
        ));
        let actor = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Actor(actor) => Some(actor),
                _ => None,
            },
            _ => None,
        });
        let actor = actor.expect("team-join actor projection");
        assert_eq!(actor.class_id, Some(5));
        assert_eq!(actor.level, Some(60));
        assert_eq!(actor.ability_score, Some(59_106));
        assert_eq!(actor.seasonal_score, Some(3_462));
        assert_eq!(actor.weapon_item_id, Some(2_000_551));
        assert_eq!(actor.specialization_id, None);

        let json = serde_json::to_string(&batch.events).unwrap();
        assert!(!json.contains("account"));
        assert!(!json.contains("token"));
        assert!(!json.contains("password"));
    }

    #[test]
    fn party_leave_and_dissolve_preserve_exact_lifecycle_without_inference() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let leave = runtime
            .process(&record_for(
                TEAM_SERVICE,
                1,
                4,
                encode(schema::NotifyLeaveTeam {
                    request: Some(schema::NotifyLeaveTeamRequest {
                        character_id: Some(9_876_543),
                        leave_type: Some(7),
                    }),
                }),
            ))
            .unwrap();
        assert!(matches!(
            &leave.events[0].event,
            rlogs_events::CanonicalEvent::PartyRosterObserved(PartyRosterEvent {
                observation: PartyRosterObservation::MemberLeft {
                    member,
                    leave_type: Some(7),
                },
            }) if member.character_id == "9876543"
        ));

        let dissolve = runtime
            .process(&record_for(
                TEAM_SERVICE,
                2,
                13,
                encode(schema::NoticeTeamDissolve {
                    request: Some(schema::NoticeTeamDissolveRequest {}),
                }),
            ))
            .unwrap();
        assert!(matches!(
            &dissolve.events[0].event,
            rlogs_events::CanonicalEvent::PartyRosterObserved(PartyRosterEvent {
                observation: PartyRosterObservation::Dissolved,
            })
        ));
    }

    #[test]
    fn season_update_attaches_to_the_privacy_reviewed_local_character() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        runtime
            .process(&record(
                1,
                21,
                encode(schema::SyncContainerData {
                    character: Some(schema::CharacterSerialize {
                        character_id: Some(987_654),
                        ..schema::CharacterSerialize::default()
                    }),
                }),
            ))
            .unwrap();

        let batch = runtime
            .process(&record(
                2,
                27,
                encode(schema::SyncSeason { season_id: Some(3) }),
            ))
            .unwrap();
        let profile = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::CharacterProfileObserved { profile } => Some(profile),
            _ => None,
        });
        let profile = CharacterProfilePatch::from_game_event(profile.expect("season profile"))
            .expect("valid BPSR profile patch");
        assert_eq!(profile.character.character_id, "987654");
        assert_eq!(
            profile.season,
            Some(SeasonProfile {
                season_id: Some(3),
                level: None,
                experience: None,
                power: None,
                strength: None,
            })
        );
    }

    #[test]
    fn local_skill_notification_emits_an_exact_cast_after_self_entity_resolution() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let self_delta = runtime
            .process(&record(
                1,
                0x2e,
                encode(schema::SyncToMeDeltaInfo {
                    delta: Some(schema::AoiSyncToMeDelta {
                        base_delta: None,
                        hate_ids: Vec::new(),
                        cooldowns: Vec::new(),
                        fight_resource_cooldowns: Vec::new(),
                        uuid: Some(216_009_015_936),
                    }),
                }),
            ))
            .unwrap();
        assert_eq!(self_delta.status, ProtocolDecodeStatus::Decoded);

        let batch = runtime
            .process(&record(
                2,
                0x43,
                encode(schema::SyncClientUseSkill {
                    skill_target_uuid: Some(1_310_784),
                    skill_level_id: Some(152_501),
                }),
            ))
            .unwrap();
        let cast = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Cast(cast) => Some(cast),
                _ => None,
            },
            _ => None,
        });
        let cast = cast.expect("canonical local skill cast");
        assert_eq!(cast.source.entity_uuid.0, 216_009_015_936);
        assert_eq!(cast.target.unwrap().entity_uuid.0, 1_310_784);
        assert_eq!(cast.ability.0, 152_501);
        assert_eq!(cast.state, CastState::Started);
    }

    #[test]
    fn current_build_client_use_slot_emits_exact_canonical_action_timing() {
        const TEST_CLIENT_WORLD_SERVICE: u64 = 0x00b5_b5b5;
        let pack = current_build_use_slot_pack(TEST_CLIENT_WORLD_SERVICE);
        let mut runtime = current_build_runtime(&pack);
        let self_delta = runtime
            .process(&record(
                1,
                0x2e,
                encode(schema::SyncToMeDeltaInfo {
                    delta: Some(schema::AoiSyncToMeDelta {
                        base_delta: None,
                        hate_ids: Vec::new(),
                        cooldowns: Vec::new(),
                        fight_resource_cooldowns: Vec::new(),
                        uuid: Some(216_009_015_936),
                    }),
                }),
            ))
            .unwrap();
        assert_eq!(self_delta.status, ProtocolDecodeStatus::Decoded);

        let route = RouteKey::new(
            PacketDirection::ClientToServer,
            FragmentKind::Call,
            TEST_CLIENT_WORLD_SERVICE,
            0x3d002,
        );
        let batch = runtime
            .process(&record_for_route(
                2,
                route,
                crate::use_skill_attr::tests::world_skill_use_payload(),
            ))
            .unwrap();
        assert_eq!(batch.status, ProtocolDecodeStatus::Decoded);
        let cast = batch.events.iter().find_map(|event| match &event.event {
            rlogs_events::CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Cast(cast) => Some(cast),
                _ => None,
            },
            _ => None,
        });
        let cast = cast.expect("canonical current-build UseSlot cast");
        assert_eq!(cast.source.entity_uuid.0, 216_009_015_936);
        assert_eq!(cast.target.unwrap().entity_uuid.0, 216_009_015_936);
        assert_eq!(cast.ability.0, 2_233);
        assert_eq!(cast.state, CastState::Started);
        let timing = cast.action_timing.expect("exact decrypted action timing");
        assert_eq!(timing.action_instance_id, 9_001);
        assert_eq!(timing.base_ability.0, 2_233);
        assert_eq!(timing.ability_level, 5);
        assert_eq!(timing.slot_id, 21);
        assert_eq!(timing.client_timestamp_raw, 1_786_202_388_123);
        assert_eq!(timing.begin_time_raw, 1_786_202_388_120);
        assert_eq!(timing.attack_speed_basis_points, 230);
        assert_eq!(timing.cast_speed_basis_points, 382);
        assert_eq!(timing.charge_speed_basis_points, 145);
        assert!(timing.passive);
        assert!(timing.activated_roulette);
        assert_eq!(timing.target_part_id, 3);
    }

    #[test]
    fn hp_and_resource_formula_attributes_are_decoded_without_changing_their_ids() {
        assert_eq!(
            decode_attribute_value(ATTR_CURRENT_HP, &[]),
            Some(EntityAttributeValue::Integer(0))
        );
        assert_eq!(
            decode_attribute_value(ATTR_CURRENT_HP, &[0x86, 0x9d, 0x23]),
            Some(EntityAttributeValue::Integer(577_158))
        );
        assert_eq!(
            decode_attribute_value(ATTR_MAX_HP_FINAL, &[0x86, 0x9d, 0x23]),
            Some(EntityAttributeValue::Integer(577_158))
        );
        assert_eq!(
            decode_attribute_value(ATTR_MAX_HP_PERCENT, &[0x98, 0x11]),
            Some(EntityAttributeValue::Integer(2_200))
        );
        assert_eq!(
            decode_attribute_value(ATTR_CURRENT_ENERGY, &[0xc8, 0xd6, 0x05]),
            Some(EntityAttributeValue::Integer(93_000))
        );
    }

    #[test]
    fn attack_and_mastery_formula_attributes_decode_packet_observed_varints() {
        // Captured before the source actor's first damage event.
        assert_eq!(
            decode_attribute_value(ATTR_PHYSICAL_ATTACK, &[0xf3, 0x2c]),
            Some(EntityAttributeValue::Integer(5_747))
        );
        // The magical sibling is not present in the physical-player fixtures,
        // but IL2CPP proves the exact attribute ID and scalar representation.
        assert_eq!(
            decode_attribute_value(ATTR_MAGICAL_ATTACK, &[0xd2, 0x09]),
            Some(EntityAttributeValue::Integer(1_234))
        );
        assert_eq!(
            decode_attribute_value(ATTR_MASTERY, &[0xb7, 0x07]),
            Some(EntityAttributeValue::Integer(951))
        );
    }

    #[test]
    fn team_luck_formula_attributes_decode_exact_packet_observed_varints() {
        assert_eq!(
            decode_attribute_value(12510, &[0xa9, 0x98, 0x01]),
            Some(EntityAttributeValue::Integer(19_497))
        );
        assert_eq!(
            decode_attribute_value(12530, &[0xf1, 0x22]),
            Some(EntityAttributeValue::Integer(4_465))
        );
        assert_eq!(decode_attribute_value(12511, &[0xf1, 0x22]), None);
        assert_eq!(decode_attribute_value(12531, &[0xf1, 0x22]), None);
    }

    #[test]
    fn inspiration_formula_attributes_decode_exact_packet_observed_varints() {
        assert_eq!(
            decode_attribute_value(11710, &[0x82, 0x49]),
            Some(EntityAttributeValue::Integer(9_346))
        );
        for attribute_id in [
            11712, 11780, 11782, 11840, 11930, 11940, 11942, 11950, 11952, 13170,
        ] {
            assert_eq!(
                decode_attribute_value(attribute_id, &[0xa0, 0x06]),
                Some(EntityAttributeValue::Integer(800))
            );
        }
        assert_eq!(decode_attribute_value(11711, &[0xa0, 0x06]), None);
        assert_eq!(decode_attribute_value(11781, &[0xa0, 0x06]), None);
        assert_eq!(decode_attribute_value(13171, &[0xa0, 0x06]), None);
    }

    #[test]
    fn inspire_action_speed_attributes_decode_exact_packet_observed_varints() {
        assert_eq!(
            decode_attribute_value(11720, &[0xff, 0x21]),
            Some(EntityAttributeValue::Integer(4_351))
        );
        assert_eq!(
            decode_attribute_value(11730, &[0xf4, 0x66]),
            Some(EntityAttributeValue::Integer(13_172))
        );
        assert_eq!(decode_attribute_value(11721, &[0xff, 0x21]), None);
        assert_eq!(decode_attribute_value(11731, &[0xf4, 0x66]), None);
    }

    #[test]
    fn elemental_damage_final_attributes_decode_without_opening_sibling_components() {
        for attribute_id in [13110, 13120, 13130, 13140, 13150, 13160, 13170, 13180] {
            assert_eq!(
                decode_attribute_value(attribute_id, &[0xd2, 0x1c]),
                Some(EntityAttributeValue::Integer(3_666))
            );
            assert_eq!(
                decode_attribute_value(attribute_id + 1, &[0xd2, 0x1c]),
                None
            );
        }
    }

    #[test]
    fn rdps_primary_and_attack_component_families_decode_proven_varints() {
        // Exact old-build Harmony replay: Dexterity final and its externally
        // transferable percentage component. The same scalar representation
        // is used by the Strength and Intelligence sibling families.
        for attribute_id in [11010, 11020, 11030] {
            assert_eq!(
                decode_attribute_value(attribute_id, &[0xc1, 0x3d]),
                Some(EntityAttributeValue::Integer(7_873))
            );
        }
        for attribute_id in [11014, 11024, 11034] {
            assert_eq!(
                decode_attribute_value(attribute_id, &[0xc8, 0x01]),
                Some(EntityAttributeValue::Integer(200))
            );
        }

        // Versioned formula runtimes also consume the physical and magical
        // attack component siblings rather than only their final values.
        for attribute_id in [11331, 11332, 11333, 11334, 11341, 11342, 11343, 11344] {
            assert_eq!(
                decode_attribute_value(attribute_id, &[0xc8, 0x01]),
                Some(EntityAttributeValue::Integer(200))
            );
        }
        assert_eq!(
            decode_attribute_value(
                11334,
                &[0xb8, 0xe5, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
            ),
            Some(EntityAttributeValue::Integer(-3_400))
        );

        // Current-build Fatal Spiral application/removal values retained from
        // actor 6 in wire packets 5087 and the matching removal packet.
        for attribute_id in 13100..=13102 {
            assert_eq!(
                decode_attribute_value(attribute_id, &[0xd5, 0x09]),
                Some(EntityAttributeValue::Integer(1_237))
            );
            assert_eq!(
                decode_attribute_value(attribute_id, &[0xed, 0x01]),
                Some(EntityAttributeValue::Integer(237))
            );
        }
        for attribute_id in 13103..=13105 {
            assert_eq!(
                decode_attribute_value(attribute_id, &[]),
                Some(EntityAttributeValue::Integer(0))
            );
        }
    }

    #[test]
    fn breaking_stage_attribute_decodes_exact_enum_values() {
        assert_eq!(
            decode_attribute_value(ATTR_BREAKING_STAGE, &[]),
            Some(EntityAttributeValue::Integer(0))
        );
        assert_eq!(
            decode_attribute_value(ATTR_BREAKING_STAGE, &[1]),
            Some(EntityAttributeValue::Integer(1))
        );
    }

    #[test]
    fn paired_summon_owner_attributes_decode_as_stable_entity_uuids() {
        let encoded_owner = [128, 133, 240, 150, 151, 1];
        for attribute_id in [ATTR_SUMMON_OWNER_PRIMARY, ATTR_SUMMON_OWNER_CONFIRMATION] {
            assert_eq!(
                decode_attribute_value(attribute_id, &encoded_owner),
                Some(EntityAttributeValue::Integer(40_581_726_848))
            );
        }
    }

    #[test]
    fn actor_ownership_requires_the_complete_confirming_attribute_pair() {
        let actor = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid(557_440),
        };
        let owner = 1_310_784;
        let attribute = |attribute_id, value| EntityAttribute {
            attribute_id,
            raw_value: Vec::new(),
            decoded: Some(EntityAttributeValue::Integer(value)),
        };

        assert_eq!(
            decode_actor_ownership(
                actor,
                &[
                    attribute(ATTR_SUMMON_OWNER_PRIMARY, owner),
                    attribute(ATTR_SUMMON_OWNER_CONFIRMATION, owner),
                ],
            ),
            Some(ActorOwnershipUpdate::Confirmed {
                owner_entity_uuid: EntityUuid(owner),
            })
        );
        assert_eq!(
            decode_actor_ownership(actor, &[attribute(ATTR_SUMMON_OWNER_PRIMARY, owner)]),
            None
        );
        assert_eq!(
            decode_actor_ownership(
                actor,
                &[
                    attribute(ATTR_SUMMON_OWNER_PRIMARY, owner),
                    attribute(ATTR_SUMMON_OWNER_CONFIRMATION, owner + 1),
                ],
            ),
            Some(ActorOwnershipUpdate::Cleared)
        );
    }

    #[test]
    fn malformed_reviewed_payload_becomes_a_data_gap_without_raw_bytes() {
        let pack = pack();
        let mut runtime = runtime(&pack);
        let batch = runtime.process(&record(1, 3, vec![0xff])).unwrap();

        assert_eq!(batch.status, ProtocolDecodeStatus::DecodeFailed);
        assert_eq!(batch.events.len(), 1);
        let json = serde_json::to_string(&batch.events).unwrap();
        assert!(json.contains("decode_failure"));
        assert!(!json.contains("255"));
    }
}
