use std::collections::BTreeMap;

use prost::Message;
use rlogs_events::{
    AbilityId, ActorEvent, ActorId, ActorKind, ActorState, BoundaryReason, CanonicalEventDraft,
    CanonicalEventDraftKind, CharacterIdentity, CooldownEvent, DamageEvent, DamageFlags,
    DataGapEvent, DataGapKind, EntityAttribute, EntityAttributeEvent, EntityAttributeValue,
    EntityRef, EntityUuid, EventEnvelope, EventEnvelopeFactory, EventProvenance, EventSensitivity,
    EventTime, HealingEvent, LifeState, MonsterId, PositionEvent, RegionContext, RegionEvidence,
    RegionEvidenceKind, RegionIdentity, RunState, SceneId, TimelineEventKind, WorldContext,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::game_schema_v1 as schema;
use crate::{
    AllowedDataDomain, CaptureGapKind, CaptureRecord, CaptureRecordKind, CharacterProfilePatch,
    DecodeDisposition, GameBuild, ProhibitedDataClass, ProtocolPack,
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
const DAMAGE_TYPE_HEAL: i32 = 2;
const DAMAGE_FLAG_BLOCKED: i32 = 0b0010;
const DAMAGE_FLAG_LUCKY: i32 = 0b0100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecoderKind {
    EnterSceneV1,
    SyncNearEntitiesV1,
    SyncContainerDataV1,
    SyncNearDeltaV1,
    SyncToMeDeltaV1,
    NotifyReviveV1,
}

impl DecoderKind {
    pub const fn domain(self) -> AllowedDataDomain {
        match self {
            Self::EnterSceneV1 => AllowedDataDomain::WorldState,
            Self::SyncNearEntitiesV1 => AllowedDataDomain::ActorState,
            Self::SyncContainerDataV1 => AllowedDataDomain::CharacterProfile,
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
}

/// Stateful bridge from one exact protocol pack to canonical plugin events.
///
/// Entity UUIDs remain exact while compact actor IDs are assigned once per
/// session. The entity registry is bounded independently from packet buffers.
pub struct ProtocolRuntime<'a> {
    pack: &'a ProtocolPack,
    entities: EntityRegistry,
    envelopes: EventEnvelopeFactory,
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
            envelopes: EventEnvelopeFactory::new(session_id, region_context),
            config,
        })
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
                    vec![CanonicalEventDraft {
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
                    }],
                )
            }
            CaptureRecordKind::Packet(packet) => {
                let Some(routed) = packet.route else {
                    return Ok(ProtocolDecodeBatch {
                        capture_sequence: record.sequence,
                        status: ProtocolDecodeStatus::Unrouted,
                        events: Vec::new(),
                    });
                };

                match self.pack.disposition(Some(&routed.key)) {
                    DecodeDisposition::OpaqueLocalOnly => {
                        return Ok(ProtocolDecodeBatch {
                            capture_sequence: record.sequence,
                            status: ProtocolDecodeStatus::OpaqueLocalOnly,
                            events: Vec::new(),
                        });
                    }
                    DecodeDisposition::Prohibited(class) => {
                        return Ok(ProtocolDecodeBatch {
                            capture_sequence: record.sequence,
                            status: ProtocolDecodeStatus::Prohibited(class),
                            events: Vec::new(),
                        });
                    }
                    DecodeDisposition::Allowed(_) => {}
                }

                let Some(decoder) = self.pack.decoder(&routed.key) else {
                    return Ok(ProtocolDecodeBatch {
                        capture_sequence: record.sequence,
                        status: ProtocolDecodeStatus::OpaqueLocalOnly,
                        events: Vec::new(),
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
                        vec![draft],
                    );
                };

                let metadata = DecodeMetadata {
                    time: EventTime {
                        observed_micros: record.observed_micros,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(
                        record.sequence,
                        packet.connection_id,
                        packet.stream_id,
                    ),
                    region: self.envelopes.region().identity.clone(),
                };
                match decode_message(decoder, payload, &metadata, &mut self.entities) {
                    Ok(drafts) => (ProtocolDecodeStatus::Decoded, drafts),
                    Err(error) => (
                        ProtocolDecodeStatus::DecodeFailed,
                        vec![decode_gap_draft(
                            record,
                            packet.connection_id,
                            packet.stream_id,
                            &error.to_string(),
                        )],
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
        drafts: Vec<CanonicalEventDraft>,
    ) -> Result<ProtocolDecodeBatch, ProtocolRuntimeError> {
        if drafts.len() > self.config.max_events_per_packet {
            return Err(ProtocolRuntimeError::EventLimitExceeded {
                count: drafts.len(),
                limit: self.config.max_events_per_packet,
            });
        }
        let events = drafts
            .into_iter()
            .map(|draft| self.envelopes.emit(draft))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ProtocolDecodeBatch {
            capture_sequence,
            status,
            events,
        })
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
) -> Result<Vec<CanonicalEventDraft>, ProtocolMessageError> {
    match decoder {
        DecoderKind::EnterSceneV1 => decode_enter_scene(payload, metadata, entities),
        DecoderKind::SyncNearEntitiesV1 => decode_sync_near_entities(payload, metadata, entities),
        DecoderKind::SyncContainerDataV1 => decode_sync_container(payload, metadata),
        DecoderKind::SyncNearDeltaV1 => decode_sync_near_delta(payload, metadata, entities),
        DecoderKind::SyncToMeDeltaV1 => decode_sync_to_me_delta(payload, metadata, entities),
        DecoderKind::NotifyReviveV1 => decode_revive(payload, metadata, entities),
    }
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
        drafts.push(draft(
            metadata,
            EventSensitivity::PersonalGameplay,
            CanonicalEventDraftKind::CharacterProfileObserved {
                profile: Box::new(
                    CharacterProfilePatch {
                        character: CharacterIdentity {
                            region: metadata.region.clone(),
                            character_id: character_id.to_string(),
                        },
                        display_name: base.and_then(|base| base.display_name.clone()),
                        display_id: base
                            .and_then(|base| base.display_id)
                            .map(|value| value.to_string()),
                        server_id: base
                            .and_then(|base| base.server_id)
                            .map(|value| value.to_string()),
                        class_id: base.and_then(|base| base.initial_class_id),
                        specialization_id: None,
                        level: character
                            .role_level
                            .and_then(|role| role.level)
                            .and_then(|value| u32::try_from(value).ok()),
                        progression: None,
                        combat_power: base.and_then(|base| base.combat_power).map(i64::from),
                        season_strength: None,
                        season: None,
                        appearance: None,
                        equipment: None,
                        owned_imagines: None,
                        active_skills: None,
                        talents: None,
                        combat_professions: None,
                        life_professions: None,
                        cosmetics: None,
                        collection_summary: None,
                        social_display: None,
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

    fn encode<M: Message>(message: M) -> Vec<u8> {
        message.encode_to_vec()
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
        ProtocolPackRoute {
            route: RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                WORLD_SERVICE,
                method_id,
            ),
            service_name: "WorldNtf".into(),
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
                route(3, DecoderKind::EnterSceneV1),
                route(6, DecoderKind::SyncNearEntitiesV1),
                route(0x15, DecoderKind::SyncContainerDataV1),
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
        let route = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            WORLD_SERVICE,
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
                role_level: Some(schema::RoleLevel { level: Some(60) }),
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
