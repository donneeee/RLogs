use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::File,
    io::{BufRead, BufReader, BufWriter},
    path::PathBuf,
};

use rlogs_events::{
    CanonicalEvent, DamageEvent, EventEnvelope, EvidenceSource, StatusEvent, StatusState,
    TimelineEventKind,
};
use serde::Serialize;

const PERCENT_SCALE: i64 = 10_000;
const MAXIMUM_MODEL_CANDIDATES: usize = 2_000;

fn main() {
    if let Err(error) = run() {
        eprintln!("module stack formula proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let reader = BufReader::new(File::open(&arguments.events)?);
    let mut state = StackState::default();
    let mut current_wire = None;
    let mut report = ReportAccumulator::default();
    let mut lines_read = 0_u64;

    for line in reader.lines() {
        lines_read = lines_read.saturating_add(1);
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let envelope: EventEnvelope = serde_json::from_str(&line)?;
        let EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } = envelope.provenance.source
        else {
            continue;
        };
        let key = WireKey {
            session_id: envelope.session_id.clone(),
            capture_sequence,
            connection_id,
            stream_id,
        };
        if current_wire
            .as_ref()
            .is_some_and(|wire: &WireAccumulator| wire.key != key)
        {
            flush_wire(
                current_wire.take().expect("wire was checked as present"),
                &mut state,
                &arguments,
                &mut report,
            );
        }
        let wire = current_wire.get_or_insert_with(|| WireAccumulator {
            key,
            first_envelope_sequence: envelope.sequence,
            last_envelope_sequence: envelope.sequence,
            state_before: state.clone(),
            statuses: Vec::new(),
            damages: Vec::new(),
        });
        wire.last_envelope_sequence = envelope.sequence;

        let CanonicalEvent::Timeline(timeline) = envelope.event else {
            continue;
        };
        match timeline.kind {
            TimelineEventKind::Status(status) if status.effect.0 == arguments.effect_id => {
                wire.statuses.push(StatusObservation::from_event(
                    envelope.sequence,
                    timeline.sequence,
                    &status,
                ));
                state.observe(&status);
            }
            TimelineEventKind::Damage(damage) => wire.damages.push(DamageObservation::from_event(
                envelope.sequence,
                timeline.sequence,
                &damage,
            )),
            _ => {}
        }
    }
    if let Some(wire) = current_wire.take() {
        flush_wire(wire, &mut state, &arguments, &mut report);
    }

    let bundle = ProofBundle {
        schema_version: 1,
        generated_by: "rlogs-bpsr-module-stack-formula-proof",
        policy: ProofPolicy {
            runtime_use: "offline_research_only_never_loaded_by_capture_or_live_meter",
            evidence_scope: "exact canonical events grouped by exact session/capture/connection/stream wire provenance",
            ordering_scope: "the decoder emits BuffEffectSync before SkillEffects inside one entity delta, so wire status order proves the final stack snapshot but not per-hit causal order",
            ladder_scope: "a candidate ladder requires exact pre-wire and post-wire stack counts from one provider, one unchanged full damage identity, an ordered nondecreasing damage list, and exactly one distinct amount for every intervening stack count",
            formula_scope: "candidate models test positive integer base damage under floor or positive round-half-up fixed-point multiplication by (10000 + baseline_zone_raw + stack * configured_raw_value) / 10000",
            attribution_scope: "this tool proves a self-only formula component and never transfers it as rDPS",
            unresolved_evidence_is_hidden: false,
        },
        input_events: arguments.events,
        effect_id: arguments.effect_id,
        configured_raw_value_per_stack: arguments.raw_value_per_stack,
        provider_scope: if arguments.provider_is_attacker {
            "effect_source_must_equal_attributed_damage_source"
        } else {
            "all_effect_providers_must_resolve_to_one_exact_instance"
        },
        lines_read,
        exact_wire_groups: report.exact_wire_groups,
        selected_status_events: report.selected_status_events,
        wires_with_selected_status: report.wires_with_selected_status,
        wires_with_exact_stack_increase: report.wires_with_exact_stack_increase,
        damage_identity_groups_considered: report.damage_identity_groups_considered,
        unresolved_multiple_selected_instances: report.unresolved_multiple_selected_instances,
        rejected_non_ladder_groups: report.rejected_non_ladder_groups,
        ladders: report.ladders,
    };
    serde_json::to_writer_pretty(BufWriter::new(File::create(&arguments.output)?), &bundle)?;
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    events: PathBuf,
    output: PathBuf,
    effect_id: i64,
    raw_value_per_stack: i64,
    provider_is_attacker: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WireKey {
    session_id: String,
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ActiveKey {
    target_entity_uuid: i64,
    source_entity_uuid: Option<i64>,
    instance_id: Option<i64>,
}

#[derive(Debug, Clone, Default)]
struct StackState {
    active: BTreeMap<ActiveKey, u32>,
}

impl StackState {
    fn observe(&mut self, status: &StatusEvent) {
        let key = ActiveKey {
            target_entity_uuid: status.target.entity_uuid.0,
            source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
            instance_id: status.instance_id.map(|instance| instance.0),
        };
        match status.state {
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                self.active.insert(key, status.stacks.unwrap_or(1));
            }
            StatusState::Consumed if status.stacks.is_some_and(|stacks| stacks > 0) => {
                self.active.insert(key, status.stacks.unwrap_or(1));
            }
            StatusState::Consumed | StatusState::Removed => {
                self.active.remove(&key);
            }
        }
    }

    fn exact_stacks(&self, target: i64, provider: Option<i64>) -> Option<u32> {
        let mut matching = self.active.iter().filter(|(key, _)| {
            key.target_entity_uuid == target
                && provider.is_none_or(|expected| key.source_entity_uuid == Some(expected))
        });
        let Some((_, stacks)) = matching.next() else {
            return Some(0);
        };
        if matching.next().is_some() {
            return None;
        }
        Some(*stacks)
    }
}

#[derive(Debug)]
struct WireAccumulator {
    key: WireKey,
    first_envelope_sequence: u64,
    last_envelope_sequence: u64,
    state_before: StackState,
    statuses: Vec<StatusObservation>,
    damages: Vec<DamageObservation>,
}

#[derive(Debug, Clone, Serialize)]
struct StatusObservation {
    envelope_sequence: u64,
    timeline_sequence: u64,
    source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    state: StatusState,
    stacks: Option<u32>,
    instance_id: Option<i64>,
    source_type_id: Option<i32>,
    source_config_id: Option<i64>,
}

impl StatusObservation {
    fn from_event(envelope_sequence: u64, timeline_sequence: u64, status: &StatusEvent) -> Self {
        Self {
            envelope_sequence,
            timeline_sequence,
            source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
            target_entity_uuid: status.target.entity_uuid.0,
            state: status.state,
            stacks: status.stacks,
            instance_id: status.instance_id.map(|instance| instance.0),
            source_type_id: status.origin.map(|origin| origin.source_type_id),
            source_config_id: status.origin.map(|origin| origin.source_config_id),
        }
    }
}

#[derive(Debug, Clone)]
struct DamageObservation {
    envelope_sequence: u64,
    timeline_sequence: u64,
    identity: DamageIdentity,
    amount: i64,
    normal_value: Option<i64>,
}

impl DamageObservation {
    fn from_event(envelope_sequence: u64, timeline_sequence: u64, damage: &DamageEvent) -> Self {
        Self {
            envelope_sequence,
            timeline_sequence,
            identity: DamageIdentity::from_event(damage),
            amount: damage.amount,
            normal_value: damage.packet.normal_value,
        }
    }

    fn formula_amount(&self) -> i64 {
        self.normal_value.unwrap_or(self.amount)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct DamageIdentity {
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
    blocked: Option<bool>,
    missed: Option<bool>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    position_bits: [Option<u32>; 3],
    hit_parts: Vec<HitPartIdentity>,
    damage_weight_bits: [Option<u32>; 3],
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
}

impl DamageIdentity {
    fn from_event(damage: &DamageEvent) -> Self {
        Self {
            source_entity_uuid: damage.source.entity_uuid.0,
            direct_source_entity_uuid: damage.direct_source.map(|source| source.entity_uuid.0),
            target_entity_uuid: damage.target.entity_uuid.0,
            ability_id: damage.ability.map(|ability| ability.0),
            hit_event_id: damage.hit_event_id,
            damage_source: damage.damage_source,
            damage_type: damage.damage_type,
            critical: damage.flags.critical,
            lucky: damage.flags.lucky,
            causes_lucky: damage.flags.causes_lucky,
            blocked: damage.flags.blocked,
            missed: damage.packet.missed,
            reported_critical: damage.packet.reported_critical,
            type_flags: damage.packet.type_flags,
            owner_level: damage.packet.owner_level,
            owner_stage: damage.packet.owner_stage,
            normal_hit: damage.packet.normal_hit,
            property: damage.packet.property,
            position_bits: position_bits(damage.packet.position.as_ref()),
            hit_parts: damage
                .packet
                .hit_parts
                .iter()
                .map(|part| HitPartIdentity {
                    part_id: part.part_id,
                    position_bits: position_bits(part.position.as_ref()),
                    damage_value: part.damage_value,
                })
                .collect(),
            damage_weight_bits: position_bits(damage.packet.damage_weight.as_ref()),
            passive_uuid: damage.packet.passive_uuid,
            rainbow: damage.packet.rainbow,
            damage_mode: damage.packet.damage_mode,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct HitPartIdentity {
    part_id: Option<i32>,
    position_bits: [Option<u32>; 3],
    damage_value: Option<i64>,
}

fn position_bits(position: Option<&rlogs_events::DamagePosition>) -> [Option<u32>; 3] {
    position.map_or([None, None, None], |position| {
        [
            position.x.map(f32::to_bits),
            position.y.map(f32::to_bits),
            position.z.map(f32::to_bits),
        ]
    })
}

#[derive(Debug, Default)]
struct ReportAccumulator {
    exact_wire_groups: u64,
    selected_status_events: u64,
    wires_with_selected_status: u64,
    wires_with_exact_stack_increase: u64,
    damage_identity_groups_considered: u64,
    unresolved_multiple_selected_instances: u64,
    rejected_non_ladder_groups: u64,
    ladders: Vec<StackLadder>,
}

fn flush_wire(
    wire: WireAccumulator,
    state_after: &mut StackState,
    arguments: &Arguments,
    report: &mut ReportAccumulator,
) {
    report.exact_wire_groups = report.exact_wire_groups.saturating_add(1);
    report.selected_status_events = report
        .selected_status_events
        .saturating_add(wire.statuses.len() as u64);
    if !wire.statuses.is_empty() {
        report.wires_with_selected_status = report.wires_with_selected_status.saturating_add(1);
    }

    let mut by_identity = BTreeMap::<DamageIdentity, Vec<DamageObservation>>::new();
    for damage in wire.damages {
        by_identity
            .entry(damage.identity.clone())
            .or_default()
            .push(damage);
    }
    let mut wire_had_exact_increase = false;
    for (identity, damages) in by_identity {
        report.damage_identity_groups_considered =
            report.damage_identity_groups_considered.saturating_add(1);
        let provider = arguments
            .provider_is_attacker
            .then_some(identity.source_entity_uuid);
        let before = wire
            .state_before
            .exact_stacks(identity.target_entity_uuid, provider);
        let after = state_after.exact_stacks(identity.target_entity_uuid, provider);
        let (Some(before), Some(after)) = (before, after) else {
            report.unresolved_multiple_selected_instances = report
                .unresolved_multiple_selected_instances
                .saturating_add(1);
            continue;
        };
        if after <= before {
            continue;
        }
        wire_had_exact_increase = true;
        let ordered_amounts = damages
            .iter()
            .map(DamageObservation::formula_amount)
            .collect::<Vec<_>>();
        let distinct_amounts = collapse_consecutive(&ordered_amounts);
        let expected_points = usize::try_from(after - before + 1).unwrap_or(usize::MAX);
        if distinct_amounts.len() != expected_points
            || distinct_amounts.iter().any(|amount| *amount <= 0)
            || !distinct_amounts.windows(2).all(|pair| pair[0] < pair[1])
        {
            report.rejected_non_ladder_groups = report.rejected_non_ladder_groups.saturating_add(1);
            continue;
        }
        let stack_amounts = distinct_amounts
            .iter()
            .enumerate()
            .map(|(index, amount)| StackAmount {
                stacks: before + u32::try_from(index).unwrap_or(u32::MAX),
                amount: *amount,
            })
            .collect::<Vec<_>>();
        let model_candidates = model_candidates(&stack_amounts, arguments.raw_value_per_stack);
        report.ladders.push(StackLadder {
            session_id: wire.key.session_id.clone(),
            capture_sequence: wire.key.capture_sequence,
            connection_id: wire.key.connection_id,
            stream_id: wire.key.stream_id,
            first_envelope_sequence: wire.first_envelope_sequence,
            last_envelope_sequence: wire.last_envelope_sequence,
            identity: identity.clone(),
            exact_pre_wire_stacks: before,
            exact_post_wire_stacks: after,
            ordered_damage_sequences: damages
                .iter()
                .map(|damage| DamageSequence {
                    envelope_sequence: damage.envelope_sequence,
                    timeline_sequence: damage.timeline_sequence,
                    amount: damage.amount,
                    normal_value: damage.normal_value,
                })
                .collect(),
            selected_status_events: wire
                .statuses
                .iter()
                .filter(|status| {
                    status.target_entity_uuid == identity.target_entity_uuid
                        && provider
                            .is_none_or(|provider| status.source_entity_uuid == Some(provider))
                })
                .cloned()
                .collect(),
            stack_amounts,
            observed_adjacent_increments: distinct_amounts
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .collect(),
            formula_model_candidates_truncated: model_candidates.len() == MAXIMUM_MODEL_CANDIDATES,
            formula_model_candidates: model_candidates,
        });
    }
    if wire_had_exact_increase {
        report.wires_with_exact_stack_increase =
            report.wires_with_exact_stack_increase.saturating_add(1);
    }
}

fn collapse_consecutive(amounts: &[i64]) -> Vec<i64> {
    let mut collapsed = Vec::new();
    for amount in amounts {
        if collapsed.last() != Some(amount) {
            collapsed.push(*amount);
        }
    }
    collapsed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum RoundingModel {
    Floor,
    PositiveRoundHalfUp,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct FormulaModelCandidate {
    rounding: RoundingModel,
    integer_base_damage: i64,
    baseline_zone_raw: i64,
}

fn model_candidates(points: &[StackAmount], magnitude: i64) -> Vec<FormulaModelCandidate> {
    if points.len() < 2 || magnitude <= 0 {
        return Vec::new();
    }
    let mut lower = 1_i64;
    let mut upper = i64::MAX;
    for pair in points.windows(2) {
        let difference = pair[1].amount - pair[0].amount;
        if difference <= 0 {
            return Vec::new();
        }
        lower = lower.max(((difference - 1).max(0) * PERCENT_SCALE / magnitude).max(1));
        upper = upper.min((difference + 1) * PERCENT_SCALE / magnitude + 2);
    }
    if lower > upper || upper - lower > 1_000_000 {
        return Vec::new();
    }

    let first = &points[0];
    let mut candidates = BTreeSet::new();
    for base in lower..=upper {
        let estimate = (first.amount * PERCENT_SCALE / base)
            - PERCENT_SCALE
            - i64::from(first.stacks) * magnitude;
        for baseline in (estimate - 12)..=(estimate + 12) {
            for rounding in [RoundingModel::Floor, RoundingModel::PositiveRoundHalfUp] {
                if points.iter().all(|point| {
                    predict(base, baseline, point.stacks, magnitude, rounding) == Some(point.amount)
                }) {
                    candidates.insert(FormulaModelCandidate {
                        rounding,
                        integer_base_damage: base,
                        baseline_zone_raw: baseline,
                    });
                    if candidates.len() >= MAXIMUM_MODEL_CANDIDATES {
                        return candidates.into_iter().collect();
                    }
                }
            }
        }
    }
    candidates.into_iter().collect()
}

fn predict(
    base: i64,
    baseline: i64,
    stacks: u32,
    magnitude: i64,
    rounding: RoundingModel,
) -> Option<i64> {
    let factor = i128::from(PERCENT_SCALE)
        .checked_add(i128::from(baseline))?
        .checked_add(i128::from(stacks).checked_mul(i128::from(magnitude))?)?;
    let numerator = i128::from(base).checked_mul(factor)?;
    if numerator <= 0 {
        return None;
    }
    let adjusted = match rounding {
        RoundingModel::Floor => numerator,
        RoundingModel::PositiveRoundHalfUp => {
            numerator.checked_add(i128::from(PERCENT_SCALE / 2))?
        }
    };
    i64::try_from(adjusted.checked_div(i128::from(PERCENT_SCALE))?).ok()
}

#[derive(Debug, Serialize)]
struct ProofBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: ProofPolicy,
    input_events: PathBuf,
    effect_id: i64,
    configured_raw_value_per_stack: i64,
    provider_scope: &'static str,
    lines_read: u64,
    exact_wire_groups: u64,
    selected_status_events: u64,
    wires_with_selected_status: u64,
    wires_with_exact_stack_increase: u64,
    damage_identity_groups_considered: u64,
    unresolved_multiple_selected_instances: u64,
    rejected_non_ladder_groups: u64,
    ladders: Vec<StackLadder>,
}

#[derive(Debug, Serialize)]
struct ProofPolicy {
    runtime_use: &'static str,
    evidence_scope: &'static str,
    ordering_scope: &'static str,
    ladder_scope: &'static str,
    formula_scope: &'static str,
    attribution_scope: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct StackLadder {
    session_id: String,
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
    first_envelope_sequence: u64,
    last_envelope_sequence: u64,
    identity: DamageIdentity,
    exact_pre_wire_stacks: u32,
    exact_post_wire_stacks: u32,
    ordered_damage_sequences: Vec<DamageSequence>,
    selected_status_events: Vec<StatusObservation>,
    stack_amounts: Vec<StackAmount>,
    observed_adjacent_increments: Vec<i64>,
    formula_model_candidates_truncated: bool,
    formula_model_candidates: Vec<FormulaModelCandidate>,
}

#[derive(Debug, Serialize)]
struct DamageSequence {
    envelope_sequence: u64,
    timeline_sequence: u64,
    amount: i64,
    normal_value: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct StackAmount {
    stacks: u32,
    amount: i64,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = std::env::args_os().skip(1).collect::<Vec<_>>();
    let events = PathBuf::from(take_value(&mut values, "--events")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let effect_id = parse_i64(take_value(&mut values, "--effect")?, "--effect")?;
    let raw_value_per_stack = parse_i64(
        take_value(&mut values, "--raw-value-per-stack")?,
        "--raw-value-per-stack",
    )?;
    let provider_is_attacker = take_switch(&mut values, "--provider-is-attacker");
    if !values.is_empty() || effect_id <= 0 || raw_value_per_stack <= 0 {
        return Err(usage());
    }
    Ok(Arguments {
        events,
        output,
        effect_id,
        raw_value_per_stack,
        provider_is_attacker,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Err(usage());
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn take_switch(values: &mut Vec<OsString>, flag: &str) -> bool {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return false;
    };
    values.remove(position);
    true
}

fn parse_i64(value: OsString, flag: &str) -> Result<i64, String> {
    value
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| format!("{flag} requires an integer"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-module-stack-formula-proof --events <canonical.jsonl> --effect <status-id> --raw-value-per-stack <fixed-point-raw> --provider-is-attacker --output <proof.json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        FormulaModelCandidate, RoundingModel, StackAmount, collapse_consecutive, model_candidates,
    };

    #[test]
    fn collapse_keeps_order_and_repeated_cap_hits_without_inventing_more_stacks() {
        assert_eq!(
            collapse_consecutive(&[29_365, 29_993, 29_993]),
            [29_365, 29_993]
        );
    }

    #[test]
    fn packet_ladder_accepts_the_exact_275_point_model_with_known_1000_point_zone() {
        let candidates = model_candidates(
            &[
                StackAmount {
                    stacks: 0,
                    amount: 33_395,
                },
                StackAmount {
                    stacks: 1,
                    amount: 34_230,
                },
                StackAmount {
                    stacks: 2,
                    amount: 35_065,
                },
            ],
            275,
        );
        assert!(candidates.contains(&FormulaModelCandidate {
            rounding: RoundingModel::PositiveRoundHalfUp,
            integer_base_damage: 30_359,
            baseline_zone_raw: 1_000,
        }));
    }

    #[test]
    fn packet_ladder_retains_all_exact_integer_models_instead_of_guessing_one() {
        let candidates = model_candidates(
            &[
                StackAmount {
                    stacks: 3,
                    amount: 29_365,
                },
                StackAmount {
                    stacks: 4,
                    amount: 29_993,
                },
            ],
            275,
        );
        assert!(candidates.contains(&FormulaModelCandidate {
            rounding: RoundingModel::PositiveRoundHalfUp,
            integer_base_damage: 22_836,
            baseline_zone_raw: 2_034,
        }));
        assert!(candidates.len() > 1);
    }
}
