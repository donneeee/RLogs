use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{
    CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, StatusEffectInstanceId,
    StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const BREAKING_STAGE_ATTRIBUTE_ID: i32 = 455;
const GENERAL_DAMAGE_ATTRIBUTE_ID: i32 = 12_670;
const BREAKING_STAGE: u64 = 0;
const BREAK_END_STAGE: u64 = 1;
// 2207121 is the short Heroic Melody Resilience Break Efficiency child.
// 2207122 (`断章_BK增伤`) is the separate child that carries the documented
// +15% damage against a Resilience Broken target.  Damage counterfactuals must
// follow the latter; treating 2207121 as the damage window mixes two distinct
// branches of Severed Chapter and produces a false negative formula audit.
const SEVERED_CHAPTER_RECIPIENT_EFFECT_ID: i64 = 2_207_122;
const SCHEMA_VERSION: u16 = 4;
const EXAMPLE_LIMIT: usize = 24;
const COMPANION_PROXIMITY_MICROS: u64 = 250_000;

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WindowKey {
    recipient_uuid: i64,
    instance_id: Option<StatusEffectInstanceId>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveWindow {
    provider_uuid: Option<i64>,
    opened_sequence: u64,
    expires_micros: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetStage {
    Breaking,
    BreakEnded,
    Other(u64),
}

#[derive(Debug, Clone, Default, Serialize)]
struct DamageBucket {
    events: u64,
    amount: i64,
    external_provider_events: u64,
    external_provider_amount: i64,
}

impl DamageBucket {
    fn record(&mut self, amount: i64, external_provider: bool) {
        self.events = self.events.saturating_add(1);
        self.amount = self.amount.saturating_add(amount);
        if external_provider {
            self.external_provider_events = self.external_provider_events.saturating_add(1);
            self.external_provider_amount = self.external_provider_amount.saturating_add(amount);
        }
    }
}

#[derive(Debug, Default)]
struct SessionAccumulator {
    stage_by_target: HashMap<i64, TargetStage>,
    general_damage_by_actor: HashMap<i64, i64>,
    active_windows: HashMap<WindowKey, ActiveWindow>,
    attribute_updates: u64,
    snapshot_updates: u64,
    delta_updates: u64,
    raw_stage_values: BTreeMap<u64, u64>,
    stage_transitions: BTreeMap<String, u64>,
    status_opened: u64,
    status_closed: u64,
    status_cross_actor_opened: u64,
    damage_during_recipient_window: u64,
    breaking: DamageBucket,
    break_ended: DamageBucket,
    unknown: DamageBucket,
    other: DamageBucket,
    breaking_unique_external_provider: DamageBucket,
    breaking_multiple_external_providers: DamageBucket,
    breaking_self_or_missing_provider: DamageBucket,
    breaking_unique_external_by_provider_uuid: BTreeMap<i64, DamageBucket>,
    breaking_unique_external_general_damage_values: BTreeMap<i64, DamageBucket>,
    breaking_unique_external_general_damage_missing: DamageBucket,
    breaking_active_by_context: HashMap<DamageContext, ContextDamage>,
    breaking_inactive_by_context: HashMap<DamageContext, ContextDamage>,
    transition_examples: Vec<TransitionExample>,
    damage_examples: Vec<DamageExample>,
    lifecycle_anchors: Vec<LifecycleAnchor>,
    entity_attribute_observations: Vec<EntityAttributeObservation>,
    temporary_attribute_observations: Vec<TemporaryAttributeObservation>,
    status_observations: Vec<StatusObservation>,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_by: &'static str,
    policy: Policy,
    sessions: Vec<SessionReport>,
    aggregate: Aggregate,
}

#[derive(Debug, Serialize)]
struct Policy {
    breaking_stage_attribute_id: i32,
    severed_chapter_recipient_effect_id: i64,
    enum_contract: BTreeMap<u64, &'static str>,
    absent_target_state: &'static str,
    unresolved_evidence_hidden: bool,
    runtime_authority: bool,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    client_build: String,
    attribute_updates: u64,
    snapshot_updates: u64,
    delta_updates: u64,
    raw_stage_values: Vec<ValueCount>,
    stage_transitions: Vec<TransitionCount>,
    status_opened: u64,
    status_closed: u64,
    status_cross_actor_opened: u64,
    damage_during_recipient_window: u64,
    breaking: DamageBucket,
    break_ended: DamageBucket,
    unknown: DamageBucket,
    other: DamageBucket,
    breaking_unique_external_provider: DamageBucket,
    breaking_multiple_external_providers: DamageBucket,
    breaking_self_or_missing_provider: DamageBucket,
    breaking_unique_external_by_provider_uuid: BTreeMap<i64, DamageBucket>,
    breaking_unique_external_general_damage_values: BTreeMap<i64, DamageBucket>,
    breaking_unique_external_general_damage_missing: DamageBucket,
    breaking_counterfactual_inventory: CounterfactualInventory,
    transition_examples: Vec<TransitionExample>,
    damage_examples: Vec<DamageExample>,
    companion_inventory: CompanionInventory,
}

#[derive(Debug, Default, Serialize)]
struct Aggregate {
    sessions: u64,
    attribute_updates: u64,
    raw_stage_values: BTreeMap<u64, u64>,
    status_opened: u64,
    status_cross_actor_opened: u64,
    damage_during_recipient_window: u64,
    breaking: DamageBucket,
    break_ended: DamageBucket,
    unknown: DamageBucket,
    other: DamageBucket,
    breaking_unique_external_provider: DamageBucket,
    breaking_multiple_external_providers: DamageBucket,
    breaking_self_or_missing_provider: DamageBucket,
    breaking_unique_external_by_provider_uuid: BTreeMap<i64, DamageBucket>,
    breaking_unique_external_general_damage_values: BTreeMap<i64, DamageBucket>,
    breaking_unique_external_general_damage_missing: DamageBucket,
    breaking_effect_active: DamageBucket,
    breaking_effect_inactive: DamageBucket,
    strict_overlap_contexts: u64,
    companion_entity_attribute_ids: BTreeMap<i32, u64>,
    companion_temporary_attribute_ids: BTreeMap<i32, u64>,
    companion_status_effect_ids: BTreeMap<i64, u64>,
}

#[derive(Debug, Clone)]
struct LifecycleAnchor {
    sequence: u64,
    observed_micros: u64,
    recipient_uuid: i64,
    provider_uuid: Option<i64>,
    lifecycle: &'static str,
}

#[derive(Debug, Clone)]
struct EntityAttributeObservation {
    sequence: u64,
    observed_micros: u64,
    actor_uuid: i64,
    update_kind: EntityAttributeUpdateKind,
    attribute_id: i32,
    raw_hex: String,
    decoded: Option<EntityAttributeValue>,
}

#[derive(Debug, Clone)]
struct TemporaryAttributeObservation {
    sequence: u64,
    observed_micros: u64,
    actor_uuid: i64,
    update_kind: EntityAttributeUpdateKind,
    attribute_id: i32,
    value: i32,
}

#[derive(Debug, Clone)]
struct StatusObservation {
    sequence: u64,
    observed_micros: u64,
    target_uuid: i64,
    source_uuid: Option<i64>,
    effect_id: i64,
    state: StatusState,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
struct CompanionInventory {
    proximity_micros: u64,
    lifecycle_anchors: u64,
    entity_attributes: Vec<AttributeCompanion>,
    temporary_attributes: Vec<TemporaryAttributeCompanion>,
    statuses: Vec<StatusCompanion>,
    examples: Vec<CompanionExample>,
    interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct AttributeCompanion {
    attribute_id: i32,
    observations: u64,
}

#[derive(Debug, Serialize)]
struct TemporaryAttributeCompanion {
    attribute_id: i32,
    observations: u64,
}

#[derive(Debug, Serialize)]
struct StatusCompanion {
    effect_id: i64,
    observations: u64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CompanionExample {
    EntityAttribute {
        anchor_sequence: u64,
        anchor_lifecycle: &'static str,
        anchor_provider_uuid: Option<i64>,
        sequence: u64,
        offset_micros: i64,
        actor_uuid: i64,
        update_kind: EntityAttributeUpdateKind,
        attribute_id: i32,
        raw_hex: String,
        decoded: Option<EntityAttributeValue>,
    },
    TemporaryAttribute {
        anchor_sequence: u64,
        anchor_lifecycle: &'static str,
        anchor_provider_uuid: Option<i64>,
        sequence: u64,
        offset_micros: i64,
        actor_uuid: i64,
        update_kind: EntityAttributeUpdateKind,
        attribute_id: i32,
        value: i32,
    },
    Status {
        anchor_sequence: u64,
        anchor_lifecycle: &'static str,
        anchor_provider_uuid: Option<i64>,
        sequence: u64,
        offset_micros: i64,
        target_uuid: i64,
        source_uuid: Option<i64>,
        effect_id: i64,
        state: StatusState,
        origin_source_type_id: Option<i32>,
        origin_source_config_id: Option<i64>,
        stacks: Option<u32>,
        duration_millis: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
struct DamageContext {
    source_uuid: i64,
    target_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    blocked: Option<bool>,
    periodic: Option<bool>,
    owner_id: Option<i32>,
    passive_uuid: Option<u32>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    type_flags: Option<i32>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ContextDamage {
    events: u64,
    amount: i64,
}

#[derive(Debug, Serialize)]
struct CounterfactualInventory {
    effect_active: DamageBucket,
    effect_inactive: DamageBucket,
    strict_overlap_contexts: u64,
    strict_overlap_active_events: u64,
    strict_overlap_inactive_events: u64,
    strict_overlap_examples: Vec<ContextComparison>,
    interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct ContextComparison {
    context: DamageContext,
    active_events: u64,
    active_amount: i64,
    inactive_events: u64,
    inactive_amount: i64,
    mean_active_to_inactive_basis_points: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ValueCount {
    value: u64,
    meaning: &'static str,
    updates: u64,
}

#[derive(Debug, Serialize)]
struct TransitionCount {
    transition: String,
    updates: u64,
}

#[derive(Debug, Clone, Serialize)]
struct TransitionExample {
    sequence: u64,
    observed_micros: u64,
    target_uuid: i64,
    update_kind: EntityAttributeUpdateKind,
    previous: &'static str,
    next_value: u64,
    next: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct DamageExample {
    sequence: u64,
    observed_micros: u64,
    provider_uuid: Option<i64>,
    recipient_uuid: i64,
    target_uuid: i64,
    target_stage: &'static str,
    amount: i64,
    ability_id: Option<i64>,
    window_opened_sequence: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Severed Chapter breaking proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut reports = Vec::new();
    let mut aggregate = Aggregate::default();

    for path in arguments.rlogs {
        let report = audit_session(path)?;
        aggregate.sessions = aggregate.sessions.saturating_add(1);
        aggregate.attribute_updates = aggregate
            .attribute_updates
            .saturating_add(report.attribute_updates);
        for value in &report.raw_stage_values {
            *aggregate.raw_stage_values.entry(value.value).or_default() += value.updates;
        }
        aggregate.status_opened = aggregate.status_opened.saturating_add(report.status_opened);
        aggregate.status_cross_actor_opened = aggregate
            .status_cross_actor_opened
            .saturating_add(report.status_cross_actor_opened);
        aggregate.damage_during_recipient_window = aggregate
            .damage_during_recipient_window
            .saturating_add(report.damage_during_recipient_window);
        merge_bucket(&mut aggregate.breaking, &report.breaking);
        merge_bucket(&mut aggregate.break_ended, &report.break_ended);
        merge_bucket(&mut aggregate.unknown, &report.unknown);
        merge_bucket(&mut aggregate.other, &report.other);
        merge_bucket(
            &mut aggregate.breaking_unique_external_provider,
            &report.breaking_unique_external_provider,
        );
        merge_bucket(
            &mut aggregate.breaking_multiple_external_providers,
            &report.breaking_multiple_external_providers,
        );
        merge_bucket(
            &mut aggregate.breaking_self_or_missing_provider,
            &report.breaking_self_or_missing_provider,
        );
        for (provider_uuid, bucket) in &report.breaking_unique_external_by_provider_uuid {
            merge_bucket(
                aggregate
                    .breaking_unique_external_by_provider_uuid
                    .entry(*provider_uuid)
                    .or_default(),
                bucket,
            );
        }
        for (value, bucket) in &report.breaking_unique_external_general_damage_values {
            merge_bucket(
                aggregate
                    .breaking_unique_external_general_damage_values
                    .entry(*value)
                    .or_default(),
                bucket,
            );
        }
        merge_bucket(
            &mut aggregate.breaking_unique_external_general_damage_missing,
            &report.breaking_unique_external_general_damage_missing,
        );
        merge_bucket(
            &mut aggregate.breaking_effect_active,
            &report.breaking_counterfactual_inventory.effect_active,
        );
        merge_bucket(
            &mut aggregate.breaking_effect_inactive,
            &report.breaking_counterfactual_inventory.effect_inactive,
        );
        aggregate.strict_overlap_contexts = aggregate.strict_overlap_contexts.saturating_add(
            report
                .breaking_counterfactual_inventory
                .strict_overlap_contexts,
        );
        for companion in &report.companion_inventory.entity_attributes {
            *aggregate
                .companion_entity_attribute_ids
                .entry(companion.attribute_id)
                .or_default() += companion.observations;
        }
        for companion in &report.companion_inventory.temporary_attributes {
            *aggregate
                .companion_temporary_attribute_ids
                .entry(companion.attribute_id)
                .or_default() += companion.observations;
        }
        for companion in &report.companion_inventory.statuses {
            *aggregate
                .companion_status_effect_ids
                .entry(companion.effect_id)
                .or_default() += companion.observations;
        }
        reports.push(report);
    }

    let report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-severed-chapter-breaking-proof",
        policy: Policy {
            breaking_stage_attribute_id: BREAKING_STAGE_ATTRIBUTE_ID,
            severed_chapter_recipient_effect_id: SEVERED_CHAPTER_RECIPIENT_EFFECT_ID,
            enum_contract: BTreeMap::from([
                (BREAKING_STAGE, "breaking"),
                (BREAK_END_STAGE, "break_ended"),
            ]),
            absent_target_state: "unknown; never inferred as breaking",
            unresolved_evidence_hidden: false,
            runtime_authority: false,
        },
        sessions: reports,
        aggregate,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn audit_session(path: PathBuf) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(&path)?), RlogLimits::default())?;
    let session_id = reader.header().session_id.clone();
    let client_build = reader.header().region.client_build.clone();
    let mut audit = SessionAccumulator::default();

    while let Some(envelope) = reader.next_event()? {
        audit.active_windows.retain(|_, window| {
            window
                .expires_micros
                .is_none_or(|expires| expires > envelope.time.observed_micros)
        });
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::EntityAttributes(event) => {
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    audit
                        .general_damage_by_actor
                        .remove(&event.actor.entity_uuid.0);
                }
                for attribute in &event.attributes {
                    audit
                        .entity_attribute_observations
                        .push(EntityAttributeObservation {
                            sequence: envelope.sequence,
                            observed_micros: envelope.time.observed_micros,
                            actor_uuid: event.actor.entity_uuid.0,
                            update_kind: event.update_kind,
                            attribute_id: attribute.attribute_id,
                            raw_hex: hex(&attribute.raw_value),
                            decoded: attribute.decoded.clone(),
                        });
                    if attribute.attribute_id == GENERAL_DAMAGE_ATTRIBUTE_ID
                        && let Some(value) = decode_varint(&attribute.raw_value)
                        && let Ok(value) = i64::try_from(value)
                    {
                        audit
                            .general_damage_by_actor
                            .insert(event.actor.entity_uuid.0, value);
                    }
                    if attribute.attribute_id != BREAKING_STAGE_ATTRIBUTE_ID {
                        continue;
                    }
                    let Some(value) = decode_varint(&attribute.raw_value) else {
                        continue;
                    };
                    audit.attribute_updates = audit.attribute_updates.saturating_add(1);
                    match event.update_kind {
                        EntityAttributeUpdateKind::Snapshot => {
                            audit.snapshot_updates = audit.snapshot_updates.saturating_add(1)
                        }
                        EntityAttributeUpdateKind::Delta => {
                            audit.delta_updates = audit.delta_updates.saturating_add(1)
                        }
                        EntityAttributeUpdateKind::Unknown => {}
                    }
                    *audit.raw_stage_values.entry(value).or_default() += 1;
                    let target_uuid = event.actor.entity_uuid.0;
                    let next = target_stage(value);
                    let previous = audit.stage_by_target.insert(target_uuid, next);
                    let transition = format!(
                        "{}->{}",
                        previous.map(stage_name).unwrap_or("unknown"),
                        stage_name(next)
                    );
                    *audit.stage_transitions.entry(transition).or_default() += 1;
                    push_limited(
                        &mut audit.transition_examples,
                        TransitionExample {
                            sequence: envelope.sequence,
                            observed_micros: envelope.time.observed_micros,
                            target_uuid,
                            update_kind: event.update_kind,
                            previous: previous.map(stage_name).unwrap_or("unknown"),
                            next_value: value,
                            next: stage_name(next),
                        },
                    );
                }
            }
            TimelineEventKind::TemporaryAttributes(event) => {
                for attribute in &event.attributes {
                    audit
                        .temporary_attribute_observations
                        .push(TemporaryAttributeObservation {
                            sequence: envelope.sequence,
                            observed_micros: envelope.time.observed_micros,
                            actor_uuid: event.actor.entity_uuid.0,
                            update_kind: event.update_kind,
                            attribute_id: attribute.id,
                            value: attribute.value,
                        });
                }
            }
            TimelineEventKind::Status(status) => {
                audit.status_observations.push(StatusObservation {
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    target_uuid: status.target.entity_uuid.0,
                    source_uuid: status.source.map(|source| source.entity_uuid.0),
                    effect_id: status.effect.0,
                    state: status.state,
                    origin_source_type_id: status.origin.map(|origin| origin.source_type_id),
                    origin_source_config_id: status.origin.map(|origin| origin.source_config_id),
                    stacks: status.stacks,
                    duration_millis: status.duration_millis,
                });
                if status.effect.0 != SEVERED_CHAPTER_RECIPIENT_EFFECT_ID {
                    continue;
                }
                let key = WindowKey {
                    recipient_uuid: status.target.entity_uuid.0,
                    instance_id: status.instance_id,
                };
                let terminal = matches!(status.state, StatusState::Removed)
                    || matches!(status.state, StatusState::Consumed) && status.stacks == Some(0);
                if terminal {
                    audit.status_closed = audit.status_closed.saturating_add(1);
                    audit.lifecycle_anchors.push(LifecycleAnchor {
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        recipient_uuid: key.recipient_uuid,
                        provider_uuid: status.source.map(|source| source.entity_uuid.0),
                        lifecycle: "closed",
                    });
                    audit.active_windows.remove(&key);
                } else if let Some(window) = audit.active_windows.get_mut(&key) {
                    window.provider_uuid = status
                        .source
                        .map(|source| source.entity_uuid.0)
                        .or(window.provider_uuid);
                    window.expires_micros =
                        expiration_micros(envelope.time.observed_micros, status.duration_millis);
                } else {
                    audit.status_opened = audit.status_opened.saturating_add(1);
                    let provider_uuid = status.source.map(|source| source.entity_uuid.0);
                    audit.lifecycle_anchors.push(LifecycleAnchor {
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        recipient_uuid: key.recipient_uuid,
                        provider_uuid,
                        lifecycle: "opened",
                    });
                    if provider_uuid.is_some_and(|provider| provider != key.recipient_uuid) {
                        audit.status_cross_actor_opened =
                            audit.status_cross_actor_opened.saturating_add(1);
                    }
                    audit.active_windows.insert(
                        key,
                        ActiveWindow {
                            provider_uuid,
                            opened_sequence: envelope.sequence,
                            expires_micros: expiration_micros(
                                envelope.time.observed_micros,
                                status.duration_millis,
                            ),
                        },
                    );
                }
            }
            TimelineEventKind::Damage(damage) => {
                let recipient_uuid = damage.source.entity_uuid.0;
                let (window, external_providers) = {
                    let matching = audit
                        .active_windows
                        .iter()
                        .filter(|(key, _)| key.recipient_uuid == recipient_uuid)
                        .map(|(_, window)| *window)
                        .collect::<Vec<_>>();
                    let external_providers = matching
                        .iter()
                        .filter_map(|window| window.provider_uuid)
                        .filter(|provider_uuid| *provider_uuid != recipient_uuid)
                        .collect::<BTreeSet<_>>();
                    let latest = matching
                        .into_iter()
                        .max_by_key(|window| window.opened_sequence);
                    (latest, external_providers)
                };
                let target_uuid = damage.target.entity_uuid.0;
                let stage = audit.stage_by_target.get(&target_uuid).copied();
                if stage == Some(TargetStage::Breaking) {
                    let context = DamageContext {
                        source_uuid: recipient_uuid,
                        target_uuid,
                        ability_id: damage.ability.map(|ability| ability.0),
                        hit_event_id: damage.hit_event_id,
                        damage_source: damage.damage_source,
                        damage_type: damage.damage_type,
                        critical: damage.flags.critical,
                        lucky: damage.flags.lucky,
                        blocked: damage.flags.blocked,
                        periodic: damage.flags.periodic,
                        owner_id: damage.packet.owner_id,
                        passive_uuid: damage.packet.passive_uuid,
                        property: damage.packet.property,
                        damage_mode: damage.packet.damage_mode,
                        type_flags: damage.packet.type_flags,
                    };
                    let contexts = if window.is_some() {
                        &mut audit.breaking_active_by_context
                    } else {
                        &mut audit.breaking_inactive_by_context
                    };
                    let totals = contexts.entry(context).or_default();
                    totals.events = totals.events.saturating_add(1);
                    totals.amount = totals.amount.saturating_add(damage.amount);
                }
                let Some(window) = window else {
                    continue;
                };
                audit.damage_during_recipient_window =
                    audit.damage_during_recipient_window.saturating_add(1);
                let external_provider = window
                    .provider_uuid
                    .is_some_and(|provider| provider != recipient_uuid);
                match stage {
                    Some(TargetStage::Breaking) => {
                        audit.breaking.record(damage.amount, external_provider);
                        match external_providers.len() {
                            0 => audit
                                .breaking_self_or_missing_provider
                                .record(damage.amount, false),
                            1 => {
                                audit
                                    .breaking_unique_external_provider
                                    .record(damage.amount, true);
                                let provider_uuid = *external_providers
                                    .first()
                                    .expect("cardinality-one set has one provider");
                                audit
                                    .breaking_unique_external_by_provider_uuid
                                    .entry(provider_uuid)
                                    .or_default()
                                    .record(damage.amount, true);
                                if let Some(current_general_damage) =
                                    audit.general_damage_by_actor.get(&recipient_uuid).copied()
                                {
                                    audit
                                        .breaking_unique_external_general_damage_values
                                        .entry(current_general_damage)
                                        .or_default()
                                        .record(damage.amount, true);
                                } else {
                                    audit
                                        .breaking_unique_external_general_damage_missing
                                        .record(damage.amount, true);
                                }
                            }
                            _ => audit
                                .breaking_multiple_external_providers
                                .record(damage.amount, true),
                        }
                    }
                    Some(TargetStage::BreakEnded) => {
                        audit.break_ended.record(damage.amount, external_provider)
                    }
                    Some(TargetStage::Other(_)) => {
                        audit.other.record(damage.amount, external_provider)
                    }
                    None => audit.unknown.record(damage.amount, external_provider),
                }
                push_limited(
                    &mut audit.damage_examples,
                    DamageExample {
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        provider_uuid: window.provider_uuid,
                        recipient_uuid,
                        target_uuid,
                        target_stage: stage.map(stage_name).unwrap_or("unknown"),
                        amount: damage.amount,
                        ability_id: damage.ability.map(|ability| ability.0),
                        window_opened_sequence: window.opened_sequence,
                    },
                );
            }
            _ => {}
        }
    }

    let breaking_counterfactual_inventory = counterfactual_inventory(
        &audit.breaking_active_by_context,
        &audit.breaking_inactive_by_context,
    );
    let companion_inventory = companion_inventory(&audit);

    Ok(SessionReport {
        rlog: path.display().to_string(),
        session_id,
        client_build,
        attribute_updates: audit.attribute_updates,
        snapshot_updates: audit.snapshot_updates,
        delta_updates: audit.delta_updates,
        raw_stage_values: audit
            .raw_stage_values
            .into_iter()
            .map(|(value, updates)| ValueCount {
                value,
                meaning: stage_name(target_stage(value)),
                updates,
            })
            .collect(),
        stage_transitions: audit
            .stage_transitions
            .into_iter()
            .map(|(transition, updates)| TransitionCount {
                transition,
                updates,
            })
            .collect(),
        status_opened: audit.status_opened,
        status_closed: audit.status_closed,
        status_cross_actor_opened: audit.status_cross_actor_opened,
        damage_during_recipient_window: audit.damage_during_recipient_window,
        breaking: audit.breaking,
        break_ended: audit.break_ended,
        unknown: audit.unknown,
        other: audit.other,
        breaking_unique_external_provider: audit.breaking_unique_external_provider,
        breaking_multiple_external_providers: audit.breaking_multiple_external_providers,
        breaking_self_or_missing_provider: audit.breaking_self_or_missing_provider,
        breaking_unique_external_by_provider_uuid: audit.breaking_unique_external_by_provider_uuid,
        breaking_unique_external_general_damage_values: audit
            .breaking_unique_external_general_damage_values,
        breaking_unique_external_general_damage_missing: audit
            .breaking_unique_external_general_damage_missing,
        breaking_counterfactual_inventory,
        transition_examples: audit.transition_examples,
        damage_examples: audit.damage_examples,
        companion_inventory,
    })
}

fn companion_inventory(audit: &SessionAccumulator) -> CompanionInventory {
    let mut entity_attribute_counts = BTreeMap::<i32, u64>::new();
    let mut temporary_attribute_counts = BTreeMap::<i32, u64>::new();
    let mut status_counts = BTreeMap::<i64, u64>::new();
    let mut examples = Vec::new();

    for anchor in &audit.lifecycle_anchors {
        for observation in &audit.entity_attribute_observations {
            if observation.actor_uuid != anchor.recipient_uuid
                || absolute_offset_micros(anchor.observed_micros, observation.observed_micros)
                    > COMPANION_PROXIMITY_MICROS
            {
                continue;
            }
            *entity_attribute_counts
                .entry(observation.attribute_id)
                .or_default() += 1;
            push_limited(
                &mut examples,
                CompanionExample::EntityAttribute {
                    anchor_sequence: anchor.sequence,
                    anchor_lifecycle: anchor.lifecycle,
                    anchor_provider_uuid: anchor.provider_uuid,
                    sequence: observation.sequence,
                    offset_micros: signed_offset_micros(
                        anchor.observed_micros,
                        observation.observed_micros,
                    ),
                    actor_uuid: observation.actor_uuid,
                    update_kind: observation.update_kind,
                    attribute_id: observation.attribute_id,
                    raw_hex: observation.raw_hex.clone(),
                    decoded: observation.decoded.clone(),
                },
            );
        }
        for observation in &audit.temporary_attribute_observations {
            if observation.actor_uuid != anchor.recipient_uuid
                || absolute_offset_micros(anchor.observed_micros, observation.observed_micros)
                    > COMPANION_PROXIMITY_MICROS
            {
                continue;
            }
            *temporary_attribute_counts
                .entry(observation.attribute_id)
                .or_default() += 1;
            push_limited(
                &mut examples,
                CompanionExample::TemporaryAttribute {
                    anchor_sequence: anchor.sequence,
                    anchor_lifecycle: anchor.lifecycle,
                    anchor_provider_uuid: anchor.provider_uuid,
                    sequence: observation.sequence,
                    offset_micros: signed_offset_micros(
                        anchor.observed_micros,
                        observation.observed_micros,
                    ),
                    actor_uuid: observation.actor_uuid,
                    update_kind: observation.update_kind,
                    attribute_id: observation.attribute_id,
                    value: observation.value,
                },
            );
        }
        for observation in &audit.status_observations {
            if observation.effect_id == SEVERED_CHAPTER_RECIPIENT_EFFECT_ID
                || observation.target_uuid != anchor.recipient_uuid
                || absolute_offset_micros(anchor.observed_micros, observation.observed_micros)
                    > COMPANION_PROXIMITY_MICROS
            {
                continue;
            }
            *status_counts.entry(observation.effect_id).or_default() += 1;
            push_limited(
                &mut examples,
                CompanionExample::Status {
                    anchor_sequence: anchor.sequence,
                    anchor_lifecycle: anchor.lifecycle,
                    anchor_provider_uuid: anchor.provider_uuid,
                    sequence: observation.sequence,
                    offset_micros: signed_offset_micros(
                        anchor.observed_micros,
                        observation.observed_micros,
                    ),
                    target_uuid: observation.target_uuid,
                    source_uuid: observation.source_uuid,
                    effect_id: observation.effect_id,
                    state: observation.state,
                    origin_source_type_id: observation.origin_source_type_id,
                    origin_source_config_id: observation.origin_source_config_id,
                    stacks: observation.stacks,
                    duration_millis: observation.duration_millis,
                },
            );
        }
    }

    CompanionInventory {
        proximity_micros: COMPANION_PROXIMITY_MICROS,
        lifecycle_anchors: audit.lifecycle_anchors.len() as u64,
        entity_attributes: entity_attribute_counts
            .into_iter()
            .map(|(attribute_id, observations)| AttributeCompanion {
                attribute_id,
                observations,
            })
            .collect(),
        temporary_attributes: temporary_attribute_counts
            .into_iter()
            .map(|(attribute_id, observations)| TemporaryAttributeCompanion {
                attribute_id,
                observations,
            })
            .collect(),
        statuses: status_counts
            .into_iter()
            .map(|(effect_id, observations)| StatusCompanion {
                effect_id,
                observations,
            })
            .collect(),
        examples,
        interpretation: "proximity inventory only; every candidate remains non-authoritative until exact semantics and an attributable formula are independently proven",
    }
}

fn counterfactual_inventory(
    active: &HashMap<DamageContext, ContextDamage>,
    inactive: &HashMap<DamageContext, ContextDamage>,
) -> CounterfactualInventory {
    let mut effect_active = DamageBucket::default();
    for totals in active.values() {
        effect_active.events = effect_active.events.saturating_add(totals.events);
        effect_active.amount = effect_active.amount.saturating_add(totals.amount);
    }
    let mut effect_inactive = DamageBucket::default();
    for totals in inactive.values() {
        effect_inactive.events = effect_inactive.events.saturating_add(totals.events);
        effect_inactive.amount = effect_inactive.amount.saturating_add(totals.amount);
    }
    let mut strict_overlap_contexts = 0_u64;
    let mut strict_overlap_active_events = 0_u64;
    let mut strict_overlap_inactive_events = 0_u64;
    let mut strict_overlap_examples = Vec::new();
    for (context, active_totals) in active {
        let Some(inactive_totals) = inactive.get(context) else {
            continue;
        };
        strict_overlap_contexts = strict_overlap_contexts.saturating_add(1);
        strict_overlap_active_events =
            strict_overlap_active_events.saturating_add(active_totals.events);
        strict_overlap_inactive_events =
            strict_overlap_inactive_events.saturating_add(inactive_totals.events);
        push_limited(
            &mut strict_overlap_examples,
            ContextComparison {
                context: *context,
                active_events: active_totals.events,
                active_amount: active_totals.amount,
                inactive_events: inactive_totals.events,
                inactive_amount: inactive_totals.amount,
                mean_active_to_inactive_basis_points: mean_ratio_basis_points(
                    *active_totals,
                    *inactive_totals,
                ),
            },
        );
    }
    strict_overlap_examples.sort_by_key(|example| {
        (
            example.context.ability_id,
            example.context.hit_event_id,
            example.context.source_uuid,
            example.context.target_uuid,
        )
    });
    CounterfactualInventory {
        effect_active,
        effect_inactive,
        strict_overlap_contexts,
        strict_overlap_active_events,
        strict_overlap_inactive_events,
        strict_overlap_examples,
        interpretation: "inventory only; matching packet shape is not formula proof because hidden server variance and unobserved actor state can differ",
    }
}

fn mean_ratio_basis_points(active: ContextDamage, inactive: ContextDamage) -> Option<i64> {
    if active.events == 0 || inactive.events == 0 || inactive.amount <= 0 {
        return None;
    }
    let numerator = i128::from(active.amount)
        .checked_mul(i128::from(inactive.events))?
        .checked_mul(10_000)?;
    let denominator = i128::from(inactive.amount).checked_mul(i128::from(active.events))?;
    i64::try_from(numerator.checked_div(denominator)?).ok()
}

fn merge_bucket(target: &mut DamageBucket, source: &DamageBucket) {
    target.events = target.events.saturating_add(source.events);
    target.amount = target.amount.saturating_add(source.amount);
    target.external_provider_events = target
        .external_provider_events
        .saturating_add(source.external_provider_events);
    target.external_provider_amount = target
        .external_provider_amount
        .saturating_add(source.external_provider_amount);
}

fn target_stage(value: u64) -> TargetStage {
    match value {
        BREAKING_STAGE => TargetStage::Breaking,
        BREAK_END_STAGE => TargetStage::BreakEnded,
        other => TargetStage::Other(other),
    }
}

fn stage_name(stage: TargetStage) -> &'static str {
    match stage {
        TargetStage::Breaking => "breaking",
        TargetStage::BreakEnded => "break_ended",
        TargetStage::Other(_) => "other",
    }
}

fn decode_varint(bytes: &[u8]) -> Option<u64> {
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

fn expiration_micros(observed_micros: u64, duration_millis: Option<u64>) -> Option<u64> {
    duration_millis
        .filter(|duration| *duration > 0)
        .map(|duration| observed_micros.saturating_add(duration.saturating_mul(1_000)))
}

fn absolute_offset_micros(anchor: u64, observation: u64) -> u64 {
    anchor.abs_diff(observation)
}

fn signed_offset_micros(anchor: u64, observation: u64) -> i64 {
    if observation >= anchor {
        i64::try_from(observation - anchor).unwrap_or(i64::MAX)
    } else {
        -i64::try_from(anchor - observation).unwrap_or(i64::MAX)
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn push_limited<T>(values: &mut Vec<T>, value: T) {
    if values.len() < EXAMPLE_LIMIT {
        values.push(value);
    }
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let mut rlogs = Vec::new();
    while let Some(value) = take_optional_value(&mut values, "--rlog") {
        if value.is_empty() {
            return Err("--rlog requires a value".to_owned());
        }
        rlogs.push(PathBuf::from(value));
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments { rlogs, output })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let position = values
        .iter()
        .position(|value| value == flag)
        .ok_or_else(usage)?;
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return Some(OsString::new());
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn usage() -> String {
    "usage: rlogs-bpsr-severed-chapter-breaking-proof --output <report.json> --rlog <sealed.rlog> [--rlog <sealed.rlog> ...]".to_owned()
}
