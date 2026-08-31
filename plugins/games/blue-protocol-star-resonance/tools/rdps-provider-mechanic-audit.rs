use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorEvent, ActorKind, ActorLoadoutSlot, CanonicalEvent, DamageEvent, EntityAttribute,
    EntityAttributeEvent, EntityAttributeValue, EntityRef, EventEnvelope, RunState, StatusEvent,
    StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 4;
const MAXIMUM_ACTIVE_WINDOWS: usize = 200_000;
const DEFAULT_EXAMPLE_LIMIT: usize = 8;
const SUMMON_OWNER_ATTRIBUTE_IDS: [i32; 2] = [90, 91];

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    candidate_inventory: String,
    candidate_effect_count: usize,
    reports: Vec<SessionReport>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    packet_events_are_exact: bool,
    localization_enables_mechanics: bool,
    temporal_overlap_enables_rdps: bool,
    unresolved_evidence_is_hidden: bool,
    owner_resolution: &'static str,
    actor_metadata_resolution: &'static str,
    overlap_deduplication: &'static str,
    runtime_use: &'static str,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    run_ordinals_observed: u32,
    run_boundaries: u64,
    damage_events: u64,
    candidate_status_events: u64,
    unmatched_terminal_status_events: u64,
    maximum_simultaneous_candidate_windows: usize,
    effects: Vec<EffectReport>,
}

#[derive(Debug, Serialize)]
struct EffectReport {
    effect_id: i64,
    lifecycle: LifecycleReport,
    recipient_scope: RecipientScopeCounts,
    providers: Vec<ProviderReport>,
    overlap_damage: OverlapDamageReport,
    examples: Vec<WindowExample>,
}

#[derive(Debug, Default, Serialize)]
struct LifecycleReport {
    status_events: u64,
    opened_windows: u64,
    closed_windows: u64,
    cross_actor_windows: u64,
    source_missing_windows: u64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    unmatched_terminal_events: u64,
    minimum_stacks: Option<u32>,
    maximum_stacks: Option<u32>,
    observed_active_micros: u64,
}

#[derive(Debug, Default, Serialize)]
struct RecipientScopeCounts {
    player: u64,
    monster: u64,
    other: u64,
    unresolved: u64,
}

#[derive(Debug, Serialize)]
struct ProviderReport {
    resolved_provider_entity_uuid: Option<i64>,
    provider_kind: Option<String>,
    display_name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
    ability_score: Option<i64>,
    weapon_item_id: Option<i64>,
    seasonal_score: Option<i64>,
    primary_loadout: Vec<ObservedLoadoutSlot>,
    auxiliary_loadout: Vec<ObservedLoadoutSlot>,
    resolution: ProviderResolution,
    windows: u64,
    player_recipient_windows: u64,
    monster_recipient_windows: u64,
    other_recipient_windows: u64,
    unresolved_recipient_windows: u64,
    raw_source_entities: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ObservedLoadoutSlot {
    slot_id: i32,
    ability_id: Option<i64>,
    item_id: Option<i64>,
    tier: Option<u32>,
}

impl From<&ActorLoadoutSlot> for ObservedLoadoutSlot {
    fn from(slot: &ActorLoadoutSlot) -> Self {
        Self {
            slot_id: slot.slot_id,
            ability_id: slot.ability_id,
            item_id: slot.item_id,
            tier: slot.tier,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProviderResolution {
    DirectPlayer,
    DirectSourceOwnerLinkWithinRun,
    PairedOwnerAttributesWithinRun,
    NonPlayer,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnerLink {
    owner_entity_uuid: i64,
    resolution: ProviderResolution,
}

#[derive(Debug, Default, Serialize)]
struct OverlapDamageReport {
    player_recipient_outgoing: DamageTotals,
    monster_incoming_from_provider: DamageTotals,
    monster_incoming_from_other_players: DamageTotals,
    monster_incoming_from_non_players: DamageTotals,
    monster_incoming_from_unresolved_sources: DamageTotals,
    other_or_unresolved_recipient_incoming: DamageTotals,
}

#[derive(Debug, Default, Clone, Serialize)]
struct DamageTotals {
    events: u64,
    amount: i64,
    critical_events: u64,
    lucky_events: u64,
}

impl DamageTotals {
    fn observe(&mut self, damage: &DamageEvent) {
        self.events = self.events.saturating_add(1);
        self.amount = self.amount.saturating_add(damage.amount.max(0));
        self.critical_events = self
            .critical_events
            .saturating_add(u64::from(damage.flags.critical == Some(true)));
        self.lucky_events = self
            .lucky_events
            .saturating_add(u64::from(damage.flags.lucky == Some(true)));
    }

    fn merge(&mut self, other: &Self) {
        self.events = self.events.saturating_add(other.events);
        self.amount = self.amount.saturating_add(other.amount);
        self.critical_events = self.critical_events.saturating_add(other.critical_events);
        self.lucky_events = self.lucky_events.saturating_add(other.lucky_events);
    }
}

#[derive(Debug, Clone, Serialize)]
struct WindowExample {
    run_ordinal: u32,
    opened_sequence: u64,
    opened_micros: u64,
    closed_sequence: u64,
    closed_micros: u64,
    instance_id: Option<i64>,
    raw_source_entity_uuid: Option<i64>,
    resolved_provider_entity_uuid: Option<i64>,
    provider_resolution: ProviderResolution,
    provider_class_id: Option<i32>,
    provider_specialization_id: Option<i32>,
    target_entity_uuid: i64,
    recipient_scope: RecipientScope,
    packet_origin_source_type_id: Option<i32>,
    packet_origin_source_config_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum RecipientScope {
    Player,
    Monster,
    Other,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActorSnapshot {
    sequence: u64,
    kind: ActorKind,
    display_name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
    ability_score: Option<i64>,
    weapon_item_id: Option<i64>,
    seasonal_score: Option<i64>,
    primary_loadout: Vec<ObservedLoadoutSlot>,
    auxiliary_loadout: Vec<ObservedLoadoutSlot>,
}

fn merge_actor_snapshot(
    previous: Option<&ActorSnapshot>,
    sequence: u64,
    actor: &ActorEvent,
) -> ActorSnapshot {
    let mut snapshot = previous.cloned().unwrap_or(ActorSnapshot {
        sequence,
        kind: actor.kind,
        display_name: None,
        class_id: None,
        specialization_id: None,
        level: None,
        ability_score: None,
        weapon_item_id: None,
        seasonal_score: None,
        primary_loadout: Vec::new(),
        auxiliary_loadout: Vec::new(),
    });
    let class_changed = actor
        .class_id
        .zip(snapshot.class_id)
        .is_some_and(|(observed, retained)| observed != retained);

    snapshot.sequence = sequence;
    snapshot.kind = actor.kind;
    if let Some(value) = &actor.display_name {
        snapshot.display_name = Some(value.clone());
    }
    if class_changed {
        snapshot.specialization_id = None;
        snapshot.weapon_item_id = None;
        snapshot.primary_loadout.clear();
        snapshot.auxiliary_loadout.clear();
    }
    if let Some(value) = actor.class_id {
        snapshot.class_id = Some(value);
    }
    if let Some(value) = actor.specialization_id {
        snapshot.specialization_id = Some(value);
    }
    if let Some(value) = actor.level {
        snapshot.level = Some(value);
    }
    if let Some(value) = actor.ability_score {
        snapshot.ability_score = Some(value);
    }
    if let Some(value) = actor.weapon_item_id {
        snapshot.weapon_item_id = Some(value);
    }
    if let Some(value) = actor.seasonal_score {
        snapshot.seasonal_score = Some(value);
    }
    if !actor.primary_loadout.is_empty() {
        snapshot.primary_loadout = actor
            .primary_loadout
            .iter()
            .map(ObservedLoadoutSlot::from)
            .collect();
    }
    if !actor.auxiliary_loadout.is_empty() {
        snapshot.auxiliary_loadout = actor
            .auxiliary_loadout
            .iter()
            .map(ObservedLoadoutSlot::from)
            .collect();
    }
    snapshot
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StatusWindowKey {
    run_ordinal: u32,
    effect_id: i64,
    instance_id: Option<i64>,
    source_actor_id: Option<u64>,
    target_actor_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct InstanceTargetKey {
    run_ordinal: u32,
    effect_id: i64,
    instance_id: i64,
    target_actor_id: u64,
}

#[derive(Debug, Clone)]
struct ActiveWindow {
    key: StatusWindowKey,
    raw_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    opened_sequence: u64,
    opened_micros: u64,
    expiration_micros: Option<u64>,
    expiration_generation: u64,
    stacks: Option<u32>,
    packet_origin: Option<(i32, i64)>,
}

#[derive(Debug, Clone)]
struct ClosedWindow {
    effect_id: i64,
    run_ordinal: u32,
    instance_id: Option<i64>,
    raw_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    opened_sequence: u64,
    opened_micros: u64,
    closed_sequence: u64,
    closed_micros: u64,
    packet_origin: Option<(i32, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProviderKey {
    entity_uuid: Option<i64>,
    kind: Option<String>,
    display_name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
    ability_score: Option<i64>,
    weapon_item_id: Option<i64>,
    seasonal_score: Option<i64>,
    primary_loadout: Vec<ObservedLoadoutSlot>,
    auxiliary_loadout: Vec<ObservedLoadoutSlot>,
    resolution: ProviderResolution,
}

#[derive(Debug, Default)]
struct ProviderAccumulator {
    windows: u64,
    player_recipient_windows: u64,
    monster_recipient_windows: u64,
    other_recipient_windows: u64,
    unresolved_recipient_windows: u64,
    raw_source_entities: BTreeSet<i64>,
}

#[derive(Debug, Default)]
struct EffectAccumulator {
    lifecycle: LifecycleReport,
    recipient_scope: RecipientScopeCounts,
    providers: BTreeMap<ProviderKey, ProviderAccumulator>,
    overlap_damage: OverlapDamageReport,
    examples: Vec<WindowExample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct OverlapKey {
    effect_id: i64,
    run_ordinal: u32,
    raw_provider_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    damage_source_entity_uuid: i64,
    recipient_scope: RecipientScope,
}

#[derive(Debug, Default)]
struct Analyzer {
    session_id: Option<String>,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    last_sequence: u64,
    current_run_ordinal: u32,
    maximum_run_ordinal: u32,
    run_boundaries: u64,
    damage_events: u64,
    candidate_status_events: u64,
    unmatched_terminal_status_events: u64,
    maximum_simultaneous_candidate_windows: usize,
    candidate_effects: BTreeSet<i64>,
    actor_history_by_entity: HashMap<i64, Vec<ActorSnapshot>>,
    actor_entities_by_actor_id: HashMap<u64, i64>,
    owner_by_run_and_direct_entity: HashMap<(u32, i64), OwnerLink>,
    owner_attribute_candidates: HashMap<(u32, i64), [Option<i64>; 2]>,
    active_by_key: HashMap<StatusWindowKey, u64>,
    active_by_instance_target: HashMap<InstanceTargetKey, BTreeSet<u64>>,
    active_by_target_actor: HashMap<u64, BTreeSet<u64>>,
    active_by_recipient_actor: HashMap<u64, BTreeSet<u64>>,
    active: HashMap<u64, ActiveWindow>,
    expirations: BinaryHeap<Reverse<(u64, u64, u64)>>,
    next_window_id: u64,
    closed_windows: Vec<ClosedWindow>,
    overlap_damage: BTreeMap<OverlapKey, DamageTotals>,
    lifecycle_by_effect: BTreeMap<i64, LifecycleReport>,
}

impl Analyzer {
    fn new(candidate_effects: &BTreeSet<i64>) -> Self {
        Self {
            candidate_effects: candidate_effects.clone(),
            next_window_id: 1,
            ..Self::default()
        }
    }

    fn observe(&mut self, envelope: &EventEnvelope) -> Result<(), String> {
        if let Some(expected) = &self.session_id {
            if expected != &envelope.session_id {
                return Err(format!(
                    "rlog contains multiple sessions: {expected} and {}",
                    envelope.session_id
                ));
            }
        } else {
            self.session_id = Some(envelope.session_id.clone());
        }
        if envelope.sequence < self.last_sequence {
            return Err(format!(
                "event sequence moved backward from {} to {}",
                self.last_sequence, envelope.sequence
            ));
        }
        self.last_sequence = envelope.sequence;
        self.first_observed_micros
            .get_or_insert(envelope.time.observed_micros);
        self.last_observed_micros = Some(envelope.time.observed_micros);
        self.expire_before(envelope.time.observed_micros, envelope.sequence);

        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return Ok(());
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => {
                self.observe_run_boundary(*state, envelope.sequence, envelope.time.observed_micros)
            }
            TimelineEventKind::Actor(actor) => self.observe_actor(envelope.sequence, actor),
            TimelineEventKind::EntityAttributes(attributes) => {
                self.observe_owner_attributes(attributes)
            }
            TimelineEventKind::Status(status) => {
                self.observe_status(envelope.sequence, envelope.time.observed_micros, status)?
            }
            TimelineEventKind::Damage(damage) => self.observe_damage(damage),
            TimelineEventKind::Healing(healing) => {
                self.observe_owner_link(healing.source, healing.direct_source)
            }
            _ => {}
        }
        Ok(())
    }

    fn observe_run_boundary(&mut self, state: RunState, sequence: u64, observed_micros: u64) {
        self.run_boundaries = self.run_boundaries.saturating_add(1);
        match state {
            RunState::Entered => {
                self.close_all(sequence, observed_micros);
                self.current_run_ordinal = self.current_run_ordinal.saturating_add(1);
                self.maximum_run_ordinal = self.maximum_run_ordinal.max(self.current_run_ordinal);
            }
            RunState::Started if self.current_run_ordinal == 0 => {
                self.current_run_ordinal = 1;
                self.maximum_run_ordinal = 1;
            }
            RunState::Ended | RunState::Completed | RunState::Failed | RunState::Exited => {
                self.close_all(sequence, observed_micros)
            }
            RunState::Started => {}
        }
    }

    fn observe_actor(&mut self, sequence: u64, actor: &ActorEvent) {
        self.actor_entities_by_actor_id
            .insert(actor.actor.actor_id.0, actor.actor.entity_uuid.0);
        let history = self
            .actor_history_by_entity
            .entry(actor.actor.entity_uuid.0)
            .or_default();
        let snapshot = merge_actor_snapshot(history.last(), sequence, actor);
        let changed = history.last().is_none_or(|previous| {
            previous.kind != snapshot.kind
                || previous.display_name != snapshot.display_name
                || previous.class_id != snapshot.class_id
                || previous.specialization_id != snapshot.specialization_id
                || previous.level != snapshot.level
                || previous.ability_score != snapshot.ability_score
                || previous.weapon_item_id != snapshot.weapon_item_id
                || previous.seasonal_score != snapshot.seasonal_score
                || previous.primary_loadout != snapshot.primary_loadout
                || previous.auxiliary_loadout != snapshot.auxiliary_loadout
        });
        if changed {
            history.push(snapshot);
        }
    }

    fn observe_status(
        &mut self,
        sequence: u64,
        observed_micros: u64,
        status: &StatusEvent,
    ) -> Result<(), String> {
        if !self.candidate_effects.contains(&status.effect.0) {
            return Ok(());
        }
        self.candidate_status_events = self.candidate_status_events.saturating_add(1);
        let lifecycle = self.lifecycle_by_effect.entry(status.effect.0).or_default();
        lifecycle.status_events = lifecycle.status_events.saturating_add(1);
        match status.state {
            StatusState::Applied => lifecycle.applied = lifecycle.applied.saturating_add(1),
            StatusState::Refreshed => lifecycle.refreshed = lifecycle.refreshed.saturating_add(1),
            StatusState::Stacked => lifecycle.stacked = lifecycle.stacked.saturating_add(1),
            StatusState::Consumed => lifecycle.consumed = lifecycle.consumed.saturating_add(1),
            StatusState::Removed => lifecycle.removed = lifecycle.removed.saturating_add(1),
        }
        if let Some(stacks) = status.stacks {
            lifecycle.minimum_stacks = Some(
                lifecycle
                    .minimum_stacks
                    .map_or(stacks, |current| current.min(stacks)),
            );
            lifecycle.maximum_stacks = Some(
                lifecycle
                    .maximum_stacks
                    .map_or(stacks, |current| current.max(stacks)),
            );
        }

        let key = StatusWindowKey {
            run_ordinal: self.current_run_ordinal,
            effect_id: status.effect.0,
            instance_id: status.instance_id.map(|value| value.0),
            source_actor_id: status.source.map(|value| value.actor_id.0),
            target_actor_id: status.target.actor_id.0,
        };
        let terminal = matches!(status.state, StatusState::Removed)
            || matches!(status.state, StatusState::Consumed) && status.stacks == Some(0);
        if terminal {
            if let Some(window_id) = self.find_window_id(key) {
                self.close_window(window_id, sequence, observed_micros);
            } else {
                self.unmatched_terminal_status_events =
                    self.unmatched_terminal_status_events.saturating_add(1);
                let lifecycle = self.lifecycle_by_effect.entry(status.effect.0).or_default();
                lifecycle.unmatched_terminal_events =
                    lifecycle.unmatched_terminal_events.saturating_add(1);
            }
            return Ok(());
        }

        if let Some(window_id) = self.find_window_id(key) {
            if let Some(window) = self.active.get_mut(&window_id) {
                window.stacks = status.stacks.or(window.stacks);
                if status.origin.is_some() {
                    window.packet_origin = status
                        .origin
                        .map(|origin| (origin.source_type_id, origin.source_config_id));
                }
            }
            self.refresh_expiration(window_id, observed_micros, status.duration_millis);
            return Ok(());
        }
        if self.active.len() >= MAXIMUM_ACTIVE_WINDOWS {
            return Err(format!(
                "candidate audit exceeded {MAXIMUM_ACTIVE_WINDOWS} simultaneous active windows"
            ));
        }

        let raw_source_entity_uuid = status.source.map(|source| source.entity_uuid.0);
        let cross_actor =
            raw_source_entity_uuid.is_some_and(|source| source != status.target.entity_uuid.0);
        let lifecycle = self.lifecycle_by_effect.entry(status.effect.0).or_default();
        lifecycle.opened_windows = lifecycle.opened_windows.saturating_add(1);
        lifecycle.cross_actor_windows = lifecycle
            .cross_actor_windows
            .saturating_add(u64::from(cross_actor));
        lifecycle.source_missing_windows = lifecycle
            .source_missing_windows
            .saturating_add(u64::from(raw_source_entity_uuid.is_none()));

        let window_id = self.next_window_id;
        self.next_window_id = self.next_window_id.saturating_add(1);
        let window = ActiveWindow {
            key,
            raw_source_entity_uuid,
            target_entity_uuid: status.target.entity_uuid.0,
            opened_sequence: sequence,
            opened_micros: observed_micros,
            expiration_micros: None,
            expiration_generation: 0,
            stacks: status.stacks,
            packet_origin: status
                .origin
                .map(|origin| (origin.source_type_id, origin.source_config_id)),
        };
        self.active_by_key.insert(key, window_id);
        if let Some(instance_id) = key.instance_id {
            self.active_by_instance_target
                .entry(InstanceTargetKey {
                    run_ordinal: key.run_ordinal,
                    effect_id: key.effect_id,
                    instance_id,
                    target_actor_id: key.target_actor_id,
                })
                .or_default()
                .insert(window_id);
        }
        self.active_by_target_actor
            .entry(key.target_actor_id)
            .or_default()
            .insert(window_id);
        self.active_by_recipient_actor
            .entry(key.target_actor_id)
            .or_default()
            .insert(window_id);
        self.active.insert(window_id, window);
        self.maximum_simultaneous_candidate_windows = self
            .maximum_simultaneous_candidate_windows
            .max(self.active.len());
        self.refresh_expiration(window_id, observed_micros, status.duration_millis);
        Ok(())
    }

    fn find_window_id(&self, key: StatusWindowKey) -> Option<u64> {
        if let Some(window_id) = self.active_by_key.get(&key) {
            return Some(*window_id);
        }
        let instance_id = key.instance_id?;
        let candidates = self.active_by_instance_target.get(&InstanceTargetKey {
            run_ordinal: key.run_ordinal,
            effect_id: key.effect_id,
            instance_id,
            target_actor_id: key.target_actor_id,
        })?;
        (candidates.len() == 1).then(|| *candidates.first().expect("one candidate exists"))
    }

    fn observe_owner_link(&mut self, owner: EntityRef, direct_source: Option<EntityRef>) {
        if let Some(direct_source) =
            direct_source.filter(|direct| direct.entity_uuid != owner.entity_uuid)
        {
            self.owner_by_run_and_direct_entity
                .entry((self.current_run_ordinal, direct_source.entity_uuid.0))
                .and_modify(|link| {
                    if link.owner_entity_uuid == owner.entity_uuid.0 {
                        link.resolution = ProviderResolution::DirectSourceOwnerLinkWithinRun;
                    }
                })
                .or_insert(OwnerLink {
                    owner_entity_uuid: owner.entity_uuid.0,
                    resolution: ProviderResolution::DirectSourceOwnerLinkWithinRun,
                });
        }
    }

    fn observe_owner_attributes(&mut self, event: &EntityAttributeEvent) {
        let key = (self.current_run_ordinal, event.actor.entity_uuid.0);
        let candidates = self.owner_attribute_candidates.entry(key).or_default();
        for attribute in &event.attributes {
            let Some(index) = SUMMON_OWNER_ATTRIBUTE_IDS
                .iter()
                .position(|attribute_id| *attribute_id == attribute.attribute_id)
            else {
                continue;
            };
            candidates[index] = integer_attribute(attribute);
        }

        let [Some(primary), Some(confirmation)] = *candidates else {
            return;
        };
        if primary > 0 && primary == confirmation {
            self.owner_by_run_and_direct_entity
                .entry(key)
                .or_insert(OwnerLink {
                    owner_entity_uuid: primary,
                    resolution: ProviderResolution::PairedOwnerAttributesWithinRun,
                });
        } else if self
            .owner_by_run_and_direct_entity
            .get(&key)
            .is_some_and(|link| {
                link.resolution == ProviderResolution::PairedOwnerAttributesWithinRun
            })
        {
            self.owner_by_run_and_direct_entity.remove(&key);
        }
    }

    fn observe_damage(&mut self, damage: &DamageEvent) {
        self.damage_events = self.damage_events.saturating_add(1);
        self.observe_owner_link(damage.source, damage.direct_source);

        let outgoing = self
            .active_by_recipient_actor
            .get(&damage.source.actor_id.0)
            .cloned()
            .unwrap_or_default();
        let mut outgoing_keys = BTreeSet::new();
        for window_id in outgoing {
            let Some(window) = self.active.get(&window_id) else {
                continue;
            };
            let recipient_scope =
                self.recipient_scope(window.target_entity_uuid, window.opened_sequence);
            if recipient_scope != RecipientScope::Player {
                continue;
            }
            outgoing_keys.insert(OverlapKey {
                effect_id: window.key.effect_id,
                run_ordinal: window.key.run_ordinal,
                raw_provider_entity_uuid: window.raw_source_entity_uuid,
                target_entity_uuid: window.target_entity_uuid,
                damage_source_entity_uuid: damage.source.entity_uuid.0,
                recipient_scope,
            });
        }
        for key in outgoing_keys {
            self.overlap_damage.entry(key).or_default().observe(damage);
        }

        let incoming = self
            .active_by_target_actor
            .get(&damage.target.actor_id.0)
            .cloned()
            .unwrap_or_default();
        let mut incoming_keys = BTreeSet::new();
        for window_id in incoming {
            let Some(window) = self.active.get(&window_id) else {
                continue;
            };
            incoming_keys.insert(OverlapKey {
                effect_id: window.key.effect_id,
                run_ordinal: window.key.run_ordinal,
                raw_provider_entity_uuid: window.raw_source_entity_uuid,
                target_entity_uuid: window.target_entity_uuid,
                damage_source_entity_uuid: damage.source.entity_uuid.0,
                recipient_scope: self
                    .recipient_scope(window.target_entity_uuid, window.opened_sequence),
            });
        }
        for key in incoming_keys {
            self.overlap_damage.entry(key).or_default().observe(damage);
        }
    }

    fn resolve_owner(&self, run_ordinal: u32, raw_entity_uuid: i64) -> i64 {
        self.owner_by_run_and_direct_entity
            .get(&(run_ordinal, raw_entity_uuid))
            .map(|link| link.owner_entity_uuid)
            .unwrap_or(raw_entity_uuid)
    }

    fn actor_snapshot_at(&self, entity_uuid: i64, sequence: u64) -> Option<&ActorSnapshot> {
        let history = self.actor_history_by_entity.get(&entity_uuid)?;
        let index = history.partition_point(|snapshot| snapshot.sequence <= sequence);
        index.checked_sub(1).and_then(|index| history.get(index))
    }

    fn recipient_scope(&self, entity_uuid: i64, sequence: u64) -> RecipientScope {
        match self
            .actor_snapshot_at(entity_uuid, sequence)
            .map(|snapshot| snapshot.kind)
        {
            Some(ActorKind::Player) => RecipientScope::Player,
            Some(ActorKind::Monster) | Some(ActorKind::TrainingDummy) => RecipientScope::Monster,
            Some(_) => RecipientScope::Other,
            None => RecipientScope::Unresolved,
        }
    }

    fn provider_key(&self, window: &ClosedWindow) -> ProviderKey {
        let Some(raw_source) = window.raw_source_entity_uuid else {
            return ProviderKey {
                entity_uuid: None,
                kind: None,
                display_name: None,
                class_id: None,
                specialization_id: None,
                level: None,
                ability_score: None,
                weapon_item_id: None,
                seasonal_score: None,
                primary_loadout: Vec::new(),
                auxiliary_loadout: Vec::new(),
                resolution: ProviderResolution::Unresolved,
            };
        };
        let resolved = self.resolve_owner(window.run_ordinal, raw_source);
        let snapshot = self.actor_snapshot_at(resolved, window.opened_sequence);
        let raw_snapshot = self.actor_snapshot_at(raw_source, window.opened_sequence);
        let selected = snapshot.or(raw_snapshot);
        let owner_resolution = self
            .owner_by_run_and_direct_entity
            .get(&(window.run_ordinal, raw_source))
            .map(|link| link.resolution);
        let resolution = match selected.map(|value| value.kind) {
            Some(ActorKind::Player) if resolved == raw_source => ProviderResolution::DirectPlayer,
            Some(ActorKind::Player) => {
                owner_resolution.unwrap_or(ProviderResolution::DirectSourceOwnerLinkWithinRun)
            }
            Some(_) => ProviderResolution::NonPlayer,
            None => ProviderResolution::Unresolved,
        };
        ProviderKey {
            entity_uuid: Some(resolved),
            kind: selected.map(|value| actor_kind_name(value.kind).to_owned()),
            display_name: selected.and_then(|value| value.display_name.clone()),
            class_id: selected.and_then(|value| value.class_id),
            specialization_id: selected.and_then(|value| value.specialization_id),
            level: selected.and_then(|value| value.level),
            ability_score: selected.and_then(|value| value.ability_score),
            weapon_item_id: selected.and_then(|value| value.weapon_item_id),
            seasonal_score: selected.and_then(|value| value.seasonal_score),
            primary_loadout: selected
                .map(|value| value.primary_loadout.clone())
                .unwrap_or_default(),
            auxiliary_loadout: selected
                .map(|value| value.auxiliary_loadout.clone())
                .unwrap_or_default(),
            resolution,
        }
    }

    fn close_window(&mut self, window_id: u64, sequence: u64, observed_micros: u64) {
        let Some(window) = self.active.remove(&window_id) else {
            return;
        };
        self.active_by_key.remove(&window.key);
        if let Some(instance_id) = window.key.instance_id {
            let instance_key = InstanceTargetKey {
                run_ordinal: window.key.run_ordinal,
                effect_id: window.key.effect_id,
                instance_id,
                target_actor_id: window.key.target_actor_id,
            };
            remove_indexed_window(&mut self.active_by_instance_target, instance_key, window_id);
        }
        remove_indexed_window(
            &mut self.active_by_target_actor,
            window.key.target_actor_id,
            window_id,
        );
        remove_indexed_window(
            &mut self.active_by_recipient_actor,
            window.key.target_actor_id,
            window_id,
        );
        let lifecycle = self
            .lifecycle_by_effect
            .entry(window.key.effect_id)
            .or_default();
        lifecycle.closed_windows = lifecycle.closed_windows.saturating_add(1);
        lifecycle.observed_active_micros = lifecycle
            .observed_active_micros
            .saturating_add(observed_micros.saturating_sub(window.opened_micros));
        self.closed_windows.push(ClosedWindow {
            effect_id: window.key.effect_id,
            run_ordinal: window.key.run_ordinal,
            instance_id: window.key.instance_id,
            raw_source_entity_uuid: window.raw_source_entity_uuid,
            target_entity_uuid: window.target_entity_uuid,
            opened_sequence: window.opened_sequence,
            opened_micros: window.opened_micros,
            closed_sequence: sequence,
            closed_micros: observed_micros,
            packet_origin: window.packet_origin,
        });
    }

    fn close_all(&mut self, sequence: u64, observed_micros: u64) {
        let active = self.active.keys().copied().collect::<Vec<_>>();
        for window_id in active {
            self.close_window(window_id, sequence, observed_micros);
        }
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
                window.expiration_generation,
                window_id,
            )));
        }
    }

    fn expire_before(&mut self, observed_micros: u64, sequence: u64) {
        while let Some(Reverse((expiration, generation, window_id))) =
            self.expirations.peek().copied()
        {
            if expiration > observed_micros {
                break;
            }
            self.expirations.pop();
            let current = self.active.get(&window_id).is_some_and(|window| {
                window.expiration_micros == Some(expiration)
                    && window.expiration_generation == generation
            });
            if current {
                self.close_window(window_id, sequence, expiration);
            }
        }
    }

    fn finish(mut self, rlog: &Path, example_limit: usize) -> SessionReport {
        let final_sequence = self.last_sequence;
        let final_micros = self.last_observed_micros.unwrap_or_default();
        self.close_all(final_sequence, final_micros);

        let mut effects = BTreeMap::<i64, EffectAccumulator>::new();
        let lifecycle_by_effect = std::mem::take(&mut self.lifecycle_by_effect);
        for (effect_id, lifecycle) in lifecycle_by_effect {
            effects.entry(effect_id).or_default().lifecycle = lifecycle;
        }
        for window in &self.closed_windows {
            let provider_key = self.provider_key(window);
            let recipient_scope =
                self.recipient_scope(window.target_entity_uuid, window.opened_sequence);
            let effect = effects.entry(window.effect_id).or_default();
            match recipient_scope {
                RecipientScope::Player => effect.recipient_scope.player += 1,
                RecipientScope::Monster => effect.recipient_scope.monster += 1,
                RecipientScope::Other => effect.recipient_scope.other += 1,
                RecipientScope::Unresolved => effect.recipient_scope.unresolved += 1,
            }
            let provider = effect.providers.entry(provider_key.clone()).or_default();
            provider.windows = provider.windows.saturating_add(1);
            match recipient_scope {
                RecipientScope::Player => {
                    provider.player_recipient_windows =
                        provider.player_recipient_windows.saturating_add(1)
                }
                RecipientScope::Monster => {
                    provider.monster_recipient_windows =
                        provider.monster_recipient_windows.saturating_add(1)
                }
                RecipientScope::Other => {
                    provider.other_recipient_windows =
                        provider.other_recipient_windows.saturating_add(1)
                }
                RecipientScope::Unresolved => {
                    provider.unresolved_recipient_windows =
                        provider.unresolved_recipient_windows.saturating_add(1)
                }
            }
            if let Some(raw_source) = window.raw_source_entity_uuid {
                provider.raw_source_entities.insert(raw_source);
            }
            if effect.examples.len() < example_limit {
                effect.examples.push(WindowExample {
                    run_ordinal: window.run_ordinal,
                    opened_sequence: window.opened_sequence,
                    opened_micros: window.opened_micros,
                    closed_sequence: window.closed_sequence,
                    closed_micros: window.closed_micros,
                    instance_id: window.instance_id,
                    raw_source_entity_uuid: window.raw_source_entity_uuid,
                    resolved_provider_entity_uuid: provider_key.entity_uuid,
                    provider_resolution: provider_key.resolution,
                    provider_class_id: provider_key.class_id,
                    provider_specialization_id: provider_key.specialization_id,
                    target_entity_uuid: window.target_entity_uuid,
                    recipient_scope,
                    packet_origin_source_type_id: window.packet_origin.map(|value| value.0),
                    packet_origin_source_config_id: window.packet_origin.map(|value| value.1),
                });
            }
        }

        for (key, totals) in &self.overlap_damage {
            let effect = effects.entry(key.effect_id).or_default();
            match key.recipient_scope {
                RecipientScope::Player => effect
                    .overlap_damage
                    .player_recipient_outgoing
                    .merge(totals),
                RecipientScope::Monster => {
                    let provider = key
                        .raw_provider_entity_uuid
                        .map(|raw| self.resolve_owner(key.run_ordinal, raw));
                    let damage_source =
                        self.resolve_owner(key.run_ordinal, key.damage_source_entity_uuid);
                    let damage_source_kind = self
                        .actor_snapshot_at(damage_source, self.last_sequence)
                        .map(|snapshot| snapshot.kind);
                    if provider == Some(damage_source) {
                        effect
                            .overlap_damage
                            .monster_incoming_from_provider
                            .merge(totals);
                    } else {
                        match damage_source_kind {
                            Some(ActorKind::Player) => effect
                                .overlap_damage
                                .monster_incoming_from_other_players
                                .merge(totals),
                            Some(_) => effect
                                .overlap_damage
                                .monster_incoming_from_non_players
                                .merge(totals),
                            None => effect
                                .overlap_damage
                                .monster_incoming_from_unresolved_sources
                                .merge(totals),
                        }
                    }
                }
                RecipientScope::Other | RecipientScope::Unresolved => effect
                    .overlap_damage
                    .other_or_unresolved_recipient_incoming
                    .merge(totals),
            }
        }

        let effects = effects
            .into_iter()
            .map(|(effect_id, effect)| EffectReport {
                effect_id,
                lifecycle: effect.lifecycle,
                recipient_scope: effect.recipient_scope,
                providers: effect
                    .providers
                    .into_iter()
                    .map(|(key, value)| ProviderReport {
                        resolved_provider_entity_uuid: key.entity_uuid,
                        provider_kind: key.kind,
                        display_name: key.display_name,
                        class_id: key.class_id,
                        specialization_id: key.specialization_id,
                        level: key.level,
                        ability_score: key.ability_score,
                        weapon_item_id: key.weapon_item_id,
                        seasonal_score: key.seasonal_score,
                        primary_loadout: key.primary_loadout,
                        auxiliary_loadout: key.auxiliary_loadout,
                        resolution: key.resolution,
                        windows: value.windows,
                        player_recipient_windows: value.player_recipient_windows,
                        monster_recipient_windows: value.monster_recipient_windows,
                        other_recipient_windows: value.other_recipient_windows,
                        unresolved_recipient_windows: value.unresolved_recipient_windows,
                        raw_source_entities: value.raw_source_entities.into_iter().collect(),
                    })
                    .collect(),
                overlap_damage: effect.overlap_damage,
                examples: effect.examples,
            })
            .collect();

        SessionReport {
            rlog: rlog.display().to_string(),
            session_id: self.session_id.unwrap_or_default(),
            first_observed_micros: self.first_observed_micros,
            last_observed_micros: self.last_observed_micros,
            run_ordinals_observed: self.maximum_run_ordinal,
            run_boundaries: self.run_boundaries,
            damage_events: self.damage_events,
            candidate_status_events: self.candidate_status_events,
            unmatched_terminal_status_events: self.unmatched_terminal_status_events,
            maximum_simultaneous_candidate_windows: self.maximum_simultaneous_candidate_windows,
            effects,
        }
    }
}

fn remove_indexed_window<K>(index: &mut HashMap<K, BTreeSet<u64>>, key: K, window_id: u64)
where
    K: Eq + std::hash::Hash + Copy,
{
    let remove_key = index.get_mut(&key).is_some_and(|windows| {
        windows.remove(&window_id);
        windows.is_empty()
    });
    if remove_key {
        index.remove(&key);
    }
}

fn integer_attribute(attribute: &EntityAttribute) -> Option<i64> {
    if let Some(EntityAttributeValue::Integer(value)) = &attribute.decoded {
        return Some(*value);
    }
    decode_varint(&attribute.raw_value).and_then(|value| i64::try_from(value).ok())
}

fn decode_varint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return Some(0);
    }
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index >= 10 || index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return (index + 1 == bytes.len()).then_some(value);
        }
    }
    None
}

fn actor_kind_name(kind: ActorKind) -> &'static str {
    match kind {
        ActorKind::Player => "player",
        ActorKind::Monster => "monster",
        ActorKind::Npc => "npc",
        ActorKind::SceneObject => "scene_object",
        ActorKind::Zone => "zone",
        ActorKind::Projectile => "projectile",
        ActorKind::Pet => "pet",
        ActorKind::TrainingDummy => "training_dummy",
        ActorKind::Drop => "drop",
        ActorKind::Field => "field",
        ActorKind::Trap => "trap",
        ActorKind::Collection => "collection",
        ActorKind::StaticObject => "static_object",
        ActorKind::Vehicle => "vehicle",
        ActorKind::Toy => "toy",
        ActorKind::Housing => "housing",
        ActorKind::Unknown(_) => "unknown",
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rDPS provider mechanic audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let candidate_effects = load_candidate_effects(&arguments.candidate_inventory)?;
    let mut reports = Vec::with_capacity(arguments.rlogs.len());
    for path in &arguments.rlogs {
        let file = File::open(path)?;
        let mut reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
        let mut analyzer = Analyzer::new(&candidate_effects);
        while let Some(envelope) = reader.next_event()? {
            analyzer.observe(&envelope)?;
        }
        if reader.summary().is_none() {
            return Err(format!("{} is not a sealed canonical rlog", path.display()).into());
        }
        reports.push(analyzer.finish(path, arguments.example_limit));
    }

    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(
        &mut writer,
        &AuditBundle {
            schema_version: SCHEMA_VERSION,
            generated_by: "rlogs-bpsr-rdps-provider-mechanic-audit",
            policy: AuditPolicy {
                packet_events_are_exact: true,
                localization_enables_mechanics: false,
                temporal_overlap_enables_rdps: false,
                unresolved_evidence_is_hidden: false,
                owner_resolution: "exact direct-source links or agreeing owner attributes 90 and 91 observed within the same run ordinal",
                actor_metadata_resolution: "fieldwise cumulative exact ActorEvent observations at or before the status-window opening sequence; absent fields in sparse updates never erase prior evidence, while a class change invalidates class-dependent specialization, weapon, and loadout fields unless the same event re-observes them",
                overlap_deduplication: "each damage event counts once per effect/provider/recipient state even when duplicate status instances overlap",
                runtime_use: "offline research only; never loaded by live capture or reducers",
            },
            candidate_inventory: arguments.candidate_inventory.display().to_string(),
            candidate_effect_count: candidate_effects.len(),
            reports,
        },
    )?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn load_candidate_effects(path: &Path) -> Result<BTreeSet<i64>, Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    candidate_effects_from_value(&value)
}

fn candidate_effects_from_value(
    value: &Value,
) -> Result<BTreeSet<i64>, Box<dyn std::error::Error>> {
    let candidates = value
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or("candidate inventory does not contain a candidates array")?;
    let mut effects = BTreeSet::new();
    for candidate in candidates {
        if let Some(effect_id) = candidate.get("effect_id").and_then(Value::as_i64) {
            effects.insert(effect_id);
        }
        if let Some(effect_ids) = candidate.get("effect_ids").and_then(Value::as_array) {
            for effect_id in effect_ids {
                effects.insert(
                    effect_id
                        .as_i64()
                        .ok_or("candidate effect_ids contains a non-integer value")?,
                );
            }
        }
    }
    if effects.is_empty() {
        return Err("candidate inventory did not contain any effect IDs".into());
    }
    Ok(effects)
}

#[derive(Debug)]
struct Arguments {
    candidate_inventory: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    example_limit: usize,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let candidate_inventory = take_value(&mut values, "--candidate-inventory")?;
    let output = take_value(&mut values, "--output")?;
    let example_limit = take_optional_value(&mut values, "--example-limit")?
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<usize>()
                .map_err(|_| "--example-limit must be a non-negative integer".to_owned())
        })
        .transpose()?
        .unwrap_or(DEFAULT_EXAMPLE_LIMIT);
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".into());
        }
        values.remove(position);
        rlogs.push(PathBuf::from(values.remove(position)));
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        candidate_inventory: candidate_inventory.into(),
        rlogs,
        output: output.into(),
        example_limit,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Err(format!("missing {flag}\n{}", usage()));
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    values.remove(position);
    Ok(values.remove(position))
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    values.remove(position);
    Ok(Some(values.remove(position)))
}

fn usage() -> String {
    "usage: rlogs-bpsr-rdps-provider-mechanic-audit --candidate-inventory <rdps-candidate-inventory.json> --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <audit.json> [--example-limit <count>]".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn actor_event(
        class_id: Option<i32>,
        specialization_id: Option<i32>,
        weapon_item_id: Option<i64>,
        primary_loadout: Vec<ActorLoadoutSlot>,
    ) -> ActorEvent {
        ActorEvent {
            actor: EntityRef {
                actor_id: rlogs_events::ActorId(2),
                entity_uuid: rlogs_events::EntityUuid(40_581_726_848),
            },
            state: rlogs_events::ActorState::Updated,
            entity_type_id: 10,
            kind: ActorKind::Player,
            character_id: None,
            monster_id: None,
            display_name: None,
            class_id,
            specialization_id,
            level: None,
            ability_score: None,
            weapon_item_id,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout,
            auxiliary_loadout: Vec::new(),
            loadout_observation: Default::default(),
        }
    }

    fn loadout(slot_id: i32, ability_id: i64, item_id: i64, tier: u32) -> ActorLoadoutSlot {
        ActorLoadoutSlot {
            slot_id,
            ability_id: Some(ability_id),
            item_id: Some(item_id),
            tier: Some(tier),
        }
    }

    #[test]
    fn candidate_inventory_accepts_scalar_and_array_effect_ids() {
        let effects = candidate_effects_from_value(&json!({
            "candidates": [
                {"effect_id": 7},
                {"effect_ids": [8, 9, 8]},
                {"effect_ids": []}
            ]
        }))
        .expect("mixed inventory should parse");
        assert_eq!(effects, BTreeSet::from([7, 8, 9]));
    }

    #[test]
    fn candidate_inventory_rejects_non_integer_array_values() {
        assert!(
            candidate_effects_from_value(&json!({
                "candidates": [{"effect_ids": [8, "9"]}]
            }))
            .is_err()
        );
    }

    #[test]
    fn sparse_actor_updates_do_not_erase_exact_prior_loadout_evidence() {
        let full = actor_event(
            Some(13),
            Some(120),
            Some(2_001_509),
            vec![loadout(7, 3_921, 3_000_011, 5)],
        );
        let first = merge_actor_snapshot(None, 203, &full);
        let sparse = actor_event(Some(13), None, None, Vec::new());
        let merged = merge_actor_snapshot(Some(&first), 10_731, &sparse);

        assert_eq!(merged.sequence, 10_731);
        assert_eq!(merged.specialization_id, Some(120));
        assert_eq!(merged.weapon_item_id, Some(2_001_509));
        assert_eq!(
            merged.primary_loadout,
            vec![ObservedLoadoutSlot {
                slot_id: 7,
                ability_id: Some(3_921),
                item_id: Some(3_000_011),
                tier: Some(5),
            }]
        );
    }

    #[test]
    fn newly_observed_nonempty_loadout_replaces_prior_loadout_at_that_sequence() {
        let first = merge_actor_snapshot(
            None,
            203,
            &actor_event(
                Some(13),
                Some(120),
                Some(2_001_509),
                vec![loadout(7, 3_921, 3_000_011, 5)],
            ),
        );
        let changed = merge_actor_snapshot(
            Some(&first),
            300,
            &actor_event(
                Some(13),
                Some(120),
                Some(2_001_509),
                vec![loadout(7, 3_971, 3_000_123, 4)],
            ),
        );

        assert_eq!(changed.primary_loadout[0].ability_id, Some(3_971));
        assert_eq!(changed.primary_loadout[0].tier, Some(4));
    }

    #[test]
    fn class_change_invalidates_unobserved_class_dependent_fields() {
        let first = merge_actor_snapshot(
            None,
            203,
            &actor_event(
                Some(13),
                Some(120),
                Some(2_001_509),
                vec![loadout(7, 3_921, 3_000_011, 5)],
            ),
        );
        let changed = merge_actor_snapshot(
            Some(&first),
            400,
            &actor_event(Some(11), None, None, Vec::new()),
        );

        assert_eq!(changed.class_id, Some(11));
        assert_eq!(changed.specialization_id, None);
        assert_eq!(changed.weapon_item_id, None);
        assert!(changed.primary_loadout.is_empty());
        assert!(changed.auxiliary_loadout.is_empty());
    }

    #[test]
    fn paired_owner_attributes_resolve_a_summon_within_the_current_run() {
        let mut analyzer = Analyzer::new(&BTreeSet::new());
        analyzer.current_run_ordinal = 3;
        let owner = 40_581_726_848_i64;
        let raw = vec![128, 133, 240, 150, 151, 1];
        analyzer.observe_owner_attributes(&EntityAttributeEvent {
            actor: EntityRef {
                actor_id: rlogs_events::ActorId(186),
                entity_uuid: rlogs_events::EntityUuid(491_584),
            },
            update_kind: rlogs_events::EntityAttributeUpdateKind::Snapshot,
            ownership: Some(rlogs_events::ActorOwnershipUpdate::Confirmed {
                owner_entity_uuid: rlogs_events::EntityUuid(owner),
            }),
            attributes: SUMMON_OWNER_ATTRIBUTE_IDS
                .into_iter()
                .map(|attribute_id| EntityAttribute {
                    attribute_id,
                    raw_value: raw.clone(),
                    decoded: None,
                })
                .collect(),
        });

        assert_eq!(decode_varint(&raw), Some(owner as u64));
        assert_eq!(analyzer.resolve_owner(3, 491_584), owner);
        assert_eq!(analyzer.resolve_owner(2, 491_584), 491_584);
        assert_eq!(
            analyzer
                .owner_by_run_and_direct_entity
                .get(&(3, 491_584))
                .map(|link| link.resolution),
            Some(ProviderResolution::PairedOwnerAttributesWithinRun)
        );
    }
}
