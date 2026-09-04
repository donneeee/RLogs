use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, EntityAttribute, EntityAttributeUpdateKind, EntityAttributeValue, RunState,
    TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const SCHEMA_VERSION: u16 = 2;
const EXAMPLE_LIMIT: usize = 12;

#[derive(Debug)]
struct Arguments {
    left_attribute_id: i32,
    right_attribute_id: i32,
    numerator: i64,
    denominator: i64,
    offset: i64,
    entity_uuid: Option<i64>,
    transition_window_micros: Option<u64>,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Default)]
struct ActorState {
    values: BTreeMap<i32, i64>,
    trusted: bool,
    pending_right: Option<PendingRightTransition>,
}

#[derive(Debug, Clone, Copy)]
struct PendingRightTransition {
    old_right: i64,
    new_right: i64,
    old_left: i64,
    sequence: u64,
    observed_micros: u64,
}

#[derive(Debug, Default, Serialize)]
struct Counters {
    snapshot_events: u64,
    delta_events: u64,
    unknown_update_events: u64,
    co_present_events: u64,
    evaluated_states: u64,
    exact_matches: u64,
    mismatches: u64,
    transition_candidates: u64,
    transition_exact_matches: u64,
    transition_mismatches: u64,
    transition_expired: u64,
    transition_same_wire_exact_matches: u64,
    transition_same_wire_mismatches: u64,
    transition_delayed_exact_matches: u64,
    transition_delayed_mismatches: u64,
}

impl Counters {
    fn add(&mut self, other: &Self) {
        self.snapshot_events = self.snapshot_events.saturating_add(other.snapshot_events);
        self.delta_events = self.delta_events.saturating_add(other.delta_events);
        self.unknown_update_events = self
            .unknown_update_events
            .saturating_add(other.unknown_update_events);
        self.co_present_events = self
            .co_present_events
            .saturating_add(other.co_present_events);
        self.evaluated_states = self.evaluated_states.saturating_add(other.evaluated_states);
        self.exact_matches = self.exact_matches.saturating_add(other.exact_matches);
        self.mismatches = self.mismatches.saturating_add(other.mismatches);
        self.transition_candidates = self
            .transition_candidates
            .saturating_add(other.transition_candidates);
        self.transition_exact_matches = self
            .transition_exact_matches
            .saturating_add(other.transition_exact_matches);
        self.transition_mismatches = self
            .transition_mismatches
            .saturating_add(other.transition_mismatches);
        self.transition_expired = self
            .transition_expired
            .saturating_add(other.transition_expired);
        self.transition_same_wire_exact_matches = self
            .transition_same_wire_exact_matches
            .saturating_add(other.transition_same_wire_exact_matches);
        self.transition_same_wire_mismatches = self
            .transition_same_wire_mismatches
            .saturating_add(other.transition_same_wire_mismatches);
        self.transition_delayed_exact_matches = self
            .transition_delayed_exact_matches
            .saturating_add(other.transition_delayed_exact_matches);
        self.transition_delayed_mismatches = self
            .transition_delayed_mismatches
            .saturating_add(other.transition_delayed_mismatches);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Pair {
    right_value: i64,
    left_value: i64,
    predicted_left_value: i64,
    residual: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct TransitionPair {
    old_right_value: i64,
    new_right_value: i64,
    old_left_value: i64,
    new_left_value: i64,
    right_delta: i64,
    left_delta: i64,
    predicted_left_delta: i64,
    residual: i64,
}

#[derive(Debug, Serialize)]
struct Example {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    entity_uuid: i64,
    sequence: u64,
    update_kind: EntityAttributeUpdateKind,
    pair: Pair,
}

#[derive(Debug, Serialize)]
struct TransitionExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    entity_uuid: i64,
    right_sequence: u64,
    left_sequence: u64,
    latency_micros: u64,
    pair: TransitionPair,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    counters: Counters,
    distinct_pairs: usize,
    exact_examples: Vec<Example>,
    mismatch_examples: Vec<Example>,
    transition_exact_examples: Vec<TransitionExample>,
    transition_mismatch_examples: Vec<TransitionExample>,
}

type SessionEvidence = (SessionReport, BTreeSet<Pair>, BTreeSet<TransitionPair>);

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    scope: AuditScope,
    formula: Formula,
    totals: Counters,
    distinct_pairs: usize,
    pairs: Vec<Pair>,
    distinct_transition_pairs: usize,
    transition_pairs: Vec<TransitionPair>,
    sessions: Vec<SessionReport>,
}

#[derive(Debug, Serialize)]
struct AuditScope {
    entity_uuid: Option<i64>,
    transition_window_micros: Option<u64>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_authority: bool,
    snapshot_semantics: &'static str,
    delta_semantics: &'static str,
    unknown_update_semantics: &'static str,
    missing_attribute_semantics: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct Formula {
    expression: String,
    left_attribute_id: i32,
    right_attribute_id: i32,
    numerator: i64,
    denominator: i64,
    offset: i64,
    rounding: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("attribute relation proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut totals = Counters::default();
    let mut pairs = BTreeSet::<Pair>::new();
    let mut transition_pairs = BTreeSet::<TransitionPair>::new();
    let mut sessions = Vec::new();
    for path in &arguments.rlogs {
        let (session, session_pairs, session_transition_pairs) = read_session(path, &arguments)?;
        totals.add(&session.counters);
        pairs.extend(session_pairs);
        transition_pairs.extend(session_transition_pairs);
        sessions.push(session);
    }
    let formula = Formula {
        expression: format!(
            "left = floor(right * {} / {}) + {}",
            arguments.numerator, arguments.denominator, arguments.offset
        ),
        left_attribute_id: arguments.left_attribute_id,
        right_attribute_id: arguments.right_attribute_id,
        numerator: arguments.numerator,
        denominator: arguments.denominator,
        offset: arguments.offset,
        rounding: "mathematical_floor",
    };
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-attribute-relation-proof",
        policy: AuditPolicy {
            runtime_authority: false,
            snapshot_semantics: "a packet-marked snapshot replaces prior known state for that actor lifecycle before its values are applied",
            delta_semantics: "a packet-marked delta updates only the attributes present in the event",
            unknown_update_semantics: "unknown updates invalidate carried state; they are evaluated only when both selected attributes coexist in that event",
            missing_attribute_semantics: "absence is unknown and is never materialized as zero",
            unresolved_evidence_is_hidden: false,
        },
        scope: AuditScope {
            entity_uuid: arguments.entity_uuid,
            transition_window_micros: arguments.transition_window_micros,
        },
        formula,
        totals,
        distinct_pairs: pairs.len(),
        pairs: pairs.into_iter().collect(),
        distinct_transition_pairs: transition_pairs.len(),
        transition_pairs: transition_pairs.into_iter().collect(),
        sessions,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_session(
    path: &Path,
    arguments: &Arguments,
) -> Result<SessionEvidence, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = None::<String>;
    let mut run_ordinal = 0_u32;
    let mut states = BTreeMap::<(u32, i64), ActorState>::new();
    let mut counters = Counters::default();
    let mut pairs = BTreeSet::<Pair>::new();
    let mut transition_pairs = BTreeSet::<TransitionPair>::new();
    let mut exact_examples = Vec::new();
    let mut mismatch_examples = Vec::new();
    let mut transition_exact_examples = Vec::new();
    let mut transition_mismatch_examples = Vec::new();

    while let Some(envelope) = reader.next_event()? {
        session_id.get_or_insert_with(|| envelope.session_id.clone());
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::EntityAttributes(event) => {
                if arguments
                    .entity_uuid
                    .is_some_and(|entity_uuid| event.actor.entity_uuid.0 != entity_uuid)
                {
                    continue;
                }
                let left_in_event = find_integer(&event.attributes, arguments.left_attribute_id);
                let right_in_event = find_integer(&event.attributes, arguments.right_attribute_id);
                if left_in_event.is_some() && right_in_event.is_some() {
                    counters.co_present_events = counters.co_present_events.saturating_add(1);
                }
                let key = (run_ordinal, event.actor.entity_uuid.0);
                let state = states.entry(key).or_default();
                let before_left = state.values.get(&arguments.left_attribute_id).copied();
                let before_right = state.values.get(&arguments.right_attribute_id).copied();
                match event.update_kind {
                    EntityAttributeUpdateKind::Snapshot => {
                        counters.snapshot_events = counters.snapshot_events.saturating_add(1);
                        state.values.clear();
                        state.pending_right = None;
                        state.trusted = true;
                    }
                    EntityAttributeUpdateKind::Delta => {
                        counters.delta_events = counters.delta_events.saturating_add(1);
                    }
                    EntityAttributeUpdateKind::Unknown => {
                        counters.unknown_update_events =
                            counters.unknown_update_events.saturating_add(1);
                        state.values.clear();
                        state.pending_right = None;
                        state.trusted = left_in_event.is_some() && right_in_event.is_some();
                    }
                }
                let touches_relation = left_in_event.is_some() || right_in_event.is_some();
                for attribute in &event.attributes {
                    if [arguments.left_attribute_id, arguments.right_attribute_id]
                        .contains(&attribute.attribute_id)
                        && let Some(value) = integer_attribute(attribute)
                    {
                        state.values.insert(attribute.attribute_id, value);
                    }
                }
                if state.trusted
                    && let Some(window_micros) = arguments.transition_window_micros
                {
                    let observed_micros = envelope.time.observed_micros;
                    if state.pending_right.is_some_and(|pending| {
                        observed_micros.saturating_sub(pending.observed_micros) > window_micros
                    }) {
                        counters.transition_expired = counters.transition_expired.saturating_add(1);
                        state.pending_right = None;
                    }
                    if let (Some(old_right), Some(new_right), Some(old_left)) =
                        (before_right, right_in_event, before_left)
                        && old_right != new_right
                    {
                        state.pending_right = Some(match state.pending_right {
                            Some(pending) if pending.new_right == old_right => {
                                PendingRightTransition {
                                    new_right,
                                    sequence: envelope.sequence,
                                    observed_micros,
                                    ..pending
                                }
                            }
                            _ => PendingRightTransition {
                                old_right,
                                new_right,
                                old_left,
                                sequence: envelope.sequence,
                                observed_micros,
                            },
                        });
                    }
                    if let (Some(new_left), Some(pending)) = (left_in_event, state.pending_right)
                        && new_left != pending.old_left
                    {
                        let latency_micros =
                            observed_micros.saturating_sub(pending.observed_micros);
                        if latency_micros <= window_micros
                            && let Some(pair) = transition_pair(pending, new_left, arguments)
                        {
                            counters.transition_candidates =
                                counters.transition_candidates.saturating_add(1);
                            if pair.residual == 0 {
                                counters.transition_exact_matches =
                                    counters.transition_exact_matches.saturating_add(1);
                                if latency_micros == 0 {
                                    counters.transition_same_wire_exact_matches = counters
                                        .transition_same_wire_exact_matches
                                        .saturating_add(1);
                                } else {
                                    counters.transition_delayed_exact_matches =
                                        counters.transition_delayed_exact_matches.saturating_add(1);
                                }
                            } else {
                                counters.transition_mismatches =
                                    counters.transition_mismatches.saturating_add(1);
                                if latency_micros == 0 {
                                    counters.transition_same_wire_mismatches =
                                        counters.transition_same_wire_mismatches.saturating_add(1);
                                } else {
                                    counters.transition_delayed_mismatches =
                                        counters.transition_delayed_mismatches.saturating_add(1);
                                }
                            }
                            transition_pairs.insert(pair.clone());
                            let examples = if pair.residual == 0 {
                                &mut transition_exact_examples
                            } else {
                                &mut transition_mismatch_examples
                            };
                            if examples.len() < EXAMPLE_LIMIT {
                                examples.push(TransitionExample {
                                    rlog: path.display().to_string(),
                                    session_id: envelope.session_id.clone(),
                                    run_ordinal,
                                    entity_uuid: event.actor.entity_uuid.0,
                                    right_sequence: pending.sequence,
                                    left_sequence: envelope.sequence,
                                    latency_micros,
                                    pair,
                                });
                            }
                        }
                        state.pending_right = None;
                    }
                }
                if !touches_relation || !state.trusted {
                    continue;
                }
                let (Some(&left), Some(&right)) = (
                    state.values.get(&arguments.left_attribute_id),
                    state.values.get(&arguments.right_attribute_id),
                ) else {
                    continue;
                };
                let Some(predicted) = predict(
                    right,
                    arguments.numerator,
                    arguments.denominator,
                    arguments.offset,
                ) else {
                    continue;
                };
                let Some(residual) = left.checked_sub(predicted) else {
                    continue;
                };
                counters.evaluated_states = counters.evaluated_states.saturating_add(1);
                if residual == 0 {
                    counters.exact_matches = counters.exact_matches.saturating_add(1);
                } else {
                    counters.mismatches = counters.mismatches.saturating_add(1);
                }
                let pair = Pair {
                    right_value: right,
                    left_value: left,
                    predicted_left_value: predicted,
                    residual,
                };
                pairs.insert(pair.clone());
                let examples = if residual == 0 {
                    &mut exact_examples
                } else {
                    &mut mismatch_examples
                };
                if examples.len() < EXAMPLE_LIMIT {
                    examples.push(Example {
                        rlog: path.display().to_string(),
                        session_id: envelope.session_id.clone(),
                        run_ordinal,
                        entity_uuid: event.actor.entity_uuid.0,
                        sequence: envelope.sequence,
                        update_kind: event.update_kind,
                        pair,
                    });
                }
            }
            _ => {}
        }
    }

    Ok((
        SessionReport {
            rlog: path.display().to_string(),
            session_id: session_id.unwrap_or_else(|| "unknown".to_owned()),
            counters,
            distinct_pairs: pairs.len(),
            exact_examples,
            mismatch_examples,
            transition_exact_examples,
            transition_mismatch_examples,
        },
        pairs,
        transition_pairs,
    ))
}

fn transition_pair(
    pending: PendingRightTransition,
    new_left: i64,
    arguments: &Arguments,
) -> Option<TransitionPair> {
    let old_predicted = predict(
        pending.old_right,
        arguments.numerator,
        arguments.denominator,
        0,
    )?;
    let new_predicted = predict(
        pending.new_right,
        arguments.numerator,
        arguments.denominator,
        0,
    )?;
    let right_delta = pending.new_right.checked_sub(pending.old_right)?;
    let left_delta = new_left.checked_sub(pending.old_left)?;
    let predicted_left_delta = new_predicted.checked_sub(old_predicted)?;
    let residual = left_delta.checked_sub(predicted_left_delta)?;
    Some(TransitionPair {
        old_right_value: pending.old_right,
        new_right_value: pending.new_right,
        old_left_value: pending.old_left,
        new_left_value: new_left,
        right_delta,
        left_delta,
        predicted_left_delta,
        residual,
    })
}

fn find_integer(attributes: &[EntityAttribute], attribute_id: i32) -> Option<i64> {
    attributes
        .iter()
        .find(|attribute| attribute.attribute_id == attribute_id)
        .and_then(integer_attribute)
}

fn integer_attribute(attribute: &EntityAttribute) -> Option<i64> {
    let decoded = attribute.decoded.clone().or_else(|| {
        decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value)
    });
    match decoded {
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

fn predict(right: i64, numerator: i64, denominator: i64, offset: i64) -> Option<i64> {
    if denominator <= 0 {
        return None;
    }
    let scaled = i128::from(right).checked_mul(i128::from(numerator))?;
    let quotient = scaled.div_euclid(i128::from(denominator));
    i64::try_from(quotient.checked_add(i128::from(offset))?).ok()
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let left_attribute_id = parse(&take_value(&mut values, "--left")?, "--left")?;
    let right_attribute_id = parse(&take_value(&mut values, "--right")?, "--right")?;
    let numerator = parse(&take_value(&mut values, "--numerator")?, "--numerator")?;
    let denominator = parse(&take_value(&mut values, "--denominator")?, "--denominator")?;
    let offset = take_optional_value(&mut values, "--offset")?
        .map(|value| parse(&value, "--offset"))
        .transpose()?
        .unwrap_or(0);
    let entity_uuid = take_optional_value(&mut values, "--entity")?
        .map(|value| parse(&value, "--entity"))
        .transpose()?;
    let transition_window_micros = take_optional_value(&mut values, "--transition-window-micros")?
        .map(|value| parse(&value, "--transition-window-micros"))
        .transpose()?;
    if denominator <= 0 {
        return Err("--denominator must be positive".to_owned());
    }
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        left_attribute_id,
        right_attribute_id,
        numerator,
        denominator,
        offset,
        entity_uuid,
        transition_window_micros,
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
        return Err(format!("{option} requires a value"));
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
        return Err(format!("{option} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(Some(value))
}

fn usage() -> String {
    "usage: rlogs-bpsr-attribute-relation-proof --output <json> --left <id> --right <id> --numerator <n> --denominator <d> [--offset <n>] [--entity <uuid>] [--transition-window-micros <micros>] --rlog <path> [--rlog <path> ...]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prediction_uses_mathematical_floor() {
        assert_eq!(predict(1_944, 35, 100, 0), Some(680));
        assert_eq!(predict(-3, 1, 2, 0), Some(-2));
    }
}
