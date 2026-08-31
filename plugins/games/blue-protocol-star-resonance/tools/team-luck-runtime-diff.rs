use std::{
    collections::{BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_combat::{
    ExactDamageContributionEvent, ExactDamageContributionProjector,
    ExactRationalDamageContributionEvent,
};
use rlogs_events::{
    CanonicalEvent, EntityAttribute, EntityAttributeValue, EvidenceSource, RunState, StatusState,
    TimelineEventKind,
};
use rlogs_game_bpsr::BpsrStateDamageContributionProjector;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const TEAM_LUCK_EFFECT_ID: i64 = 2_302_121;
const CRITICAL_DAMAGE_ATTRIBUTE_ID: i32 = 12_510;
const LUCKY_DAMAGE_ATTRIBUTE_ID: i32 = 12_530;
const TEAM_LUCK_CRITICAL_RAW_DELTA: i64 = 520;
const TEAM_LUCK_LUCKY_RAW_DELTA: i64 = 340;
const PERCENT_SCALE: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct WireKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

type RecipientWireKey = (u32, WireKey, i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct StatusKey {
    effect_id: i64,
    instance_id: Option<i64>,
    source_entity_uuid: Option<i64>,
}

#[derive(Debug, Default, Serialize)]
struct DiffCounts {
    critical_or_lucky_damage_events: u64,
    critical_only_damage_events: u64,
    lucky_only_damage_events: u64,
    lifecycle_eligible_events: u64,
    lifecycle_eligible_critical_only_events: u64,
    lifecycle_eligible_lucky_only_events: u64,
    proof_eligible_events: u64,
    proof_eligible_critical_only_events: u64,
    proof_eligible_lucky_only_events: u64,
    proof_eligible_critical_only_amount: i128,
    proof_eligible_lucky_only_amount: i128,
    combined_flag_events: u64,
    runtime_emitted_combined_events: u64,
    runtime_emitted_combined_lifecycle_eligible_events: u64,
    runtime_emitted_combined_outside_lifecycle_events: u64,
    runtime_emitted_combined_transition_wire: u64,
    runtime_emitted_combined_missing_critical_attribute: u64,
    runtime_emitted_combined_missing_lucky_attribute: u64,
    runtime_emitted_combined_no_external_provider: u64,
    runtime_emitted_combined_multiple_external_providers: u64,
    inverse_incompatible_events: u64,
    ambiguous_counterfactual_events: u64,
    invalid_factor_events: u64,
    runtime_emitted_events: u64,
    both_events: u64,
    proof_only_events: u64,
    runtime_only_events: u64,
    neither_events: u64,
    runtime_only_proof_transition_wire: u64,
    runtime_only_proof_missing_critical_attribute: u64,
    runtime_only_proof_missing_lucky_attribute: u64,
    runtime_only_proof_no_external_provider: u64,
    runtime_only_proof_multiple_external_providers: u64,
}

#[derive(Debug, Serialize)]
struct DiffExample {
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    source_actor_id: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    amount: i64,
    critical: bool,
    lucky: bool,
    transition_wire: bool,
    proof_critical_damage_raw: Option<i64>,
    proof_lucky_damage_raw: Option<i64>,
    proof_external_provider_entity_uuids: Vec<i64>,
    proof_exact_marginal: Option<i64>,
    runtime_provider_actor_id: Option<u64>,
    runtime_numerator: Option<String>,
    runtime_denominator: Option<String>,
}

#[derive(Debug, Serialize)]
struct FileReport {
    source_path: String,
    counts: DiffCounts,
    runtime_only_examples: Vec<DiffExample>,
    proof_only_examples: Vec<DiffExample>,
}

#[derive(Debug, Serialize)]
struct Bundle {
    schema_version: u16,
    effect_id: i64,
    critical_damage_attribute_id: i32,
    lucky_damage_attribute_id: i32,
    reports: Vec<FileReport>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Team Luck runtime differential failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut reports = Vec::with_capacity(arguments.rlogs.len());
    for path in &arguments.rlogs {
        reports.push(audit(path, arguments.example_limit)?);
    }
    let bundle = Bundle {
        schema_version: 3,
        effect_id: TEAM_LUCK_EFFECT_ID,
        critical_damage_attribute_id: CRITICAL_DAMAGE_ATTRIBUTE_ID,
        lucky_damage_attribute_id: LUCKY_DAMAGE_ATTRIBUTE_ID,
        reports,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn audit(path: &Path, example_limit: usize) -> Result<FileReport, Box<dyn std::error::Error>> {
    let transition_wires = selected_effect_transition_wires(path)?;
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut projector = BpsrStateDamageContributionProjector::new()?;
    let mut integer_output = Vec::<ExactDamageContributionEvent>::new();
    let mut rational_output = Vec::<ExactRationalDamageContributionEvent>::new();
    let mut run_ordinal = 0_u32;
    let mut attributes = HashMap::<(u32, i64, i32), i64>::new();
    let mut statuses = HashMap::<(u32, i64), BTreeSet<StatusKey>>::new();
    let mut counts = DiffCounts::default();
    let mut runtime_only_examples = Vec::new();
    let mut proof_only_examples = Vec::new();

    while let Some(envelope) = reader.next_event()? {
        integer_output.clear();
        rational_output.clear();
        projector.observe(&envelope, &mut integer_output, &mut rational_output);
        let runtime = rational_output
            .iter()
            .find(|event| event.effect_id == TEAM_LUCK_EFFECT_ID);

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
                for attribute in &event.attributes {
                    if matches!(
                        attribute.attribute_id,
                        CRITICAL_DAMAGE_ATTRIBUTE_ID | LUCKY_DAMAGE_ATTRIBUTE_ID
                    ) && let Some(value) = decode_attribute(attribute)
                    {
                        attributes.insert(
                            (
                                run_ordinal,
                                event.actor.entity_uuid.0,
                                attribute.attribute_id,
                            ),
                            value,
                        );
                    }
                }
            }
            TimelineEventKind::Status(status) => {
                let key = StatusKey {
                    effect_id: status.effect.0,
                    instance_id: status.instance_id.map(|value| value.0),
                    source_entity_uuid: status.source.map(|value| value.entity_uuid.0),
                };
                let active = statuses
                    .entry((run_ordinal, status.target.entity_uuid.0))
                    .or_default();
                match status.state {
                    StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                        active.insert(key);
                    }
                    StatusState::Consumed | StatusState::Removed => {
                        active.remove(&key);
                    }
                }
            }
            TimelineEventKind::Damage(damage) => {
                let critical = damage.flags.critical == Some(true);
                let lucky = damage.flags.lucky == Some(true);
                if !critical && !lucky {
                    continue;
                }
                counts.critical_or_lucky_damage_events =
                    counts.critical_or_lucky_damage_events.saturating_add(1);
                if critical && !lucky {
                    counts.critical_only_damage_events =
                        counts.critical_only_damage_events.saturating_add(1);
                } else if lucky && !critical {
                    counts.lucky_only_damage_events =
                        counts.lucky_only_damage_events.saturating_add(1);
                }
                let source_entity_uuid = damage.source.entity_uuid.0;
                let transition_wire = wire_key(&envelope.provenance.source).is_some_and(|wire| {
                    transition_wires.contains(&(run_ordinal, wire, source_entity_uuid))
                });
                let critical_raw = attributes
                    .get(&(
                        run_ordinal,
                        source_entity_uuid,
                        CRITICAL_DAMAGE_ATTRIBUTE_ID,
                    ))
                    .copied();
                let lucky_raw = attributes
                    .get(&(run_ordinal, source_entity_uuid, LUCKY_DAMAGE_ATTRIBUTE_ID))
                    .copied();
                let providers = statuses
                    .get(&(run_ordinal, source_entity_uuid))
                    .into_iter()
                    .flat_map(|active| active.iter())
                    .filter(|key| key.effect_id == TEAM_LUCK_EFFECT_ID)
                    .filter_map(|key| key.source_entity_uuid)
                    .filter(|provider| *provider != source_entity_uuid)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                let lifecycle_eligible = !transition_wire
                    && critical_raw.is_some()
                    && lucky_raw.is_some()
                    && providers.len() == 1;
                let counterfactual = lifecycle_eligible.then(|| {
                    team_luck_counterfactual(
                        damage.amount,
                        critical,
                        lucky,
                        critical_raw.expect("checked above"),
                        lucky_raw.expect("checked above"),
                    )
                });
                let proof_exact_marginal = counterfactual.and_then(|outcome| match outcome {
                    CounterfactualOutcome::Exact(amount) => Some(amount),
                    CounterfactualOutcome::CombinedFlags
                    | CounterfactualOutcome::InverseIncompatible
                    | CounterfactualOutcome::Ambiguous
                    | CounterfactualOutcome::InvalidFactor => None,
                });
                let proof = matches!(counterfactual, Some(CounterfactualOutcome::Exact(_)));
                let runtime_emitted = runtime.is_some();
                match (proof, runtime_emitted) {
                    (true, true) => counts.both_events = counts.both_events.saturating_add(1),
                    (true, false) => {
                        counts.proof_only_events = counts.proof_only_events.saturating_add(1)
                    }
                    (false, true) => {
                        counts.runtime_only_events = counts.runtime_only_events.saturating_add(1)
                    }
                    (false, false) => {
                        counts.neither_events = counts.neither_events.saturating_add(1)
                    }
                }
                if lifecycle_eligible {
                    counts.lifecycle_eligible_events =
                        counts.lifecycle_eligible_events.saturating_add(1);
                    if critical && !lucky {
                        counts.lifecycle_eligible_critical_only_events = counts
                            .lifecycle_eligible_critical_only_events
                            .saturating_add(1);
                    } else if lucky && !critical {
                        counts.lifecycle_eligible_lucky_only_events = counts
                            .lifecycle_eligible_lucky_only_events
                            .saturating_add(1);
                    }
                    match counterfactual.expect("lifecycle eligibility computed it") {
                        CounterfactualOutcome::Exact(_) => {}
                        CounterfactualOutcome::CombinedFlags => {
                            counts.combined_flag_events =
                                counts.combined_flag_events.saturating_add(1);
                        }
                        CounterfactualOutcome::InverseIncompatible => {
                            counts.inverse_incompatible_events =
                                counts.inverse_incompatible_events.saturating_add(1);
                        }
                        CounterfactualOutcome::Ambiguous => {
                            counts.ambiguous_counterfactual_events =
                                counts.ambiguous_counterfactual_events.saturating_add(1);
                        }
                        CounterfactualOutcome::InvalidFactor => {
                            counts.invalid_factor_events =
                                counts.invalid_factor_events.saturating_add(1);
                        }
                    }
                }
                if proof {
                    counts.proof_eligible_events = counts.proof_eligible_events.saturating_add(1);
                    let amount = i128::from(
                        proof_exact_marginal.expect("exact proof outcome carries a marginal"),
                    );
                    if critical && !lucky {
                        counts.proof_eligible_critical_only_events =
                            counts.proof_eligible_critical_only_events.saturating_add(1);
                        counts.proof_eligible_critical_only_amount = counts
                            .proof_eligible_critical_only_amount
                            .saturating_add(amount);
                    } else if lucky && !critical {
                        counts.proof_eligible_lucky_only_events =
                            counts.proof_eligible_lucky_only_events.saturating_add(1);
                        counts.proof_eligible_lucky_only_amount = counts
                            .proof_eligible_lucky_only_amount
                            .saturating_add(amount);
                    }
                }
                if runtime_emitted {
                    counts.runtime_emitted_events = counts.runtime_emitted_events.saturating_add(1);
                    if critical && lucky {
                        counts.runtime_emitted_combined_events =
                            counts.runtime_emitted_combined_events.saturating_add(1);
                        if lifecycle_eligible {
                            counts.runtime_emitted_combined_lifecycle_eligible_events = counts
                                .runtime_emitted_combined_lifecycle_eligible_events
                                .saturating_add(1);
                        } else {
                            counts.runtime_emitted_combined_outside_lifecycle_events = counts
                                .runtime_emitted_combined_outside_lifecycle_events
                                .saturating_add(1);
                            if transition_wire {
                                counts.runtime_emitted_combined_transition_wire = counts
                                    .runtime_emitted_combined_transition_wire
                                    .saturating_add(1);
                            }
                            if critical_raw.is_none() {
                                counts.runtime_emitted_combined_missing_critical_attribute = counts
                                    .runtime_emitted_combined_missing_critical_attribute
                                    .saturating_add(1);
                            }
                            if lucky_raw.is_none() {
                                counts.runtime_emitted_combined_missing_lucky_attribute = counts
                                    .runtime_emitted_combined_missing_lucky_attribute
                                    .saturating_add(1);
                            }
                            if providers.is_empty() {
                                counts.runtime_emitted_combined_no_external_provider = counts
                                    .runtime_emitted_combined_no_external_provider
                                    .saturating_add(1);
                            } else if providers.len() > 1 {
                                counts.runtime_emitted_combined_multiple_external_providers =
                                    counts
                                        .runtime_emitted_combined_multiple_external_providers
                                        .saturating_add(1);
                            }
                        }
                    }
                }
                if runtime_emitted && !proof {
                    if transition_wire {
                        counts.runtime_only_proof_transition_wire =
                            counts.runtime_only_proof_transition_wire.saturating_add(1);
                    }
                    if critical_raw.is_none() {
                        counts.runtime_only_proof_missing_critical_attribute = counts
                            .runtime_only_proof_missing_critical_attribute
                            .saturating_add(1);
                    }
                    if lucky_raw.is_none() {
                        counts.runtime_only_proof_missing_lucky_attribute = counts
                            .runtime_only_proof_missing_lucky_attribute
                            .saturating_add(1);
                    }
                    if providers.is_empty() {
                        counts.runtime_only_proof_no_external_provider = counts
                            .runtime_only_proof_no_external_provider
                            .saturating_add(1);
                    } else if providers.len() > 1 {
                        counts.runtime_only_proof_multiple_external_providers = counts
                            .runtime_only_proof_multiple_external_providers
                            .saturating_add(1);
                    }
                }
                let example = DiffExample {
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    run_ordinal,
                    source_actor_id: damage.source.actor_id.0,
                    source_entity_uuid,
                    target_entity_uuid: damage.target.entity_uuid.0,
                    ability_id: damage.ability.map(|value| value.0),
                    amount: damage.amount,
                    critical,
                    lucky,
                    transition_wire,
                    proof_critical_damage_raw: critical_raw,
                    proof_lucky_damage_raw: lucky_raw,
                    proof_external_provider_entity_uuids: providers,
                    proof_exact_marginal,
                    runtime_provider_actor_id: runtime.map(|event| event.provider_actor_id),
                    runtime_numerator: runtime.map(|event| event.numerator.to_string()),
                    runtime_denominator: runtime.map(|event| event.denominator.to_string()),
                };
                if runtime_emitted && !proof && runtime_only_examples.len() < example_limit {
                    runtime_only_examples.push(example);
                } else if proof && !runtime_emitted && proof_only_examples.len() < example_limit {
                    proof_only_examples.push(example);
                }
            }
            _ => {}
        }
    }

    Ok(FileReport {
        source_path: path.display().to_string(),
        counts,
        runtime_only_examples,
        proof_only_examples,
    })
}

fn selected_effect_transition_wires(
    path: &Path,
) -> Result<BTreeSet<RecipientWireKey>, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut run_ordinal = 0_u32;
    let mut wires = BTreeSet::new();
    while let Some(envelope) = reader.next_event()? {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::Status(status) if status.effect.0 == TEAM_LUCK_EFFECT_ID => {
                if let Some(wire) = wire_key(&envelope.provenance.source) {
                    wires.insert((run_ordinal, wire, status.target.entity_uuid.0));
                }
            }
            _ => {}
        }
    }
    Ok(wires)
}

fn wire_key(source: &EvidenceSource) -> Option<WireKey> {
    match source {
        EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } => Some(WireKey {
            capture_sequence: *capture_sequence,
            connection_id: *connection_id,
            stream_id: *stream_id,
        }),
        EvidenceSource::Derived { .. } | EvidenceSource::Manual { .. } => None,
    }
}

fn decode_attribute(attribute: &EntityAttribute) -> Option<i64> {
    if let Some(EntityAttributeValue::Integer(value)) = attribute.decoded {
        return Some(value);
    }
    decode_varint(&attribute.raw_value).and_then(|value| i64::try_from(value).ok())
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

/// Independent audit oracle for the exact runtime projection. A hit is
/// attributable only when removing Team Luck produces one integer
/// counterfactual for every latent pre-floor value that could have produced
/// the packet amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CounterfactualOutcome {
    Exact(i64),
    CombinedFlags,
    InverseIncompatible,
    Ambiguous,
    InvalidFactor,
}

fn team_luck_counterfactual(
    observed_damage: i64,
    critical: bool,
    lucky: bool,
    critical_damage_raw: i64,
    lucky_damage_raw: i64,
) -> CounterfactualOutcome {
    if observed_damage <= 0 || (!critical && !lucky) {
        return CounterfactualOutcome::InvalidFactor;
    }
    if critical && lucky {
        return CounterfactualOutcome::CombinedFlags;
    }
    let observed_factor = if critical {
        PERCENT_SCALE.checked_add(critical_damage_raw)
    } else {
        Some(lucky_damage_raw)
    };
    let Some(observed_factor) = observed_factor else {
        return CounterfactualOutcome::InvalidFactor;
    };
    let removed_factor = observed_factor.checked_sub(if critical {
        TEAM_LUCK_CRITICAL_RAW_DELTA
    } else {
        TEAM_LUCK_LUCKY_RAW_DELTA
    });
    let Some(removed_factor) = removed_factor else {
        return CounterfactualOutcome::InvalidFactor;
    };
    if observed_factor <= 0 || removed_factor <= 0 {
        return CounterfactualOutcome::InvalidFactor;
    }
    let Some((base_minimum, base_maximum)) =
        inverse_floor_preimage(observed_damage, observed_factor, PERCENT_SCALE)
    else {
        return CounterfactualOutcome::InverseIncompatible;
    };
    let Some(counterfactual_minimum) = base_minimum
        .checked_mul(i128::from(removed_factor))
        .and_then(|value| value.checked_div(i128::from(PERCENT_SCALE)))
    else {
        return CounterfactualOutcome::InvalidFactor;
    };
    let Some(counterfactual_maximum) = base_maximum
        .checked_mul(i128::from(removed_factor))
        .and_then(|value| value.checked_div(i128::from(PERCENT_SCALE)))
    else {
        return CounterfactualOutcome::InvalidFactor;
    };
    if counterfactual_minimum != counterfactual_maximum {
        return CounterfactualOutcome::Ambiguous;
    }
    let Ok(counterfactual) = i64::try_from(counterfactual_minimum) else {
        return CounterfactualOutcome::InvalidFactor;
    };
    let Some(amount) = observed_damage
        .checked_sub(counterfactual)
        .filter(|amount| *amount > 0 && *amount <= observed_damage)
    else {
        return CounterfactualOutcome::InvalidFactor;
    };
    CounterfactualOutcome::Exact(amount)
}

fn inverse_floor_preimage(output: i64, numerator: i64, denominator: i64) -> Option<(i128, i128)> {
    if output < 0 || numerator <= 0 || denominator <= 0 {
        return None;
    }
    let output = i128::from(output);
    let numerator = i128::from(numerator);
    let denominator = i128::from(denominator);
    let minimum = ceil_div_positive(output.checked_mul(denominator)?, numerator)?;
    let maximum = ceil_div_positive(output.checked_add(1)?.checked_mul(denominator)?, numerator)?
        .checked_sub(1)?;
    (minimum <= maximum).then_some((minimum, maximum))
}

fn ceil_div_positive(numerator: i128, denominator: i128) -> Option<i128> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    numerator
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)
}

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    example_limit: usize,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let example_limit = take_optional_value(&mut values, "--example-limit")
        .map(|value| value.to_string_lossy().parse::<usize>())
        .transpose()
        .map_err(|_| "--example-limit must be an unsigned integer".to_owned())?
        .unwrap_or(20);
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        values.remove(position);
        rlogs.push(PathBuf::from(values.remove(position)));
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlogs,
        output,
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

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return None;
    }
    values.remove(position);
    Some(values.remove(position))
}

fn usage() -> String {
    "usage: rlogs-bpsr-team-luck-runtime-diff --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <report.json> [--example-limit <count>]".to_owned()
}
