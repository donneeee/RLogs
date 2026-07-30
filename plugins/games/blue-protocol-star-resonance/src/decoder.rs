use std::collections::BTreeMap;

use prost::Message;
use rlogs_events::{
    AbilityId, ActorEvent, ActorId, ActorKind, ActorState, BoundaryReason, CanonicalEventDraft,
    CanonicalEventDraftKind, CharacterIdentity, CooldownEvent, DamageEvent, DamageFlags,
    DataGapEvent, DataGapKind, DungeonEvent, DungeonEventKind, DungeonFlowPhase,
    DungeonFlowSnapshot, EntityAttribute, EntityAttributeEvent, EntityAttributeValue, EntityRef,
    EntityUuid, EventEnvelope, EventEnvelopeFactory, EventProvenance, EventSensitivity, EventTime,
    HealingEvent, LifeState, MonsterId, PositionEvent, RegionContext, RegionEvidence,
    RegionEvidenceKind, RegionIdentity, RunState, SceneId, TimelineEventKind, WorldContext,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dirty_blob_v1;
use crate::dungeon_dirty_v1::{self, DirtyDungeonObjectiveMutation};
use crate::game_schema_v1 as schema;
use crate::{
    ActivityProgress, AllowedDataDomain, BattleImagineSkill, CaptureGapKind, CaptureRecord,
    CaptureRecordKind, CharacterAppearance, CharacterProfilePatch, CharacterProgression,
    CollectionSummary, CombatPowerBreakdown, CombatPowerComponent, CombatPowerSubcomponent,
    CombatProfessionProfile, CultivationAreaProfile, CultivationLineProfile, DecodeDisposition,
    DungeonProgress, DungeonTargetProgress, EquipmentAttributeProfile, EquipmentEnchantmentProfile,
    EquipmentItem, GameBuild, HandbookProgress, LifeProfessionProfile, MasterModeDungeonProgress,
    ModuleItemProfile, ModulePartProfile, ModuleProfile, ModuleUpgradeRecord, ProhibitedDataClass,
    ProtocolPack, ReputationProgress, RgbColor, SeasonCultivationProfile, SeasonMedalHole,
    SeasonMedalNode, SeasonMedalProfile, SeasonProfile, SkillLevel, SocialDisplay, TalentLevel,
    TalentProgressProfile, WeeklyTowerProgress,
};

const ATTR_NAME: i32 = 0x01;
const ATTR_MONSTER_ID: i32 = 0x0a;
const ATTR_POSITION: i32 = 0x34;
const ATTR_CLASS_ID: i32 = 0xdc;
const ATTR_LEVEL: i32 = 0x2710;
const ATTR_COMBAT_POWER: i32 = 0x272e;
const ATTR_SEASON_STRENGTH: i32 = 0x2cb0;
const ATTR_SCENE_ID: i32 = 0x155;
const ATTR_SCENE_LINE: i32 = 0x157;

const ENTITY_PLAYER: i32 = 10;
const MODULE_PACKAGE_ID: i32 = 5;
const DAMAGE_TYPE_HEAL: i32 = 2;
const DAMAGE_FLAG_BLOCKED: i32 = 0b0010;
const DAMAGE_FLAG_LUCKY: i32 = 0b0100;

const DUNGEON_STATE_NULL: i32 = 0;
const DUNGEON_STATE_ACTIVE: i32 = 1;
const DUNGEON_STATE_READY: i32 = 2;
const DUNGEON_STATE_PLAYING: i32 = 3;
const DUNGEON_STATE_END: i32 = 4;
const DUNGEON_STATE_SETTLEMENT: i32 = 5;
const DUNGEON_STATE_VOTE: i32 = 6;

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
    SyncNearDeltaV1,
    SyncToMeDeltaV1,
    NotifyReviveV1,
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
            | Self::SyncSeasonV1 => AllowedDataDomain::CharacterProfile,
            Self::SyncNearDeltaV1 | Self::SyncToMeDeltaV1 | Self::NotifyReviveV1 => {
                AllowedDataDomain::Combat
            }
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
    entities: EntityRegistry,
    dungeon: DungeonTracker,
    profile: ProfileTracker,
    envelopes: EventEnvelopeFactory,
    server_clock: Option<ServerClockAnchor>,
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
            entities: EntityRegistry::new(config.max_entities),
            dungeon: DungeonTracker::default(),
            profile: ProfileTracker::default(),
            envelopes: EventEnvelopeFactory::new(session_id, region_context),
            server_clock: None,
            config,
        })
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
                    &mut self.dungeon,
                    &mut self.profile,
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
        let events = decoded
            .drafts
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

    fn game_time_millis(&self, observed_micros: u64) -> Option<i64> {
        let anchor = self.server_clock?;
        let elapsed_millis =
            i64::try_from(observed_micros.checked_sub(anchor.observed_micros)? / 1_000).ok()?;
        anchor.server_milliseconds.checked_add(elapsed_millis)
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
    last_dirty_profile: Option<CharacterProfilePatch>,
    last_dirty_world: Option<WorldContext>,
    last_social_profile: Option<CharacterProfilePatch>,
    last_social_world: Option<WorldContext>,
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
        };
        self.by_uuid.insert(uuid, state);
        Ok(state)
    }
}

fn decode_message(
    decoder: DecoderKind,
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    dungeon: &mut DungeonTracker,
    profile: &mut ProfileTracker,
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
        DecoderKind::EnterSceneV1 => decode_enter_scene(payload, metadata, entities),
        DecoderKind::NotifyLoadSceneEndV1 => decode_load_scene_end(payload, metadata),
        DecoderKind::SyncNearEntitiesV1 => decode_sync_near_entities(payload, metadata, entities),
        DecoderKind::SyncContainerDataV1 => {
            decode_sync_container(payload, metadata, &mut profile.local_character)
        }
        DecoderKind::SyncContainerDirtyDataV1 => {
            decode_sync_container_dirty(payload, metadata, profile)
        }
        DecoderKind::NotifySocialDataV1 => decode_notify_social_data(payload, metadata, profile),
        DecoderKind::SyncNearDeltaV1 => decode_sync_near_delta(payload, metadata, entities),
        DecoderKind::SyncToMeDeltaV1 => decode_sync_to_me_delta(payload, metadata, entities),
        DecoderKind::NotifyReviveV1 => decode_revive(payload, metadata, entities),
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
                    season: Some(SeasonProfile {
                        season_id: Some(i64::from(season_id)),
                        level: None,
                        experience: None,
                        power: None,
                        strength: None,
                    }),
                    appearance: None,
                    equipment: None,
                    modules: None,
                    owned_imagines: None,
                    battle_imagine_skills: None,
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
        let business_card_style_id = avatar
            .and_then(|avatar| positive_i32(avatar.business_card_style_id))
            .or_else(|| personal_zone.and_then(|zone| positive_i32(zone.business_card_style_id)));
        let avatar_frame_id = avatar
            .and_then(|avatar| positive_i32(avatar.avatar_frame_id))
            .or_else(|| personal_zone.and_then(|zone| positive_i32(zone.avatar_frame_id)));
        let appearance = if basic.and_then(|basic| basic.gender_id).is_some()
            || basic.and_then(|basic| basic.body_size_id).is_some()
            || avatar_id.is_some()
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
                business_card_style_id,
                avatar_frame_id,
                unlocked_profile_image_ids: Vec::new(),
                unlocked_face_item_ids: Vec::new(),
                unlocked_voice_ids: Vec::new(),
            })
        } else {
            None
        };

        let equipment = social.equipment.as_ref().map(|equipment| {
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
        });

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
            season,
            appearance,
            equipment,
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: None,
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

fn positive_i32(value: Option<i32>) -> Option<i32> {
    value.filter(|value| *value > 0)
}

fn clean_text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.to_owned())
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
    event.objective_id = Some(i64::from(objective.target_id.unwrap_or(map_key)));
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
        decode_entity(player, ActorState::Spawned, metadata, entities, &mut drafts)?;
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
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncNearEntities::decode(payload)?;
    let mut drafts = Vec::new();
    for entity in message.appeared {
        decode_entity(entity, ActorState::Spawned, metadata, entities, &mut drafts)?;
    }
    for disappeared in message.disappeared {
        let Some(uuid) = disappeared.uuid else {
            continue;
        };
        let state = entities.resolve(uuid, None)?;
        drafts.push(timeline_draft(
            metadata,
            TimelineEventKind::Actor(ActorEvent {
                actor: state.identity,
                state: ActorState::Despawned,
                entity_type_id: state.entity_type_id,
                kind: actor_kind(state.entity_type_id),
                monster_id: None,
                display_name: None,
                class_id: None,
                level: None,
            }),
        ));
    }
    Ok(drafts)
}

fn decode_sync_container(
    payload: &[u8],
    metadata: &DecodeMetadata,
    local_character: &mut Option<CharacterIdentity>,
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
        *local_character = Some(identity.clone());
        let professions = character.professions.as_ref();
        let current_profession_id =
            professions.and_then(|professions| positive_i32(professions.current_profession_id));
        let (combat_professions, active_skills, talents) = container_professions(professions);
        let battle_imagine_skills =
            container_battle_imagine_skills(professions, character.slots.as_ref());
        drafts.push(draft(
            metadata,
            EventSensitivity::PersonalGameplay,
            CanonicalEventDraftKind::CharacterProfileObserved {
                profile: Box::new(
                    CharacterProfilePatch {
                        character: identity,
                        display_name: base.and_then(|base| base.display_name.clone()),
                        display_id: base
                            .and_then(|base| base.display_id)
                            .map(|value| value.to_string()),
                        server_id: base
                            .and_then(|base| base.server_id)
                            .map(|value| value.to_string()),
                        class_id: current_profession_id
                            .or_else(|| base.and_then(|base| base.initial_class_id)),
                        specialization_id: None,
                        level: character
                            .role_level
                            .as_ref()
                            .and_then(|role| role.level)
                            .and_then(|value| u32::try_from(value).ok()),
                        progression: container_progression(character.role_level.as_ref()),
                        combat_power: base
                            .and_then(|base| base.combat_power)
                            .or_else(|| {
                                character.fight_power.as_ref().and_then(|power| power.total)
                            })
                            .map(i64::from),
                        combat_power_breakdown: container_fight_power(
                            character.fight_power.as_ref(),
                        ),
                        season_strength: None,
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
                        modules: container_modules(
                            character.item_package.as_ref(),
                            character.modules.as_ref(),
                        ),
                        owned_imagines: None,
                        battle_imagine_skills,
                        active_skills,
                        talents,
                        talent_progress: container_talent_progress(professions),
                        combat_professions,
                        life_professions: container_life_professions(
                            character.life_professions.as_ref(),
                        ),
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
                        ),
                        activity_progress: container_activity_progress(
                            character.challenge_dungeons.as_ref(),
                            character.master_mode_dungeons.as_ref(),
                            character.weekly_tower.as_ref(),
                        ),
                        season_medals: container_season_medals(character.season_medals.as_ref()),
                        season_cultivation: container_season_cultivation(
                            character.season_cultivation.as_ref(),
                        ),
                        reputations: container_reputations(character.reputations.as_ref()),
                        current_profession_project_id: character
                            .current_profession_project
                            .as_ref()
                            .and_then(|project| positive_i32(project.project_id)),
                        social_display: container_personal_zone(character.personal_zone.as_ref()),
                    }
                    .into_game_event()?,
                ),
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
) -> Option<CollectionSummary> {
    if fashion.is_none()
        && collection_book.is_none()
        && personal_zone.is_none()
        && rides.is_none()
        && emojis.is_none()
        && handbook.is_none()
        && vanity_pets.is_none()
        && fantasy_atlas.is_none()
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
    })
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

fn decode_sync_container_dirty(
    payload: &[u8],
    metadata: &DecodeMetadata,
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
                character,
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
                season: None,
                appearance,
                equipment: None,
                modules: None,
                owned_imagines: None,
                battle_imagine_skills: None,
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

    let mut drafts = Vec::with_capacity(2);
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
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    let message = schema::SyncNearDeltaInfo::decode(payload)?;
    let mut drafts = Vec::new();
    for delta in message.deltas {
        decode_aoi_delta(delta, None, metadata, entities, &mut drafts)?;
    }
    Ok(drafts)
}

fn decode_sync_to_me_delta(
    payload: &[u8],
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
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
            metadata,
            entities,
            &mut drafts,
        )?;
    }
    if let Some(uuid) = local_uuid {
        let actor = entities.resolve(uuid, Some(ENTITY_PLAYER))?.identity;
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
                }),
            ));
        }
    }
    Ok(drafts)
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
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    let Some(uuid) = entity.uuid else {
        return Ok(());
    };
    let state = entities.resolve(uuid, entity.entity_type)?;
    let attributes = entity.attributes.unwrap_or_default();
    let name = attr_text(&attributes, ATTR_NAME);
    let monster_id = attr_integer(&attributes, ATTR_MONSTER_ID).map(MonsterId);
    let class_id =
        attr_integer(&attributes, ATTR_CLASS_ID).and_then(|value| i32::try_from(value).ok());
    let level = attr_integer(&attributes, ATTR_LEVEL).and_then(|value| u32::try_from(value).ok());

    drafts.push(timeline_draft(
        metadata,
        TimelineEventKind::Actor(ActorEvent {
            actor: state.identity,
            state: actor_state,
            entity_type_id: state.entity_type_id,
            kind: actor_kind(state.entity_type_id),
            monster_id,
            display_name: name,
            class_id,
            level,
        }),
    ));
    emit_attributes(state.identity, &attributes, metadata, drafts);
    emit_position(state.identity, &attributes, metadata, drafts);
    Ok(())
}

fn decode_aoi_delta(
    delta: schema::AoiSyncDelta,
    entity_type_id: Option<i32>,
    metadata: &DecodeMetadata,
    entities: &mut EntityRegistry,
    drafts: &mut Vec<CanonicalEventDraft>,
) -> Result<(), ProtocolMessageError> {
    let Some(target_uuid) = delta.uuid else {
        return Ok(());
    };
    let target = entities.resolve(target_uuid, entity_type_id)?;
    if let Some(attributes) = &delta.attributes {
        drafts.push(timeline_draft(
            metadata,
            TimelineEventKind::Actor(ActorEvent {
                actor: target.identity,
                state: ActorState::Updated,
                entity_type_id: target.entity_type_id,
                kind: actor_kind(target.entity_type_id),
                monster_id: attr_integer(attributes, ATTR_MONSTER_ID).map(MonsterId),
                display_name: attr_text(attributes, ATTR_NAME),
                class_id: attr_integer(attributes, ATTR_CLASS_ID)
                    .and_then(|value| i32::try_from(value).ok()),
                level: attr_integer(attributes, ATTR_LEVEL)
                    .and_then(|value| u32::try_from(value).ok()),
            }),
        ));
        emit_attributes(target.identity, attributes, metadata, drafts);
        emit_position(target.identity, attributes, metadata, drafts);
    }

    let Some(effect) = delta.skill_effects else {
        return Ok(());
    };
    for damage in effect.damage {
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
        let ability = damage.owner_id.map(|value| AbilityId(i64::from(value)));

        if damage.damage_type == Some(DAMAGE_TYPE_HEAL) {
            drafts.push(timeline_draft(
                metadata,
                TimelineEventKind::Healing(HealingEvent {
                    source,
                    direct_source,
                    target: target.identity,
                    ability,
                    amount,
                    effective_amount: None,
                    overheal: None,
                    critical: damage.critical,
                    periodic: None,
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
                        critical: damage.critical,
                        lucky: damage
                            .type_flags
                            .map(|flags| flags & DAMAGE_FLAG_LUCKY != 0)
                            .or_else(|| damage.lucky_value.map(|_| true)),
                        blocked: damage
                            .type_flags
                            .map(|flags| flags & DAMAGE_FLAG_BLOCKED != 0),
                        periodic: None,
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

fn emit_attributes(
    actor: EntityRef,
    attributes: &schema::AttrCollection,
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
    if !decoded.is_empty() {
        drafts.push(timeline_draft(
            metadata,
            TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                actor,
                attributes: decoded,
            }),
        ));
    }
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
        .as_deref()?;
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

fn attr_position(attributes: &schema::AttrCollection) -> Option<schema::Position> {
    let raw = attributes
        .attributes
        .iter()
        .find(|attribute| attribute.id == Some(ATTR_POSITION))?
        .raw_data
        .as_deref()?;
    schema::Position::decode(raw).ok()
}

fn decode_attribute_value(id: i32, raw: &[u8]) -> Option<EntityAttributeValue> {
    match id {
        ATTR_NAME => decode_length_prefixed_text(raw).map(EntityAttributeValue::Text),
        ATTR_POSITION => {
            let position = schema::Position::decode(raw).ok()?;
            Some(EntityAttributeValue::Position {
                x: position.x?,
                y: position.y?,
                z: position.z?,
                facing_radians: position.facing_radians,
            })
        }
        ATTR_MONSTER_ID | ATTR_CLASS_ID | ATTR_LEVEL | ATTR_COMBAT_POWER | ATTR_SEASON_STRENGTH
        | ATTR_SCENE_ID | ATTR_SCENE_LINE => decode_varint(raw)
            .and_then(|value| i64::try_from(value).ok())
            .map(EntityAttributeValue::Integer),
        _ => None,
    }
}

fn decode_varint(bytes: &[u8]) -> Option<u64> {
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
    use super::*;
    use crate::{
        CompressionState, FragmentKind, MappingConfidence, PROTOCOL_PACK_SCHEMA_VERSION,
        PacketDirection, PacketEnvelope, PacketPayload, ProtocolPackDefinition, ProtocolPackRoute,
        ProtocolPackRouteDisposition, ProtocolPackTarget, RouteKey, RoutedMessage,
    };

    const WORLD_SERVICE: u64 = 0x6333_5342;
    const WORLD_LOGIN_SERVICE: u64 = 78_136_601;
    const SOCIAL_SERVICE: u64 = 625_772_963;

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
        avatar: Option<FullSocialAvatarInfo>,
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
    }

    #[derive(Clone, PartialEq, Message)]
    struct FullSocialAvatarInfo {
        #[prost(int32, optional, tag = "1")]
        avatar_id: Option<i32>,
        #[prost(string, optional, tag = "2")]
        profile_image_url: Option<String>,
        #[prost(string, optional, tag = "3")]
        half_body_image_url: Option<String>,
        #[prost(int32, optional, tag = "4")]
        business_card_style_id: Option<i32>,
        #[prost(int32, optional, tag = "5")]
        avatar_frame_id: Option<i32>,
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
            provenance: Vec::new(),
            routes: vec![
                route_for(WORLD_LOGIN_SERVICE, 3, DecoderKind::NotifyEnterWorldV1),
                route_for(SOCIAL_SERVICE, 1, DecoderKind::NotifySocialDataV1),
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
        let route = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            service_id,
            method_id,
        );
        CaptureRecord {
            sequence,
            observed_micros: sequence * 100,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 7,
                stream_id: 8,
                source: None,
                destination: None,
                direction: PacketDirection::ServerToClient,
                fragment: Some(FragmentKind::Notify),
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
    fn dungeon_snapshot_emits_exact_started_ended_and_objective_timeline() {
        let pack = pack();
        let mut runtime = runtime(&pack);

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
                ..
            }) if instance_id == "555"
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
    fn dungeon_flow_snapshot_preserves_raw_values_without_inventing_units_or_results() {
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
                kind: DungeonEventKind::Ended,
                flow: Some(DungeonFlowSnapshot {
                    result_id: Some(1),
                    ..
                }),
                ..
            })
        )));
        assert!(!ended.events.iter().any(|event| matches!(
            &event.event,
            rlogs_events::CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Completed | DungeonEventKind::Failed,
                ..
            })
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
                ..
            })
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
                    attributes: Some(schema::AttrCollection {
                        uuid: None,
                        attributes: vec![
                            text_attr(ATTR_NAME, "RLogs Tester"),
                            int_attr(ATTR_CLASS_ID, 8),
                            int_attr(ATTR_LEVEL, 60),
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
                attributes: None,
                skill_effects: Some(schema::SkillEffect {
                    uuid: Some(target_uuid),
                    damage: vec![
                        schema::DamageInfo {
                            damage_source: Some(1),
                            critical: Some(true),
                            damage_type: Some(0),
                            type_flags: Some(DAMAGE_FLAG_LUCKY),
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
                            damage_type: Some(DAMAGE_TYPE_HEAL),
                            value: Some(2_000),
                            attacker_uuid: Some(owner_uuid),
                            owner_id: Some(2_345),
                            ..schema::DamageInfo::default()
                        },
                    ],
                    total_damage: Some(10_000),
                }),
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
        assert!(
            timeline
                .iter()
                .any(|event| matches!(event, TimelineEventKind::Healing(_)))
        );
        assert!(timeline.iter().any(|event| matches!(
            event,
            TimelineEventKind::Life {
                state: LifeState::Died,
                ..
            }
        )));
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
                    battle_imagine_skills: std::collections::HashMap::new(),
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
                    avatar: Some(FullSocialAvatarInfo {
                        avatar_id: Some(42),
                        profile_image_url: Some("https://private.invalid/profile.png".into()),
                        half_body_image_url: Some("https://private.invalid/half-body.png".into()),
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
        assert!(!json.contains("private.invalid"));
        assert!(!json.contains("profile_image_url"));
        assert!(!json.contains("half_body_image_url"));

        let duplicate = runtime
            .process(&record_for(SOCIAL_SERVICE, 2, 1, payload))
            .unwrap();
        assert_eq!(duplicate.status, ProtocolDecodeStatus::Decoded);
        assert!(duplicate.events.is_empty());
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
