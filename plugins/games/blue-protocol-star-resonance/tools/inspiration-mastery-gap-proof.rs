use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{
    CanonicalEvent, EntityAttribute, EntityAttributeUpdateKind, EntityAttributeValue,
    EvidenceSource, RunState, TimelineEventKind,
};
use rlogs_game_bpsr::{BpsrDamageProperty, decode_known_entity_attribute_value};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const SCHEMA_VERSION: u16 = 5;
const MASTERY_ATTRIBUTE_ID: i32 = 11_940;
const LIGHT_DAMAGE_ATTRIBUTE_ID: i32 = 13_170;
const INSPIRATION_EFFECT_ID: i64 = 2_202_041;
const CONVERSION_NUMERATOR: i64 = 60;
const CONVERSION_DENOMINATOR: i64 = 100;
const NORMAL_MASTERY_DELTA: i64 = 300;
const FULL_BLOOM_MASTERY_DELTA: i64 = 360;
const EXAMPLE_LIMIT: usize = 24;

#[derive(Debug)]
struct Arguments {
    entity_uuid: i64,
    window_micros: u64,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Default)]
struct ActorState {
    mastery: Option<i64>,
    light_damage: Option<i64>,
    inspiration_instances: u32,
    pending: Option<PendingTransition>,
}

#[derive(Debug, Clone)]
struct PendingTransition {
    run_ordinal: u32,
    mastery_sequence: u64,
    mastery_observed_micros: u64,
    mastery_wire: Option<WireIdentity>,
    mastery_before: i64,
    mastery_after: i64,
    mastery_delta: i64,
    light_before: Option<i64>,
    expected_light_delta: i64,
    inspiration_active: bool,
    damage_events: u64,
    light_damage_events: u64,
    light_damage_amount: i64,
    property_counts: BTreeMap<String, u64>,
    light_damage_by_ability: BTreeMap<i64, DamageAggregate>,
    gap_damage_on_mastery_wire: u64,
    gap_damage_wires: Vec<Option<WireIdentity>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct WireIdentity {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Default, Serialize)]
struct Counters {
    matching_mastery_transitions: u64,
    matching_light_transitions: u64,
    mismatched_light_transitions: u64,
    expired_mastery_transitions: u64,
    superseded_mastery_transitions: u64,
    same_event_light_transitions: u64,
    delayed_light_transitions: u64,
    transitions_with_any_gap_damage: u64,
    transitions_with_gap_light_damage: u64,
    gap_damage_events: u64,
    gap_light_damage_events: u64,
    gap_light_damage_amount: i64,
    gap_light_damage_by_ability: BTreeMap<i64, DamageAggregate>,
    observed_light_transitions_on_same_wire: u64,
    observed_light_transitions_on_distinct_wires: u64,
    gap_damage_on_mastery_wire: u64,
    gap_damage_on_light_wire: u64,
    gap_damage_on_distinct_wire: u64,
}

impl Counters {
    fn add(&mut self, other: &Self) {
        self.matching_mastery_transitions += other.matching_mastery_transitions;
        self.matching_light_transitions += other.matching_light_transitions;
        self.mismatched_light_transitions += other.mismatched_light_transitions;
        self.expired_mastery_transitions += other.expired_mastery_transitions;
        self.superseded_mastery_transitions += other.superseded_mastery_transitions;
        self.same_event_light_transitions += other.same_event_light_transitions;
        self.delayed_light_transitions += other.delayed_light_transitions;
        self.transitions_with_any_gap_damage += other.transitions_with_any_gap_damage;
        self.transitions_with_gap_light_damage += other.transitions_with_gap_light_damage;
        self.gap_damage_events += other.gap_damage_events;
        self.gap_light_damage_events += other.gap_light_damage_events;
        self.gap_light_damage_amount += other.gap_light_damage_amount;
        self.observed_light_transitions_on_same_wire +=
            other.observed_light_transitions_on_same_wire;
        self.observed_light_transitions_on_distinct_wires +=
            other.observed_light_transitions_on_distinct_wires;
        self.gap_damage_on_mastery_wire += other.gap_damage_on_mastery_wire;
        self.gap_damage_on_light_wire += other.gap_damage_on_light_wire;
        self.gap_damage_on_distinct_wire += other.gap_damage_on_distinct_wire;
        for (&ability_id, aggregate) in &other.gap_light_damage_by_ability {
            self.gap_light_damage_by_ability
                .entry(ability_id)
                .or_default()
                .add(aggregate);
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct DamageAggregate {
    events: u64,
    amount: i64,
}

impl DamageAggregate {
    fn add(&mut self, other: &Self) {
        self.events = self.events.saturating_add(other.events);
        self.amount = self.amount.saturating_add(other.amount);
    }
}

#[derive(Debug, Serialize)]
struct TransitionExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    entity_uuid: i64,
    mastery_sequence: u64,
    light_sequence: u64,
    mastery_wire: Option<WireIdentity>,
    light_wire: Option<WireIdentity>,
    same_wire: bool,
    latency_micros: u64,
    mastery_before: i64,
    mastery_after: i64,
    mastery_delta: i64,
    light_before: Option<i64>,
    light_after: i64,
    light_delta: Option<i64>,
    expected_light_delta: i64,
    exact: bool,
    inspiration_active_at_mastery_transition: bool,
    gap_damage_events: u64,
    gap_light_damage_events: u64,
    gap_light_damage_amount: i64,
    gap_damage_property_counts: BTreeMap<String, u64>,
    gap_damage_on_mastery_wire: u64,
    gap_damage_on_light_wire: u64,
    gap_damage_on_distinct_wire: u64,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    counters: Counters,
    transition_boundaries: BTreeMap<u64, u64>,
    exact_examples: Vec<TransitionExample>,
    mismatch_examples: Vec<TransitionExample>,
    gap_light_damage_examples: Vec<GapLightDamageExample>,
}

#[derive(Debug, Serialize)]
struct GapLightDamageExample {
    sequence: u64,
    observed_micros: u64,
    damage_wire: Option<WireIdentity>,
    run_ordinal: u32,
    mastery_sequence: u64,
    mastery_wire: Option<WireIdentity>,
    mastery_before: i64,
    mastery_after: i64,
    serialized_light: Option<i64>,
    logical_light: Option<i64>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    amount: i64,
    normal_value: Option<i64>,
    critical: Option<bool>,
    lucky: Option<bool>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    property: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_total_damage: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: Policy,
    scope: Scope,
    totals: Counters,
    sessions: Vec<SessionReport>,
}

#[derive(Debug, Serialize)]
struct Policy {
    runtime_authority: bool,
    packet_property_is_element_authority: bool,
    latest_light_attribute_is_snapshot_authority: bool,
    unresolved_evidence_is_hidden: bool,
    purpose: &'static str,
}

#[derive(Debug, Serialize)]
struct Scope {
    entity_uuid: i64,
    mastery_attribute_id: i32,
    light_damage_attribute_id: i32,
    light_damage_property_id: i32,
    inspiration_effect_id: i64,
    accepted_mastery_delta_magnitudes: [i64; 2],
    conversion_numerator: i64,
    conversion_denominator: i64,
    transition_window_micros: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Inspiration Mastery gap proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut totals = Counters::default();
    let mut sessions = Vec::new();
    for path in &arguments.rlogs {
        let report = read_session(path, &arguments)?;
        totals.add(&report.counters);
        sessions.push(report);
    }

    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-inspiration-mastery-gap-proof",
        policy: Policy {
            runtime_authority: false,
            packet_property_is_element_authority: true,
            latest_light_attribute_is_snapshot_authority: false,
            unresolved_evidence_is_hidden: false,
            purpose: "Measure damage events between packet-observed Inspiration Mastery transitions and their delayed derived Light Damage transitions before selecting a live snapshot policy.",
        },
        scope: Scope {
            entity_uuid: arguments.entity_uuid,
            mastery_attribute_id: MASTERY_ATTRIBUTE_ID,
            light_damage_attribute_id: LIGHT_DAMAGE_ATTRIBUTE_ID,
            light_damage_property_id: BpsrDamageProperty::Light.protocol_id(),
            inspiration_effect_id: INSPIRATION_EFFECT_ID,
            accepted_mastery_delta_magnitudes: [NORMAL_MASTERY_DELTA, FULL_BLOOM_MASTERY_DELTA],
            conversion_numerator: CONVERSION_NUMERATOR,
            conversion_denominator: CONVERSION_DENOMINATOR,
            transition_window_micros: arguments.window_micros,
        },
        totals,
        sessions,
    };

    if let Some(parent) = arguments.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", arguments.output.display());
    Ok(())
}

fn read_session(
    path: &PathBuf,
    arguments: &Arguments,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut state = ActorState::default();
    let mut counters = Counters::default();
    let mut exact_examples = Vec::new();
    let mut mismatch_examples = Vec::new();
    let mut gap_light_damage_examples = Vec::new();
    let mut transition_boundaries = BTreeMap::new();
    let mut session_id = String::new();
    let mut run_ordinal = 0_u32;

    while let Some(envelope) = reader.next_event()? {
        if session_id.is_empty() {
            session_id.clone_from(&envelope.session_id);
        }
        expire_pending(
            &mut state,
            &mut counters,
            envelope.time.observed_micros,
            arguments.window_micros,
        );
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary {
                state: boundary, ..
            } => match boundary {
                RunState::Entered => {
                    run_ordinal = run_ordinal.saturating_add(1);
                    state = ActorState::default();
                }
                RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::Status(status)
                if status.target.entity_uuid.0 == arguments.entity_uuid
                    && status.effect.0 == INSPIRATION_EFFECT_ID =>
            {
                match status.state {
                    rlogs_events::StatusState::Applied => {
                        state.inspiration_instances = state.inspiration_instances.saturating_add(1)
                    }
                    rlogs_events::StatusState::Removed => {
                        state.inspiration_instances = state.inspiration_instances.saturating_sub(1)
                    }
                    _ => {}
                }
            }
            TimelineEventKind::EntityAttributes(event)
                if event.actor.entity_uuid.0 == arguments.entity_uuid =>
            {
                let mastery_in_event = find_integer(&event.attributes, MASTERY_ATTRIBUTE_ID);
                let light_in_event = find_integer(&event.attributes, LIGHT_DAMAGE_ATTRIBUTE_ID);
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    state.pending = None;
                    state.mastery = mastery_in_event;
                    state.light_damage = light_in_event;
                    continue;
                }

                let mastery_before = state.mastery;
                let light_before = state.light_damage;
                if let Some(value) = mastery_in_event {
                    state.mastery = Some(value);
                }
                if let Some(value) = light_in_event {
                    state.light_damage = Some(value);
                }

                if let (Some(before), Some(after)) = (mastery_before, mastery_in_event)
                    && before != after
                {
                    let delta = after.saturating_sub(before);
                    if [NORMAL_MASTERY_DELTA, FULL_BLOOM_MASTERY_DELTA].contains(&delta.abs()) {
                        if state.pending.take().is_some() {
                            counters.superseded_mastery_transitions += 1;
                        }
                        counters.matching_mastery_transitions += 1;
                        state.pending = Some(PendingTransition {
                            run_ordinal,
                            mastery_sequence: envelope.sequence,
                            mastery_observed_micros: envelope.time.observed_micros,
                            mastery_wire: wire_identity(&envelope.provenance.source),
                            mastery_before: before,
                            mastery_after: after,
                            mastery_delta: delta,
                            light_before,
                            expected_light_delta: delta * CONVERSION_NUMERATOR
                                / CONVERSION_DENOMINATOR,
                            inspiration_active: state.inspiration_instances > 0,
                            damage_events: 0,
                            light_damage_events: 0,
                            light_damage_amount: 0,
                            property_counts: BTreeMap::new(),
                            light_damage_by_ability: BTreeMap::new(),
                            gap_damage_on_mastery_wire: 0,
                            gap_damage_wires: Vec::new(),
                        });
                    }
                }

                if let Some(light_after) = light_in_event
                    && let Some(pending) = state.pending.take()
                {
                    let light_wire = wire_identity(&envelope.provenance.source);
                    let same_wire =
                        pending.mastery_wire.is_some() && pending.mastery_wire == light_wire;
                    if same_wire {
                        counters.observed_light_transitions_on_same_wire = counters
                            .observed_light_transitions_on_same_wire
                            .saturating_add(1);
                    } else {
                        counters.observed_light_transitions_on_distinct_wires = counters
                            .observed_light_transitions_on_distinct_wires
                            .saturating_add(1);
                    }
                    let gap_damage_on_light_wire = pending
                        .gap_damage_wires
                        .iter()
                        .filter(|wire| wire.is_some() && **wire == light_wire)
                        .count() as u64;
                    let gap_damage_on_distinct_wire = pending
                        .damage_events
                        .saturating_sub(pending.gap_damage_on_mastery_wire)
                        .saturating_sub(gap_damage_on_light_wire);
                    counters.gap_damage_on_mastery_wire = counters
                        .gap_damage_on_mastery_wire
                        .saturating_add(pending.gap_damage_on_mastery_wire);
                    counters.gap_damage_on_light_wire = counters
                        .gap_damage_on_light_wire
                        .saturating_add(gap_damage_on_light_wire);
                    counters.gap_damage_on_distinct_wire = counters
                        .gap_damage_on_distinct_wire
                        .saturating_add(gap_damage_on_distinct_wire);
                    transition_boundaries.insert(pending.mastery_sequence, envelope.sequence);
                    let light_delta = pending
                        .light_before
                        .map(|before| light_after.saturating_sub(before));
                    let exact = light_delta == Some(pending.expected_light_delta);
                    if exact {
                        counters.matching_light_transitions += 1;
                    } else {
                        counters.mismatched_light_transitions += 1;
                    }
                    let latency = envelope
                        .time
                        .observed_micros
                        .saturating_sub(pending.mastery_observed_micros);
                    if latency == 0 {
                        counters.same_event_light_transitions += 1;
                    } else {
                        counters.delayed_light_transitions += 1;
                    }
                    add_gap_counters(&mut counters, &pending);
                    let example = TransitionExample {
                        rlog: path.display().to_string(),
                        session_id: envelope.session_id.clone(),
                        run_ordinal: pending.run_ordinal,
                        entity_uuid: arguments.entity_uuid,
                        mastery_sequence: pending.mastery_sequence,
                        light_sequence: envelope.sequence,
                        mastery_wire: pending.mastery_wire,
                        light_wire,
                        same_wire,
                        latency_micros: latency,
                        mastery_before: pending.mastery_before,
                        mastery_after: pending.mastery_after,
                        mastery_delta: pending.mastery_delta,
                        light_before: pending.light_before,
                        light_after,
                        light_delta,
                        expected_light_delta: pending.expected_light_delta,
                        exact,
                        inspiration_active_at_mastery_transition: pending.inspiration_active,
                        gap_damage_events: pending.damage_events,
                        gap_light_damage_events: pending.light_damage_events,
                        gap_light_damage_amount: pending.light_damage_amount,
                        gap_damage_property_counts: pending.property_counts,
                        gap_damage_on_mastery_wire: pending.gap_damage_on_mastery_wire,
                        gap_damage_on_light_wire,
                        gap_damage_on_distinct_wire,
                    };
                    let examples = if exact {
                        &mut exact_examples
                    } else {
                        &mut mismatch_examples
                    };
                    if examples.len() < EXAMPLE_LIMIT {
                        examples.push(example);
                    }
                }
            }
            TimelineEventKind::Damage(damage)
                if damage.source.entity_uuid.0 == arguments.entity_uuid =>
            {
                if let Some(pending) = state.pending.as_mut() {
                    let damage_wire = wire_identity(&envelope.provenance.source);
                    pending.damage_events += 1;
                    if damage_wire.is_some() && damage_wire == pending.mastery_wire {
                        pending.gap_damage_on_mastery_wire =
                            pending.gap_damage_on_mastery_wire.saturating_add(1);
                    }
                    pending.gap_damage_wires.push(damage_wire);
                    *pending
                        .property_counts
                        .entry(property_key(damage.packet.property))
                        .or_insert(0) += 1;
                    if damage.packet.property == Some(BpsrDamageProperty::Light.protocol_id()) {
                        let ability_id = damage.ability.map(|value| value.0).unwrap_or(0);
                        pending.light_damage_events += 1;
                        pending.light_damage_amount =
                            pending.light_damage_amount.saturating_add(damage.amount);
                        let aggregate = pending
                            .light_damage_by_ability
                            .entry(ability_id)
                            .or_default();
                        aggregate.events = aggregate.events.saturating_add(1);
                        aggregate.amount = aggregate.amount.saturating_add(damage.amount);
                        if gap_light_damage_examples.len() < 10_000 {
                            gap_light_damage_examples.push(GapLightDamageExample {
                                sequence: envelope.sequence,
                                observed_micros: envelope.time.observed_micros,
                                damage_wire,
                                run_ordinal,
                                mastery_sequence: pending.mastery_sequence,
                                mastery_wire: pending.mastery_wire,
                                mastery_before: pending.mastery_before,
                                mastery_after: pending.mastery_after,
                                serialized_light: pending.light_before,
                                logical_light: pending.light_before.and_then(|value| {
                                    value.checked_add(pending.expected_light_delta)
                                }),
                                source_entity_uuid: damage.source.entity_uuid.0,
                                direct_source_entity_uuid: damage
                                    .direct_source
                                    .map(|source| source.entity_uuid.0),
                                target_entity_uuid: damage.target.entity_uuid.0,
                                ability_id,
                                hit_event_id: damage.hit_event_id,
                                amount: damage.amount,
                                normal_value: damage.packet.normal_value,
                                critical: damage.flags.critical,
                                lucky: damage.flags.lucky,
                                owner_level: damage.packet.owner_level,
                                owner_stage: damage.packet.owner_stage,
                                property: damage.packet.property,
                                skill_effect_uuid: damage.packet.skill_effect_uuid,
                                skill_effect_total_damage: damage.packet.skill_effect_total_damage,
                                skill_effect_group_index: damage.packet.skill_effect_group_index,
                                skill_effect_component_index: damage
                                    .packet
                                    .skill_effect_component_index,
                                skill_effect_component_count: damage
                                    .packet
                                    .skill_effect_component_count,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(pending) = state.pending.take() {
        counters.expired_mastery_transitions += 1;
        add_gap_counters(&mut counters, &pending);
    }
    Ok(SessionReport {
        rlog: path.display().to_string(),
        session_id,
        counters,
        transition_boundaries,
        exact_examples,
        mismatch_examples,
        gap_light_damage_examples,
    })
}

fn expire_pending(
    state: &mut ActorState,
    counters: &mut Counters,
    observed_micros: u64,
    window_micros: u64,
) {
    let expired = state.pending.as_ref().is_some_and(|pending| {
        observed_micros.saturating_sub(pending.mastery_observed_micros) > window_micros
    });
    if expired && let Some(pending) = state.pending.take() {
        counters.expired_mastery_transitions += 1;
        add_gap_counters(counters, &pending);
    }
}

fn add_gap_counters(counters: &mut Counters, pending: &PendingTransition) {
    if pending.damage_events > 0 {
        counters.transitions_with_any_gap_damage += 1;
    }
    if pending.light_damage_events > 0 {
        counters.transitions_with_gap_light_damage += 1;
    }
    counters.gap_damage_events += pending.damage_events;
    counters.gap_light_damage_events += pending.light_damage_events;
    counters.gap_light_damage_amount = counters
        .gap_light_damage_amount
        .saturating_add(pending.light_damage_amount);
    for (&ability_id, aggregate) in &pending.light_damage_by_ability {
        counters
            .gap_light_damage_by_ability
            .entry(ability_id)
            .or_default()
            .add(aggregate);
    }
}

fn find_integer(attributes: &[EntityAttribute], attribute_id: i32) -> Option<i64> {
    attributes
        .iter()
        .find(|attribute| attribute.attribute_id == attribute_id)
        .and_then(integer_attribute)
}

fn integer_attribute(attribute: &EntityAttribute) -> Option<i64> {
    match attribute.decoded.clone().or_else(|| {
        decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value)
    }) {
        Some(EntityAttributeValue::Integer(value)) => Some(value),
        Some(EntityAttributeValue::Text(_)) | Some(EntityAttributeValue::Position { .. }) => None,
        None => decode_varint(&attribute.raw_value).and_then(|value| i64::try_from(value).ok()),
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

fn property_key(raw: Option<i32>) -> String {
    match raw.and_then(BpsrDamageProperty::from_protocol_id) {
        Some(property) => format!("{}:{}", property.protocol_id(), property.as_str()),
        None => raw.map_or_else(|| "missing".to_owned(), |value| format!("{value}:unknown")),
    }
}

fn wire_identity(source: &EvidenceSource) -> Option<WireIdentity> {
    match source {
        EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } => Some(WireIdentity {
            capture_sequence: *capture_sequence,
            connection_id: *connection_id,
            stream_id: *stream_id,
        }),
        EvidenceSource::Derived { .. } | EvidenceSource::Manual { .. } => None,
    }
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let entity_uuid = parse(&take_value(&mut values, "--entity")?, "--entity")?;
    let window_micros = take_optional_value(&mut values, "--window-micros")?
        .map(|value| parse(&value, "--window-micros"))
        .transpose()?
        .unwrap_or(250_000);
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err(usage());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        entity_uuid,
        window_micros,
        rlogs,
        output,
    })
}

fn parse<T: std::str::FromStr>(value: &OsString, option: &str) -> Result<T, String> {
    value
        .to_string_lossy()
        .parse::<T>()
        .map_err(|_| format!("{option} requires a numeric value"))
}

fn take_value(values: &mut Vec<OsString>, option: &str) -> Result<OsString, String> {
    let Some(position) = values.iter().position(|value| value == option) else {
        return Err(usage());
    };
    if position + 1 >= values.len() {
        return Err(usage());
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn take_optional_value(
    values: &mut Vec<OsString>,
    option: &str,
) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == option) else {
        return Ok(None);
    };
    if position + 1 >= values.len() {
        return Err(usage());
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(Some(value))
}

fn usage() -> String {
    "usage: rlogs-bpsr-inspiration-mastery-gap-proof --entity <uuid> [--window-micros <n>] --rlog <path> [--rlog <path> ...] --output <path>".to_owned()
}
