use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap},
};

use rlogs_events::{
    ActorKind, ActorOwnershipUpdate, CanonicalEvent, CastEvent, DamageEvent, EventEnvelope,
    HealingEvent, StatusEvent, StatusState, TimelineEventKind,
};
use serde::{Deserialize, Serialize};

use crate::{
    EffectFingerprintMatchKind, EffectFingerprintResolution, EffectSourceCandidate,
    localized_status_effect_name, resolve_effect_origin_fingerprint, status_effect_presentation,
};

pub const RDPS_AUDIT_SCHEMA_VERSION: u16 = 7;
const MAXIMUM_ACTIVE_STATUS_WINDOWS: usize = 100_000;
const ORIGIN_CORRELATION_WINDOW_MICROS: u64 = 2_000_000;
const MAXIMUM_PROVIDER_RECIPIENT_EXAMPLES_PER_CLASS: usize = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdpsAuditDamageTotals {
    pub events: u64,
    pub damage: i64,
}

impl RdpsAuditDamageTotals {
    fn observe(&mut self, amount: i64) {
        self.events = self.events.saturating_add(1);
        self.damage = self.damage.saturating_add(amount.max(0));
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RdpsAuditProviderRecipientMatrix {
    /// Legacy raw cross-actor player-to-player total. This remains the sum of
    /// same-owner proxy and genuinely external player windows so older audit
    /// consumers retain their conservation invariant.
    pub resolved_player_to_player: u64,
    /// A summon, Imagine, pet, projectile, or other proxy affected the player
    /// that owns it. This is not transferable rDPS evidence.
    pub resolved_same_owner_player_to_player: u64,
    /// Provider and recipient resolve to two distinct player owners. Only this
    /// subdivision proves an externally transferable lifecycle.
    pub resolved_external_player_to_player: u64,
    pub resolved_player_to_monster: u64,
    pub resolved_player_to_other: u64,
    pub non_player_to_player: u64,
    pub non_player_to_monster: u64,
    pub non_player_to_other: u64,
    pub unresolved_to_player: u64,
    pub unresolved_to_monster: u64,
    pub unresolved_to_other: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdpsEffectAudit {
    pub effect_id: i64,
    pub localized_name: Option<String>,
    pub technical_name: Option<String>,
    pub presentation_resolution: Option<String>,
    pub status_events: u64,
    pub window_count: u64,
    pub cross_actor_window_count: u64,
    pub source_missing_window_count: u64,
    pub source_player_window_count: u64,
    pub source_resolved_player_window_count: u64,
    pub source_owner_resolved_window_count: u64,
    pub target_player_window_count: u64,
    pub target_monster_window_count: u64,
    pub cross_actor_provider_recipient_windows: RdpsAuditProviderRecipientMatrix,
    /// Bounded, deterministic examples retained for exact lifecycle proof.
    /// These are evidence rows, not inferred relationships or runtime rules.
    pub provider_recipient_examples: Vec<RdpsAuditProviderRecipientExample>,
    pub applied: u64,
    pub refreshed: u64,
    pub stacked: u64,
    pub consumed: u64,
    pub removed: u64,
    pub minimum_stacks: Option<u32>,
    pub maximum_stacks: Option<u32>,
    pub distinct_provider_entities: usize,
    pub distinct_resolved_provider_entities: usize,
    pub distinct_recipient_entities: usize,
    pub cross_actor_recipient_outgoing: RdpsAuditDamageTotals,
    pub cross_actor_recipient_incoming: RdpsAuditDamageTotals,
    pub packet_origin_observation_count: u64,
    pub packet_origins: Vec<RdpsAuditPacketOrigin>,
    pub origin_observation_count: u64,
    pub uncorrelated_origin_observation_count: u64,
    pub ambiguous_origin_observation_count: u64,
    pub originating_abilities: Vec<RdpsAuditAbilityCorrelation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsAuditProviderClass {
    ResolvedPlayer,
    NonPlayer,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsAuditRecipientClass {
    Player,
    Monster,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsAuditProviderRecipientExampleClass {
    ExternalPlayerToPlayer,
    SameOwnerPlayerToPlayer,
    PlayerToMonster,
    PlayerToOther,
    NonPlayerToPlayer,
    NonPlayerToMonster,
    NonPlayerToOther,
    UnresolvedToPlayer,
    UnresolvedToMonster,
    UnresolvedToOther,
    SameActorOrEntity,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RdpsAuditProviderRecipientExample {
    pub class: RdpsAuditProviderRecipientExampleClass,
    pub raw_source_actor_id: Option<u64>,
    pub raw_target_actor_id: u64,
    pub raw_source_entity_uuid: Option<i64>,
    pub resolved_source_entity_uuid: Option<i64>,
    pub raw_target_entity_uuid: i64,
    pub resolved_target_entity_uuid: i64,
    pub provider_class: RdpsAuditProviderClass,
    pub recipient_class: RdpsAuditRecipientClass,
    pub cross_actor: bool,
    pub same_resolved_owner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdpsAuditPacketOrigin {
    pub source_type_id: i32,
    pub source_config_id: i64,
    pub observation_count: u64,
    /// Whether this exact packet tuple or only the effect fallback matched the
    /// generated fingerprint catalog.
    #[serde(default)]
    pub fingerprint_match_kind: EffectFingerprintMatchKind,
    /// Certainty that the terminal formula endpoint has been identified.
    #[serde(default)]
    pub endpoint_resolution: EffectFingerprintResolution,
    /// Separate certainty that the equipped skill, factor, item, or node which
    /// owns the endpoint has been identified.
    #[serde(default)]
    pub owner_resolution: EffectFingerprintResolution,
    #[serde(default)]
    pub candidate_sources: Vec<EffectSourceCandidate>,
    /// Formula endpoints retained when the owning source remains unresolved.
    #[serde(default)]
    pub unresolved_terminal_ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsAuditActionKind {
    Cast,
    Damage,
    Healing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdpsAuditAbilityCorrelation {
    pub ability_id: i64,
    /// Populated only for the exact skill-level IDs carried by cast packets.
    pub base_skill_id: Option<i64>,
    pub action_kind: RdpsAuditActionKind,
    pub observation_count: u64,
    pub same_target_count: u64,
    pub preceding_status_count: u64,
    pub following_status_count: u64,
    pub simultaneous_count: u64,
    pub owner_resolved_count: u64,
    pub minimum_absolute_delay_micros: u64,
    pub maximum_absolute_delay_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdpsAuditReport {
    pub schema_version: u16,
    pub session_id: String,
    pub first_observed_micros: Option<u64>,
    pub last_observed_micros: Option<u64>,
    pub damage_events: u64,
    pub effects: Vec<RdpsEffectAudit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StatusWindowKey {
    effect_id: i64,
    instance_id: Option<i64>,
    source_actor_id: Option<u64>,
    target_actor_id: u64,
}

#[derive(Debug, Clone)]
struct ActiveStatusWindow {
    key: StatusWindowKey,
    source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    expiration_micros: Option<u64>,
    expiration_generation: u64,
    cross_actor: bool,
}

#[derive(Debug, Default)]
struct EffectAccumulator {
    status_events: u64,
    window_count: u64,
    cross_actor_window_count: u64,
    source_missing_window_count: u64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    minimum_stacks: Option<u32>,
    maximum_stacks: Option<u32>,
    provider_entities: BTreeSet<i64>,
    recipient_entities: BTreeSet<i64>,
    windows: Vec<WindowObservation>,
    cross_actor_recipient_outgoing: RdpsAuditDamageTotals,
    cross_actor_recipient_incoming: RdpsAuditDamageTotals,
    packet_origins: BTreeMap<(i32, i64), u64>,
}

#[derive(Debug, Clone, Copy)]
struct WindowObservation {
    source_actor_id: Option<u64>,
    target_actor_id: u64,
    source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    cross_actor: bool,
}

#[derive(Debug, Clone, Copy)]
struct ActionObservation {
    observed_micros: u64,
    sequence: u64,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: Option<i64>,
    ability_id: i64,
    kind: RdpsAuditActionKind,
}

#[derive(Debug, Clone, Copy)]
struct StatusOriginObservation {
    effect_id: i64,
    observed_micros: u64,
    sequence: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
}

#[derive(Debug, Default)]
struct AbilityCorrelationAccumulator {
    observation_count: u64,
    same_target_count: u64,
    preceding_status_count: u64,
    following_status_count: u64,
    simultaneous_count: u64,
    owner_resolved_count: u64,
    minimum_absolute_delay_micros: Option<u64>,
    maximum_absolute_delay_micros: u64,
}

#[derive(Debug, Default)]
struct EffectOriginCorrelation {
    total: u64,
    uncorrelated: u64,
    ambiguous: u64,
    abilities: Vec<RdpsAuditAbilityCorrelation>,
}

#[derive(Debug, thiserror::Error)]
pub enum RdpsAuditError {
    #[error("rDPS audit received multiple sessions: {expected} and {actual}")]
    MixedSessions { expected: String, actual: String },
    #[error("rDPS audit event sequence moved backward from {previous} to {actual}")]
    SequenceMovedBackward { previous: u64, actual: u64 },
    #[error("rDPS audit exceeded its {limit} active-window safety limit")]
    ActiveWindowLimitExceeded { limit: usize },
    #[error("rDPS audit presentation lookup failed: {0}")]
    Presentation(String),
}

#[derive(Debug, Default)]
pub struct RdpsEffectAuditAnalyzer {
    session_id: Option<String>,
    last_sequence: Option<u64>,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    damage_events: u64,
    actor_kinds: HashMap<u64, ActorKind>,
    actor_kinds_by_entity: HashMap<i64, ActorKind>,
    owner_by_direct_entity: HashMap<i64, i64>,
    actions: Vec<ActionObservation>,
    status_origins: Vec<StatusOriginObservation>,
    effects: BTreeMap<i64, EffectAccumulator>,
    active_by_key: HashMap<StatusWindowKey, u64>,
    active: HashMap<u64, ActiveStatusWindow>,
    outgoing_by_actor: HashMap<u64, BTreeSet<u64>>,
    incoming_by_actor: HashMap<u64, BTreeSet<u64>>,
    expirations: BinaryHeap<Reverse<(u64, u64, u64)>>,
    next_window_id: u64,
}

impl RdpsEffectAuditAnalyzer {
    pub fn new() -> Self {
        Self {
            next_window_id: 1,
            ..Self::default()
        }
    }

    pub fn observe(&mut self, envelope: &EventEnvelope) -> Result<(), RdpsAuditError> {
        self.validate(envelope)?;
        self.expire_before(envelope.time.observed_micros);
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return Ok(());
        };
        match &timeline.kind {
            TimelineEventKind::Actor(actor) => {
                self.actor_kinds.insert(actor.actor.actor_id.0, actor.kind);
                self.actor_kinds_by_entity
                    .insert(actor.actor.entity_uuid.0, actor.kind);
            }
            TimelineEventKind::EntityAttributes(attributes) => {
                if let Some(ActorOwnershipUpdate::Confirmed { owner_entity_uuid }) =
                    attributes.ownership
                {
                    // Audit reports span the whole session. Retain the first
                    // packet-confirmed relation as historical proof even if a
                    // later despawn emits Cleared; entity UUID reuse or a
                    // conflicting owner must not silently rewrite old windows.
                    self.owner_by_direct_entity
                        .entry(attributes.actor.entity_uuid.0)
                        .or_insert(owner_entity_uuid.0);
                }
            }
            TimelineEventKind::Status(status) => {
                self.observe_status(envelope.time.observed_micros, envelope.sequence, status)?;
            }
            TimelineEventKind::Cast(cast) => {
                self.observe_cast(envelope.time.observed_micros, envelope.sequence, cast)
            }
            TimelineEventKind::Damage(damage) => {
                self.observe_damage(envelope.time.observed_micros, envelope.sequence, damage)
            }
            TimelineEventKind::Healing(healing) => {
                self.observe_healing(envelope.time.observed_micros, envelope.sequence, healing)
            }
            _ => {}
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<RdpsAuditReport, RdpsAuditError> {
        let active_ids = self.active.keys().copied().collect::<Vec<_>>();
        for window_id in active_ids {
            self.close_window(window_id);
        }
        let origin_correlations = self.correlate_origins();
        let mut effects = Vec::with_capacity(self.effects.len());
        for (effect_id, accumulator) in self.effects {
            let mut source_player_window_count = 0;
            let mut source_resolved_player_window_count = 0;
            let mut source_owner_resolved_window_count = 0;
            let mut target_player_window_count = 0;
            let mut target_monster_window_count = 0;
            let mut cross_actor_provider_recipient_windows =
                RdpsAuditProviderRecipientMatrix::default();
            let mut provider_recipient_examples = BTreeMap::<
                RdpsAuditProviderRecipientExampleClass,
                BTreeSet<RdpsAuditProviderRecipientExample>,
            >::new();
            for window in accumulator.windows {
                let raw_source_kind = window
                    .source_actor_id
                    .and_then(|actor_id| self.actor_kinds.get(&actor_id))
                    .copied();
                let raw_source_is_player = raw_source_kind == Some(ActorKind::Player);
                if raw_source_is_player {
                    source_player_window_count += 1;
                }
                let resolved_source = window.source_entity_uuid.map(|source| {
                    self.owner_by_direct_entity
                        .get(&source)
                        .copied()
                        .unwrap_or(source)
                });
                if window
                    .source_entity_uuid
                    .zip(resolved_source)
                    .is_some_and(|(raw, resolved)| raw != resolved)
                {
                    source_owner_resolved_window_count += 1;
                }
                let resolved_source_kind = resolved_source
                    .and_then(|source| self.actor_kinds_by_entity.get(&source))
                    .copied()
                    .or(raw_source_kind)
                    .or_else(|| {
                        window
                            .source_entity_uuid
                            .and_then(|source| self.actor_kinds_by_entity.get(&source))
                            .copied()
                    });
                let provider_class = match resolved_source_kind {
                    Some(ActorKind::Player) => RdpsAuditProviderClass::ResolvedPlayer,
                    Some(_) => RdpsAuditProviderClass::NonPlayer,
                    None => RdpsAuditProviderClass::Unresolved,
                };
                if provider_class == RdpsAuditProviderClass::ResolvedPlayer {
                    source_resolved_player_window_count += 1;
                }
                let raw_target_kind = self
                    .actor_kinds
                    .get(&window.target_actor_id)
                    .copied()
                    .or_else(|| {
                        self.actor_kinds_by_entity
                            .get(&window.target_entity_uuid)
                            .copied()
                    });
                let resolved_target = self
                    .owner_by_direct_entity
                    .get(&window.target_entity_uuid)
                    .copied()
                    .unwrap_or(window.target_entity_uuid);
                let target_kind = self
                    .actor_kinds_by_entity
                    .get(&resolved_target)
                    .copied()
                    .or(raw_target_kind);
                let recipient_class = match target_kind {
                    Some(ActorKind::Player) => {
                        target_player_window_count += 1;
                        RdpsAuditRecipientClass::Player
                    }
                    Some(ActorKind::Monster) => {
                        target_monster_window_count += 1;
                        RdpsAuditRecipientClass::Monster
                    }
                    _ => RdpsAuditRecipientClass::Other,
                };
                let same_resolved_owner = resolved_source == Some(resolved_target);
                let example_class = if !window.cross_actor {
                    RdpsAuditProviderRecipientExampleClass::SameActorOrEntity
                } else {
                    match (provider_class, recipient_class) {
                        (
                            RdpsAuditProviderClass::ResolvedPlayer,
                            RdpsAuditRecipientClass::Player,
                        ) if same_resolved_owner => {
                            RdpsAuditProviderRecipientExampleClass::SameOwnerPlayerToPlayer
                        }
                        (
                            RdpsAuditProviderClass::ResolvedPlayer,
                            RdpsAuditRecipientClass::Player,
                        ) => RdpsAuditProviderRecipientExampleClass::ExternalPlayerToPlayer,
                        (
                            RdpsAuditProviderClass::ResolvedPlayer,
                            RdpsAuditRecipientClass::Monster,
                        ) => RdpsAuditProviderRecipientExampleClass::PlayerToMonster,
                        (
                            RdpsAuditProviderClass::ResolvedPlayer,
                            RdpsAuditRecipientClass::Other,
                        ) => RdpsAuditProviderRecipientExampleClass::PlayerToOther,
                        (RdpsAuditProviderClass::NonPlayer, RdpsAuditRecipientClass::Player) => {
                            RdpsAuditProviderRecipientExampleClass::NonPlayerToPlayer
                        }
                        (RdpsAuditProviderClass::NonPlayer, RdpsAuditRecipientClass::Monster) => {
                            RdpsAuditProviderRecipientExampleClass::NonPlayerToMonster
                        }
                        (RdpsAuditProviderClass::NonPlayer, RdpsAuditRecipientClass::Other) => {
                            RdpsAuditProviderRecipientExampleClass::NonPlayerToOther
                        }
                        (RdpsAuditProviderClass::Unresolved, RdpsAuditRecipientClass::Player) => {
                            RdpsAuditProviderRecipientExampleClass::UnresolvedToPlayer
                        }
                        (RdpsAuditProviderClass::Unresolved, RdpsAuditRecipientClass::Monster) => {
                            RdpsAuditProviderRecipientExampleClass::UnresolvedToMonster
                        }
                        (RdpsAuditProviderClass::Unresolved, RdpsAuditRecipientClass::Other) => {
                            RdpsAuditProviderRecipientExampleClass::UnresolvedToOther
                        }
                    }
                };
                provider_recipient_examples
                    .entry(example_class)
                    .or_default()
                    .insert(RdpsAuditProviderRecipientExample {
                        class: example_class,
                        raw_source_actor_id: window.source_actor_id,
                        raw_target_actor_id: window.target_actor_id,
                        raw_source_entity_uuid: window.source_entity_uuid,
                        resolved_source_entity_uuid: resolved_source,
                        raw_target_entity_uuid: window.target_entity_uuid,
                        resolved_target_entity_uuid: resolved_target,
                        provider_class,
                        recipient_class,
                        cross_actor: window.cross_actor,
                        same_resolved_owner,
                    });
                if window.cross_actor {
                    match (provider_class, recipient_class) {
                        (
                            RdpsAuditProviderClass::ResolvedPlayer,
                            RdpsAuditRecipientClass::Player,
                        ) => {
                            cross_actor_provider_recipient_windows.resolved_player_to_player += 1;
                            if same_resolved_owner {
                                cross_actor_provider_recipient_windows
                                    .resolved_same_owner_player_to_player += 1;
                            } else {
                                cross_actor_provider_recipient_windows
                                    .resolved_external_player_to_player += 1;
                            }
                        }
                        (
                            RdpsAuditProviderClass::ResolvedPlayer,
                            RdpsAuditRecipientClass::Monster,
                        ) => {
                            cross_actor_provider_recipient_windows.resolved_player_to_monster += 1;
                        }
                        (
                            RdpsAuditProviderClass::ResolvedPlayer,
                            RdpsAuditRecipientClass::Other,
                        ) => {
                            cross_actor_provider_recipient_windows.resolved_player_to_other += 1;
                        }
                        (RdpsAuditProviderClass::NonPlayer, RdpsAuditRecipientClass::Player) => {
                            cross_actor_provider_recipient_windows.non_player_to_player += 1;
                        }
                        (RdpsAuditProviderClass::NonPlayer, RdpsAuditRecipientClass::Monster) => {
                            cross_actor_provider_recipient_windows.non_player_to_monster += 1;
                        }
                        (RdpsAuditProviderClass::NonPlayer, RdpsAuditRecipientClass::Other) => {
                            cross_actor_provider_recipient_windows.non_player_to_other += 1;
                        }
                        (RdpsAuditProviderClass::Unresolved, RdpsAuditRecipientClass::Player) => {
                            cross_actor_provider_recipient_windows.unresolved_to_player += 1;
                        }
                        (RdpsAuditProviderClass::Unresolved, RdpsAuditRecipientClass::Monster) => {
                            cross_actor_provider_recipient_windows.unresolved_to_monster += 1;
                        }
                        (RdpsAuditProviderClass::Unresolved, RdpsAuditRecipientClass::Other) => {
                            cross_actor_provider_recipient_windows.unresolved_to_other += 1;
                        }
                    }
                }
            }
            let presentation =
                status_effect_presentation(effect_id).map_err(RdpsAuditError::Presentation)?;
            let correlation = origin_correlations.get(&effect_id);
            let resolved_provider_entities = accumulator
                .provider_entities
                .iter()
                .map(|source| {
                    self.owner_by_direct_entity
                        .get(source)
                        .copied()
                        .unwrap_or(*source)
                })
                .collect::<BTreeSet<_>>();
            effects.push(RdpsEffectAudit {
                effect_id,
                localized_name: localized_status_effect_name(effect_id, "en-US")
                    .map_err(RdpsAuditError::Presentation)?
                    .map(str::to_owned),
                technical_name: presentation.and_then(|value| value.technical_name.clone()),
                presentation_resolution: presentation.map(|value| value.resolution.clone()),
                status_events: accumulator.status_events,
                window_count: accumulator.window_count,
                cross_actor_window_count: accumulator.cross_actor_window_count,
                source_missing_window_count: accumulator.source_missing_window_count,
                source_player_window_count,
                source_resolved_player_window_count,
                source_owner_resolved_window_count,
                target_player_window_count,
                target_monster_window_count,
                cross_actor_provider_recipient_windows,
                provider_recipient_examples: provider_recipient_examples
                    .into_values()
                    .flat_map(|examples| {
                        examples
                            .into_iter()
                            .take(MAXIMUM_PROVIDER_RECIPIENT_EXAMPLES_PER_CLASS)
                    })
                    .collect(),
                applied: accumulator.applied,
                refreshed: accumulator.refreshed,
                stacked: accumulator.stacked,
                consumed: accumulator.consumed,
                removed: accumulator.removed,
                minimum_stacks: accumulator.minimum_stacks,
                maximum_stacks: accumulator.maximum_stacks,
                distinct_provider_entities: accumulator.provider_entities.len(),
                distinct_resolved_provider_entities: resolved_provider_entities.len(),
                distinct_recipient_entities: accumulator.recipient_entities.len(),
                cross_actor_recipient_outgoing: accumulator.cross_actor_recipient_outgoing,
                cross_actor_recipient_incoming: accumulator.cross_actor_recipient_incoming,
                packet_origin_observation_count: accumulator.packet_origins.values().copied().sum(),
                packet_origins: accumulator
                    .packet_origins
                    .into_iter()
                    .map(|((source_type_id, source_config_id), observation_count)| {
                        let fingerprint = resolve_effect_origin_fingerprint(
                            effect_id,
                            Some((source_type_id, source_config_id)),
                            None,
                            0,
                            StatusState::Applied,
                        );
                        RdpsAuditPacketOrigin {
                            source_type_id,
                            source_config_id,
                            observation_count,
                            fingerprint_match_kind: fingerprint.match_kind,
                            endpoint_resolution: fingerprint.endpoint_resolution,
                            owner_resolution: fingerprint.owner_resolution,
                            candidate_sources: fingerprint.candidate_sources.to_vec(),
                            unresolved_terminal_ids: fingerprint.unresolved_terminal_ids.to_vec(),
                        }
                    })
                    .collect(),
                origin_observation_count: correlation.map_or(0, |value| value.total),
                uncorrelated_origin_observation_count: correlation
                    .map_or(0, |value| value.uncorrelated),
                ambiguous_origin_observation_count: correlation.map_or(0, |value| value.ambiguous),
                originating_abilities: correlation
                    .map(|value| value.abilities.clone())
                    .unwrap_or_default(),
            });
        }
        Ok(RdpsAuditReport {
            schema_version: RDPS_AUDIT_SCHEMA_VERSION,
            session_id: self.session_id.unwrap_or_default(),
            first_observed_micros: self.first_observed_micros,
            last_observed_micros: self.last_observed_micros,
            damage_events: self.damage_events,
            effects,
        })
    }

    fn validate(&mut self, envelope: &EventEnvelope) -> Result<(), RdpsAuditError> {
        if let Some(expected) = &self.session_id {
            if expected != &envelope.session_id {
                return Err(RdpsAuditError::MixedSessions {
                    expected: expected.clone(),
                    actual: envelope.session_id.clone(),
                });
            }
        } else {
            self.session_id = Some(envelope.session_id.clone());
        }
        if let Some(previous) = self.last_sequence
            && envelope.sequence < previous
        {
            return Err(RdpsAuditError::SequenceMovedBackward {
                previous,
                actual: envelope.sequence,
            });
        }
        self.last_sequence = Some(envelope.sequence);
        self.first_observed_micros
            .get_or_insert(envelope.time.observed_micros);
        self.last_observed_micros = Some(envelope.time.observed_micros);
        Ok(())
    }

    fn observe_status(
        &mut self,
        observed_micros: u64,
        sequence: u64,
        status: &StatusEvent,
    ) -> Result<(), RdpsAuditError> {
        let key = StatusWindowKey {
            effect_id: status.effect.0,
            instance_id: status.instance_id.map(|value| value.0),
            source_actor_id: status.source.map(|source| source.actor_id.0),
            target_actor_id: status.target.actor_id.0,
        };
        let accumulator = self.effects.entry(status.effect.0).or_default();
        accumulator.status_events = accumulator.status_events.saturating_add(1);
        match status.state {
            StatusState::Applied => accumulator.applied = accumulator.applied.saturating_add(1),
            StatusState::Refreshed => {
                accumulator.refreshed = accumulator.refreshed.saturating_add(1)
            }
            StatusState::Stacked => accumulator.stacked = accumulator.stacked.saturating_add(1),
            StatusState::Consumed => accumulator.consumed = accumulator.consumed.saturating_add(1),
            StatusState::Removed => accumulator.removed = accumulator.removed.saturating_add(1),
        }
        if let Some(stacks) = status.stacks {
            accumulator.minimum_stacks = Some(
                accumulator
                    .minimum_stacks
                    .map_or(stacks, |current| current.min(stacks)),
            );
            accumulator.maximum_stacks = Some(
                accumulator
                    .maximum_stacks
                    .map_or(stacks, |current| current.max(stacks)),
            );
        }

        if matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) && let Some(origin) = status.origin
        {
            let observations = accumulator
                .packet_origins
                .entry((origin.source_type_id, origin.source_config_id))
                .or_default();
            *observations = observations.saturating_add(1);
        }

        if matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) && let Some(source) = status.source
        {
            self.status_origins.push(StatusOriginObservation {
                effect_id: status.effect.0,
                observed_micros,
                sequence,
                source_entity_uuid: source.entity_uuid.0,
                target_entity_uuid: status.target.entity_uuid.0,
            });
        }

        if matches!(status.state, StatusState::Removed)
            || matches!(status.state, StatusState::Consumed) && status.stacks == Some(0)
        {
            if let Some(window_id) = self.active_by_key.get(&key).copied() {
                self.close_window(window_id);
            }
            return Ok(());
        }

        if let Some(window_id) = self.active_by_key.get(&key).copied() {
            self.refresh_expiration(window_id, observed_micros, status.duration_millis);
            return Ok(());
        }
        if self.active.len() >= MAXIMUM_ACTIVE_STATUS_WINDOWS {
            return Err(RdpsAuditError::ActiveWindowLimitExceeded {
                limit: MAXIMUM_ACTIVE_STATUS_WINDOWS,
            });
        }
        let source_entity_uuid = status.source.map(|source| source.entity_uuid.0);
        let target_entity_uuid = status.target.entity_uuid.0;
        let cross_actor = source_entity_uuid.is_some_and(|source| source != target_entity_uuid);
        let accumulator = self.effects.entry(status.effect.0).or_default();
        accumulator.window_count = accumulator.window_count.saturating_add(1);
        accumulator.cross_actor_window_count = accumulator
            .cross_actor_window_count
            .saturating_add(u64::from(cross_actor));
        accumulator.source_missing_window_count = accumulator
            .source_missing_window_count
            .saturating_add(u64::from(source_entity_uuid.is_none()));
        if let Some(source) = source_entity_uuid {
            accumulator.provider_entities.insert(source);
        }
        accumulator.recipient_entities.insert(target_entity_uuid);
        accumulator.windows.push(WindowObservation {
            source_actor_id: key.source_actor_id,
            target_actor_id: key.target_actor_id,
            source_entity_uuid,
            target_entity_uuid,
            cross_actor,
        });

        let window_id = self.next_window_id;
        self.next_window_id = self.next_window_id.saturating_add(1);
        let window = ActiveStatusWindow {
            key,
            source_entity_uuid,
            target_entity_uuid,
            expiration_micros: None,
            expiration_generation: 0,
            cross_actor,
        };
        self.active_by_key.insert(key, window_id);
        self.active.insert(window_id, window);
        self.outgoing_by_actor
            .entry(key.target_actor_id)
            .or_default()
            .insert(window_id);
        self.incoming_by_actor
            .entry(key.target_actor_id)
            .or_default()
            .insert(window_id);
        self.refresh_expiration(window_id, observed_micros, status.duration_millis);
        Ok(())
    }

    fn observe_cast(&mut self, observed_micros: u64, sequence: u64, cast: &CastEvent) {
        self.actions.push(ActionObservation {
            observed_micros,
            sequence,
            source_entity_uuid: cast.source.entity_uuid.0,
            direct_source_entity_uuid: None,
            target_entity_uuid: cast.target.map(|target| target.entity_uuid.0),
            ability_id: cast.ability.0,
            kind: RdpsAuditActionKind::Cast,
        });
    }

    fn observe_damage(&mut self, observed_micros: u64, sequence: u64, damage: &DamageEvent) {
        self.damage_events = self.damage_events.saturating_add(1);
        self.observe_owner(
            damage.source.entity_uuid.0,
            damage.direct_source.map(|source| source.entity_uuid.0),
        );
        if let Some(ability) = damage.ability {
            self.actions.push(ActionObservation {
                observed_micros,
                sequence,
                source_entity_uuid: damage.source.entity_uuid.0,
                direct_source_entity_uuid: damage.direct_source.map(|source| source.entity_uuid.0),
                target_entity_uuid: Some(damage.target.entity_uuid.0),
                ability_id: ability.0,
                kind: RdpsAuditActionKind::Damage,
            });
        }
        let amount = damage.amount.max(0);
        let outgoing = self
            .outgoing_by_actor
            .get(&damage.source.actor_id.0)
            .cloned()
            .unwrap_or_default();
        for window_id in outgoing {
            let Some(window) = self.active.get(&window_id) else {
                continue;
            };
            if window.cross_actor {
                self.effects
                    .entry(window.key.effect_id)
                    .or_default()
                    .cross_actor_recipient_outgoing
                    .observe(amount);
            }
        }
        let incoming = self
            .incoming_by_actor
            .get(&damage.target.actor_id.0)
            .cloned()
            .unwrap_or_default();
        for window_id in incoming {
            let Some(window) = self.active.get(&window_id) else {
                continue;
            };
            if window.cross_actor {
                self.effects
                    .entry(window.key.effect_id)
                    .or_default()
                    .cross_actor_recipient_incoming
                    .observe(amount);
            }
        }
    }

    fn observe_healing(&mut self, observed_micros: u64, sequence: u64, healing: &HealingEvent) {
        self.observe_owner(
            healing.source.entity_uuid.0,
            healing.direct_source.map(|source| source.entity_uuid.0),
        );
        if let Some(ability) = healing.ability {
            self.actions.push(ActionObservation {
                observed_micros,
                sequence,
                source_entity_uuid: healing.source.entity_uuid.0,
                direct_source_entity_uuid: healing.direct_source.map(|source| source.entity_uuid.0),
                target_entity_uuid: Some(healing.target.entity_uuid.0),
                ability_id: ability.0,
                kind: RdpsAuditActionKind::Healing,
            });
        }
    }

    fn observe_owner(&mut self, owner: i64, direct_source: Option<i64>) {
        if let Some(direct_source) = direct_source.filter(|direct| *direct != owner) {
            self.owner_by_direct_entity
                .entry(direct_source)
                .or_insert(owner);
        }
    }

    fn correlate_origins(&self) -> BTreeMap<i64, EffectOriginCorrelation> {
        let mut action_indexes = HashMap::<i64, Vec<usize>>::new();
        for (index, action) in self.actions.iter().enumerate() {
            action_indexes
                .entry(action.source_entity_uuid)
                .or_default()
                .push(index);
            if let Some(direct) = action
                .direct_source_entity_uuid
                .filter(|direct| *direct != action.source_entity_uuid)
            {
                action_indexes.entry(direct).or_default().push(index);
            }
        }

        let mut totals = BTreeMap::<i64, (u64, u64, u64)>::new();
        let mut correlations =
            BTreeMap::<(i64, RdpsAuditActionKind, i64), AbilityCorrelationAccumulator>::new();
        for status in &self.status_origins {
            let totals = totals.entry(status.effect_id).or_default();
            totals.0 = totals.0.saturating_add(1);
            let resolved_owner = self
                .owner_by_direct_entity
                .get(&status.source_entity_uuid)
                .copied();
            let mut provider_entities = BTreeSet::from([status.source_entity_uuid]);
            if let Some(owner) = resolved_owner {
                provider_entities.insert(owner);
            }
            let lower = status
                .observed_micros
                .saturating_sub(ORIGIN_CORRELATION_WINDOW_MICROS);
            let upper = status
                .observed_micros
                .saturating_add(ORIGIN_CORRELATION_WINDOW_MICROS);
            let mut candidate_indexes = BTreeSet::new();
            for provider in provider_entities {
                let Some(indexes) = action_indexes.get(&provider) else {
                    continue;
                };
                let start =
                    indexes.partition_point(|index| self.actions[*index].observed_micros < lower);
                for index in &indexes[start..] {
                    if self.actions[*index].observed_micros > upper {
                        break;
                    }
                    candidate_indexes.insert(*index);
                }
            }
            if candidate_indexes.is_empty() {
                totals.1 = totals.1.saturating_add(1);
                continue;
            }

            // Keep the nearest evidence independently for casts, damage, and
            // healing. This exposes competing candidates instead of silently
            // deciding that the nearest packet must be the source skill.
            let mut nearest = BTreeMap::<RdpsAuditActionKind, (u64, Vec<usize>)>::new();
            for index in candidate_indexes {
                let action = self.actions[index];
                let delay = action.observed_micros.abs_diff(status.observed_micros);
                let entry = nearest.entry(action.kind).or_insert((delay, Vec::new()));
                match delay.cmp(&entry.0) {
                    std::cmp::Ordering::Less => *entry = (delay, vec![index]),
                    std::cmp::Ordering::Equal => entry.1.push(index),
                    std::cmp::Ordering::Greater => {}
                }
            }
            let ambiguous = nearest.values().any(|(_, indexes)| {
                indexes
                    .iter()
                    .map(|index| self.actions[*index].ability_id)
                    .collect::<BTreeSet<_>>()
                    .len()
                    > 1
            });
            if ambiguous {
                totals.2 = totals.2.saturating_add(1);
            }

            for (kind, (_, indexes)) in nearest {
                let mut observed_keys = BTreeSet::new();
                for index in indexes {
                    let action = self.actions[index];
                    if !observed_keys.insert(action.ability_id) {
                        continue;
                    }
                    let delay = action.observed_micros.abs_diff(status.observed_micros);
                    let accumulator = correlations
                        .entry((status.effect_id, kind, action.ability_id))
                        .or_default();
                    accumulator.observation_count = accumulator.observation_count.saturating_add(1);
                    accumulator.same_target_count = accumulator.same_target_count.saturating_add(
                        u64::from(action.target_entity_uuid == Some(status.target_entity_uuid)),
                    );
                    match action
                        .observed_micros
                        .cmp(&status.observed_micros)
                        .then_with(|| action.sequence.cmp(&status.sequence))
                    {
                        std::cmp::Ordering::Less => {
                            accumulator.preceding_status_count =
                                accumulator.preceding_status_count.saturating_add(1)
                        }
                        std::cmp::Ordering::Greater => {
                            accumulator.following_status_count =
                                accumulator.following_status_count.saturating_add(1)
                        }
                        std::cmp::Ordering::Equal => {
                            accumulator.simultaneous_count =
                                accumulator.simultaneous_count.saturating_add(1)
                        }
                    }
                    accumulator.owner_resolved_count = accumulator
                        .owner_resolved_count
                        .saturating_add(u64::from(resolved_owner.is_some()));
                    accumulator.minimum_absolute_delay_micros = Some(
                        accumulator
                            .minimum_absolute_delay_micros
                            .map_or(delay, |minimum| minimum.min(delay)),
                    );
                    accumulator.maximum_absolute_delay_micros =
                        accumulator.maximum_absolute_delay_micros.max(delay);
                }
            }
        }

        let mut result = BTreeMap::<i64, EffectOriginCorrelation>::new();
        for (effect_id, (total, uncorrelated, ambiguous)) in totals {
            result.insert(
                effect_id,
                EffectOriginCorrelation {
                    total,
                    uncorrelated,
                    ambiguous,
                    abilities: Vec::new(),
                },
            );
        }
        for ((effect_id, action_kind, ability_id), accumulator) in correlations {
            result
                .entry(effect_id)
                .or_default()
                .abilities
                .push(RdpsAuditAbilityCorrelation {
                    ability_id,
                    base_skill_id: (action_kind == RdpsAuditActionKind::Cast && ability_id >= 100)
                        .then_some(ability_id / 100),
                    action_kind,
                    observation_count: accumulator.observation_count,
                    same_target_count: accumulator.same_target_count,
                    preceding_status_count: accumulator.preceding_status_count,
                    following_status_count: accumulator.following_status_count,
                    simultaneous_count: accumulator.simultaneous_count,
                    owner_resolved_count: accumulator.owner_resolved_count,
                    minimum_absolute_delay_micros: accumulator
                        .minimum_absolute_delay_micros
                        .unwrap_or_default(),
                    maximum_absolute_delay_micros: accumulator.maximum_absolute_delay_micros,
                });
        }
        for correlation in result.values_mut() {
            correlation.abilities.sort_by_key(|ability| {
                (
                    Reverse(ability.observation_count),
                    ability.action_kind,
                    ability.ability_id,
                )
            });
        }
        result
    }

    fn refresh_expiration(
        &mut self,
        window_id: u64,
        observed_micros: u64,
        duration_millis: Option<u64>,
    ) {
        let Some(window) = self.active.get_mut(&window_id) else {
            return;
        };
        window.expiration_generation = window.expiration_generation.saturating_add(1);
        window.expiration_micros = duration_millis
            .filter(|duration| *duration > 0)
            .map(|duration| observed_micros.saturating_add(duration.saturating_mul(1_000)));
        if let Some(expiration) = window.expiration_micros {
            self.expirations.push(Reverse((
                expiration,
                window_id,
                window.expiration_generation,
            )));
        }
    }

    fn expire_before(&mut self, observed_micros: u64) {
        while let Some(Reverse((expiration, window_id, generation))) =
            self.expirations.peek().copied()
        {
            if expiration > observed_micros {
                break;
            }
            self.expirations.pop();
            if self.active.get(&window_id).is_some_and(|window| {
                window.expiration_generation == generation
                    && window.expiration_micros == Some(expiration)
            }) {
                self.close_window(window_id);
            }
        }
    }

    fn close_window(&mut self, window_id: u64) {
        let Some(window) = self.active.remove(&window_id) else {
            return;
        };
        self.active_by_key.remove(&window.key);
        remove_index(
            &mut self.outgoing_by_actor,
            window.key.target_actor_id,
            window_id,
        );
        remove_index(
            &mut self.incoming_by_actor,
            window.key.target_actor_id,
            window_id,
        );
        let _ = window.source_entity_uuid;
        let _ = window.target_entity_uuid;
    }
}

fn remove_index(index: &mut HashMap<u64, BTreeSet<u64>>, actor_id: u64, window_id: u64) {
    let remove_actor = index.get_mut(&actor_id).is_some_and(|windows| {
        windows.remove(&window_id);
        windows.is_empty()
    });
    if remove_actor {
        index.remove(&actor_id);
    }
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        ActorEvent, ActorId, ActorKind, ActorOwnershipUpdate, ActorState, CanonicalEvent,
        EVENT_SCHEMA_VERSION, EntityAttributeEvent, EntityAttributeUpdateKind, EntityRef,
        EntityUuid, EventEnvelope, EventProvenance, EventSensitivity, EventTime, RegionContext,
        RegionIdentity, StatusEffectId, StatusEffectInstanceId, StatusEvent, StatusOrigin,
        StatusState, TimelineEvent, TimelineEventKind,
    };

    use super::{
        MAXIMUM_PROVIDER_RECIPIENT_EXAMPLES_PER_CLASS, RDPS_AUDIT_SCHEMA_VERSION,
        RdpsAuditProviderClass, RdpsAuditProviderRecipientExampleClass, RdpsAuditRecipientClass,
        RdpsEffectAuditAnalyzer,
    };

    fn status_envelope(
        sequence: u64,
        effect_id: i64,
        instance_id: i64,
        source_type_id: i32,
        source_config_id: i64,
    ) -> EventEnvelope {
        status_envelope_between(
            sequence,
            effect_id,
            instance_id,
            source_type_id,
            source_config_id,
            1,
            100,
            2,
            200,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn status_envelope_between(
        sequence: u64,
        effect_id: i64,
        instance_id: i64,
        source_type_id: i32,
        source_config_id: i64,
        source_actor_id: u64,
        source_entity_uuid: i64,
        target_actor_id: u64,
        target_entity_uuid: i64,
    ) -> EventEnvelope {
        let time = EventTime {
            observed_micros: sequence * 1_000,
            game_time_millis: None,
        };
        let provenance = EventProvenance::manual("rDPS audit origin test");
        EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "packet-origin-test".into(),
            sequence,
            region: RegionContext {
                identity: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: None,
                },
                client_build: "24252055".into(),
                protocol_pack_digest: "test".into(),
                evidence: vec![],
            },
            time,
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time,
                provenance,
                kind: TimelineEventKind::Status(StatusEvent {
                    source: Some(EntityRef {
                        actor_id: ActorId(source_actor_id),
                        entity_uuid: EntityUuid(source_entity_uuid),
                    }),
                    target: EntityRef {
                        actor_id: ActorId(target_actor_id),
                        entity_uuid: EntityUuid(target_entity_uuid),
                    },
                    effect: StatusEffectId(effect_id),
                    instance_id: Some(StatusEffectInstanceId(instance_id)),
                    origin: Some(StatusOrigin {
                        source_type_id,
                        source_config_id,
                    }),
                    state: StatusState::Applied,
                    stacks: Some(1),
                    level: None,
                    part_id: None,
                    count: None,
                    created_at_millis: None,
                    duration_millis: Some(10_000),
                }),
            }),
        }
    }

    fn ownership_envelope(
        sequence: u64,
        actor_id: u64,
        entity_uuid: i64,
        owner_entity_uuid: i64,
    ) -> EventEnvelope {
        let mut envelope = actor_envelope(sequence, actor_id, entity_uuid);
        let CanonicalEvent::Timeline(timeline) = &mut envelope.event else {
            unreachable!()
        };
        timeline.kind = TimelineEventKind::EntityAttributes(EntityAttributeEvent {
            actor: EntityRef {
                actor_id: ActorId(actor_id),
                entity_uuid: EntityUuid(entity_uuid),
            },
            update_kind: EntityAttributeUpdateKind::Delta,
            ownership: Some(ActorOwnershipUpdate::Confirmed {
                owner_entity_uuid: EntityUuid(owner_entity_uuid),
            }),
            attributes: Vec::new(),
        });
        envelope
    }

    fn actor_envelope(sequence: u64, actor_id: u64, entity_uuid: i64) -> EventEnvelope {
        let time = EventTime {
            observed_micros: sequence * 1_000,
            game_time_millis: None,
        };
        let provenance = EventProvenance::manual("rDPS audit actor test");
        EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "packet-origin-test".into(),
            sequence,
            region: RegionContext {
                identity: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: None,
                },
                client_build: "24252055".into(),
                protocol_pack_digest: "test".into(),
                evidence: vec![],
            },
            time,
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time,
                provenance,
                kind: TimelineEventKind::Actor(ActorEvent {
                    actor: EntityRef {
                        actor_id: ActorId(actor_id),
                        entity_uuid: EntityUuid(entity_uuid),
                    },
                    state: ActorState::Spawned,
                    entity_type_id: 1,
                    kind: ActorKind::Player,
                    character_id: None,
                    monster_id: None,
                    display_name: None,
                    class_id: None,
                    specialization_id: None,
                    level: None,
                    ability_score: None,
                    weapon_item_id: None,
                    weapon_breakthrough_count: None,
                    seasonal_score: None,
                    primary_loadout: vec![],
                    auxiliary_loadout: vec![],
                    loadout_observation: Default::default(),
                }),
            }),
        }
    }

    #[test]
    fn packet_origins_are_counted_exactly_and_competing_origins_are_preserved() {
        let mut analyzer = RdpsEffectAuditAnalyzer::new();
        analyzer
            .observe(&status_envelope(1, 2_208_281, 10, 1, 2_208_280))
            .unwrap();
        analyzer
            .observe(&status_envelope(2, 2_208_281, 11, 1, 2_208_280))
            .unwrap();
        analyzer
            .observe(&status_envelope(3, 2_208_281, 12, 1, 2_208_340))
            .unwrap();

        let report = analyzer.finish().unwrap();
        assert_eq!(report.schema_version, RDPS_AUDIT_SCHEMA_VERSION);
        let effect = report
            .effects
            .iter()
            .find(|effect| effect.effect_id == 2_208_281)
            .unwrap();
        assert_eq!(effect.packet_origin_observation_count, 3);
        assert_eq!(effect.packet_origins.len(), 2);
        assert_eq!(effect.packet_origins[0].source_type_id, 1);
        assert_eq!(effect.packet_origins[0].source_config_id, 2_208_280);
        assert_eq!(effect.packet_origins[0].observation_count, 2);
        assert_eq!(effect.packet_origins[1].source_type_id, 1);
        assert_eq!(effect.packet_origins[1].source_config_id, 2_208_340);
        assert_eq!(effect.packet_origins[1].observation_count, 1);
    }

    #[test]
    fn packet_origin_audit_carries_formula_endpoint_and_owner_certainty() {
        let mut analyzer = RdpsEffectAuditAnalyzer::new();
        analyzer
            .observe(&status_envelope(1, 25_204, 10, 1, 2_204_030))
            .unwrap();

        let report = analyzer.finish().unwrap();
        let origin = &report
            .effects
            .iter()
            .find(|effect| effect.effect_id == 25_204)
            .unwrap()
            .packet_origins[0];
        assert_eq!(
            origin.fingerprint_match_kind,
            crate::EffectFingerprintMatchKind::ExactPacketOrigin
        );
        assert_eq!(
            origin.endpoint_resolution,
            crate::EffectFingerprintResolution::Exact
        );
        assert_eq!(
            origin.owner_resolution,
            crate::EffectFingerprintResolution::Exact
        );
        assert_eq!(origin.candidate_sources.len(), 1);
        assert_eq!(origin.candidate_sources[0].source_id, "talent:202");
    }

    #[test]
    fn cross_actor_matrix_proves_player_provider_and_player_recipient_on_same_window() {
        let mut analyzer = RdpsEffectAuditAnalyzer::new();
        analyzer.observe(&actor_envelope(1, 1, 100)).unwrap();
        analyzer.observe(&actor_envelope(2, 2, 200)).unwrap();
        analyzer
            .observe(&status_envelope(3, 3_003_052, 10, 1, 3_003_053))
            .unwrap();

        let report = analyzer.finish().unwrap();
        let effect = report
            .effects
            .iter()
            .find(|effect| effect.effect_id == 3_003_052)
            .unwrap();
        assert_eq!(effect.cross_actor_window_count, 1);
        assert_eq!(
            effect
                .cross_actor_provider_recipient_windows
                .resolved_player_to_player,
            1
        );
        assert_eq!(
            effect
                .cross_actor_provider_recipient_windows
                .resolved_external_player_to_player,
            1
        );
        assert_eq!(
            effect
                .cross_actor_provider_recipient_windows
                .resolved_same_owner_player_to_player,
            0
        );
        assert_eq!(
            effect
                .cross_actor_provider_recipient_windows
                .resolved_player_to_monster,
            0
        );
        assert_eq!(
            effect
                .cross_actor_provider_recipient_windows
                .unresolved_to_other,
            0
        );
        assert_eq!(effect.provider_recipient_examples.len(), 1);
        let example = &effect.provider_recipient_examples[0];
        assert_eq!(
            example.class,
            RdpsAuditProviderRecipientExampleClass::ExternalPlayerToPlayer
        );
        assert_eq!(example.raw_source_actor_id, Some(1));
        assert_eq!(example.raw_target_actor_id, 2);
        assert_eq!(example.raw_source_entity_uuid, Some(100));
        assert_eq!(example.resolved_source_entity_uuid, Some(100));
        assert_eq!(example.raw_target_entity_uuid, 200);
        assert_eq!(example.resolved_target_entity_uuid, 200);
        assert_eq!(
            example.provider_class,
            RdpsAuditProviderClass::ResolvedPlayer
        );
        assert_eq!(example.recipient_class, RdpsAuditRecipientClass::Player);
        assert!(example.cross_actor);
        assert!(!example.same_resolved_owner);
    }

    #[test]
    fn owned_proxy_affecting_its_owner_is_not_external_player_support() {
        let mut analyzer = RdpsEffectAuditAnalyzer::new();
        analyzer.observe(&actor_envelope(1, 1, 100)).unwrap();
        analyzer.observe(&actor_envelope(2, 3, 300)).unwrap();
        analyzer
            .observe(&ownership_envelope(3, 3, 300, 100))
            .unwrap();
        analyzer
            .observe(&status_envelope_between(
                4, 2_110_093, 10, 1, 2_110_092, 3, 300, 1, 100,
            ))
            .unwrap();

        let report = analyzer.finish().unwrap();
        let effect = report
            .effects
            .iter()
            .find(|effect| effect.effect_id == 2_110_093)
            .unwrap();
        assert_eq!(effect.cross_actor_window_count, 1);
        assert_eq!(effect.source_owner_resolved_window_count, 1);
        assert_eq!(
            effect
                .cross_actor_provider_recipient_windows
                .resolved_player_to_player,
            1
        );
        assert_eq!(
            effect
                .cross_actor_provider_recipient_windows
                .resolved_same_owner_player_to_player,
            1
        );
        assert_eq!(
            effect
                .cross_actor_provider_recipient_windows
                .resolved_external_player_to_player,
            0
        );
        assert_eq!(effect.provider_recipient_examples.len(), 1);
        let example = &effect.provider_recipient_examples[0];
        assert_eq!(
            example.class,
            RdpsAuditProviderRecipientExampleClass::SameOwnerPlayerToPlayer
        );
        assert_eq!(example.raw_source_actor_id, Some(3));
        assert_eq!(example.raw_source_entity_uuid, Some(300));
        assert_eq!(example.resolved_source_entity_uuid, Some(100));
        assert_eq!(example.raw_target_entity_uuid, 100);
        assert_eq!(example.resolved_target_entity_uuid, 100);
        assert!(example.cross_actor);
        assert!(example.same_resolved_owner);
    }

    #[test]
    fn provider_recipient_examples_are_bounded_and_deterministic_per_class() {
        let mut analyzer = RdpsEffectAuditAnalyzer::new();
        analyzer.observe(&actor_envelope(1, 1, 100)).unwrap();
        for offset in 0..(MAXIMUM_PROVIDER_RECIPIENT_EXAMPLES_PER_CLASS + 3) {
            let actor_id = 10 + offset as u64;
            let entity_uuid = 1_000 + offset as i64;
            analyzer
                .observe(&actor_envelope(2 + offset as u64, actor_id, entity_uuid))
                .unwrap();
        }
        for offset in 0..(MAXIMUM_PROVIDER_RECIPIENT_EXAMPLES_PER_CLASS + 3) {
            let actor_id = 10 + offset as u64;
            let entity_uuid = 1_000 + offset as i64;
            analyzer
                .observe(&status_envelope_between(
                    20 + offset as u64,
                    3_003_052,
                    100 + offset as i64,
                    1,
                    3_003_053,
                    1,
                    100,
                    actor_id,
                    entity_uuid,
                ))
                .unwrap();
        }

        let report = analyzer.finish().unwrap();
        let effect = report
            .effects
            .iter()
            .find(|effect| effect.effect_id == 3_003_052)
            .unwrap();
        assert_eq!(
            effect.provider_recipient_examples.len(),
            MAXIMUM_PROVIDER_RECIPIENT_EXAMPLES_PER_CLASS
        );
        assert_eq!(
            effect.provider_recipient_examples[0].raw_target_entity_uuid,
            1_000
        );
        assert_eq!(
            effect.provider_recipient_examples[3].raw_target_entity_uuid,
            1_003
        );
        assert!(effect.provider_recipient_examples.iter().all(|example| {
            example.class == RdpsAuditProviderRecipientExampleClass::ExternalPlayerToPlayer
        }));
    }
}
