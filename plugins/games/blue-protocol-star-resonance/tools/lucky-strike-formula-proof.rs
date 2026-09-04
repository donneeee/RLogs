use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, EvidenceSource, RunState,
    TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 6;
const DEFAULT_LUCKY_ABILITY_ID: i64 = 2_031_109;
const DEFAULT_LUCKY_HIT_EVENT_ID: i32 = 3;
const ATTACK_ATTRIBUTE_ID: i32 = 11_330;
const MAGIC_ATTACK_ATTRIBUTE_ID: i32 = 11_340;
const LUCKY_PROBABILITY_ATTRIBUTE_ID: i32 = 11_780;
const LUCKY_DAMAGE_ATTRIBUTE_ID: i32 = 12_530;
const DEFAULT_EXAMPLE_LIMIT: usize = 24;

#[derive(Debug)]
struct Arguments {
    game_build: String,
    lucky_ability_id: i64,
    lucky_hit_event_id: i32,
    surface: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    example_limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WireKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone)]
struct DamageRow {
    damage_id: String,
    damage_script: Option<String>,
    pve_damage_ratio: Vec<i64>,
}

#[derive(Debug, Default)]
struct DamageSurface {
    rows_by_key: BTreeMap<(i64, i32), Vec<DamageRow>>,
}

#[derive(Debug, Clone)]
struct Hit {
    rlog: String,
    session_id: String,
    sequence: u64,
    wire: WireKey,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: i32,
    amount: i64,
    critical: Option<bool>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
    owner_stage: Option<i32>,
    group_index: Option<u32>,
    component_index: Option<u32>,
    component_count: Option<u32>,
    lucky_probability: Option<i64>,
    lucky_damage_multiplier: Option<i64>,
    attack: Option<i64>,
    magic_attack: Option<i64>,
    local_source_attributes: Option<Vec<(i32, i64)>>,
    damage_row: Option<DamageRow>,
}

#[derive(Debug, Default)]
struct Accumulator {
    wire_messages: u64,
    lucky_events: u64,
    lucky_observed_damage: i128,
    events_with_packet_multiplier: u64,
    events_with_packet_attack: u64,
    events_with_packet_magic_attack: u64,
    events_with_one_same_group_parent: u64,
    events_with_one_adjacent_parent: u64,
    events_with_one_immediate_following_parent: u64,
    events_resolved_by_adjacency: u64,
    events_with_ambiguous_parent: u64,
    events_without_parent: u64,
    parent_identity_counts: BTreeMap<String, u64>,
    parent_proportional: FormulaAccumulator,
    parent_multiplier: FormulaAccumulator,
    attack_multiplier: FormulaAccumulator,
    magic_attack_multiplier: FormulaAccumulator,
    relation_groups: BTreeMap<RelationKey, RelationGroupAccumulator>,
    source_attribute_candidate_events: u64,
    source_attribute_candidate_pairs: u64,
    source_attribute_candidates: BTreeMap<i32, SourceAttributeCandidateAccumulator>,
    examples: Vec<PairExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RelationKey {
    parent_ability_id: i64,
    parent_hit_event_id: i32,
    parent_owner_stage: Option<i32>,
    parent_critical: Option<bool>,
    lucky_critical: Option<bool>,
}

#[derive(Debug, Default)]
struct RelationGroupAccumulator {
    events: u64,
    attack_values: BTreeSet<i64>,
    multiplier_values: BTreeSet<i64>,
    lucky_amounts: BTreeSet<i64>,
    attack_multiplier: FormulaAccumulator,
}

#[derive(Debug, Default)]
struct SourceAttributeCandidateAccumulator {
    events_present: u64,
    values: BTreeSet<i64>,
    relation_group_values: BTreeMap<RelationKey, BTreeSet<i64>>,
    multiplier_formula: FormulaAccumulator,
}

#[derive(Debug, Clone, Default)]
struct FormulaAccumulator {
    events: u64,
    exact_events: u64,
    absolute_residual_sum: u128,
    maximum_absolute_residual: u128,
    minimum_residual: Option<i128>,
    maximum_residual: Option<i128>,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    selection: Selection,
    policy: Policy,
    inputs: Inputs,
    coverage: Coverage,
    parent_identity_counts: BTreeMap<String, u64>,
    formula_tests: FormulaTests,
    relation_groups: Vec<RelationGroupResult>,
    source_attribute_candidates: Vec<SourceAttributeCandidateResult>,
    examples: Vec<PairExample>,
}

#[derive(Debug, Serialize)]
struct Selection {
    lucky_ability_id: i64,
    lucky_hit_event_id: i32,
    lucky_flag_required: bool,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_numeric_build_is_authoritative: bool,
    local_or_offline_evidence_only: bool,
    remote_player_only_packets_are_required: bool,
    remote_player_only_packets_are_treated_as_zero: bool,
    remote_player_only_packets_are_synthesized: bool,
    current_character_snapshot_substitution_allowed: bool,
    rlogs_are_streamed_one_at_a_time: bool,
    wire_group_state_is_bounded_to_one_wire_message: bool,
    examples_are_bounded: bool,
    occurrence_authority: &'static str,
    parent_selection: &'static str,
    multiplier_authority: &'static str,
    formula_authority: bool,
    source_attribute_candidate_family_authority: bool,
    local_source_attribute_inventory_scope: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct Coverage {
    source_rlog_count: usize,
    wire_messages: u64,
    lucky_events: u64,
    lucky_observed_damage: String,
    events_with_packet_multiplier: u64,
    events_with_packet_attack: u64,
    events_with_packet_magic_attack: u64,
    events_with_one_same_group_parent: u64,
    events_with_one_adjacent_parent: u64,
    events_with_one_immediate_following_parent: u64,
    events_resolved_by_adjacency: u64,
    events_with_ambiguous_parent: u64,
    events_without_parent: u64,
    source_attribute_candidate_events: u64,
    source_attribute_candidate_pairs: u64,
}

#[derive(Debug, Serialize)]
struct Inputs {
    damage_surface: InputDescriptor,
    rlogs: Vec<InputDescriptor>,
}

#[derive(Debug, Serialize)]
struct InputDescriptor {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct FormulaTests {
    parent_coefficient_proportional: FormulaResult,
    parent_amount_times_lucky_multiplier: FormulaResult,
    source_attack_times_lucky_multiplier: FormulaResult,
    source_magic_attack_times_lucky_multiplier: FormulaResult,
}

#[derive(Debug, Serialize)]
struct FormulaResult {
    expression: String,
    events: u64,
    exact_events: u64,
    absolute_residual_sum: String,
    maximum_absolute_residual: String,
    minimum_residual: Option<String>,
    maximum_residual: Option<String>,
}

#[derive(Debug, Serialize)]
struct SourceAttributeCandidateResult {
    attribute_id: i32,
    events_present: u64,
    missing_candidate_events: u64,
    distinct_values: usize,
    minimum_value: Option<String>,
    maximum_value: Option<String>,
    relation_groups_present: usize,
    relation_groups_with_within_group_variation: usize,
    floor_attribute_times_lucky_multiplier: FormulaResult,
}

#[derive(Debug, Serialize)]
struct RelationGroupResult {
    parent_ability_id: i64,
    parent_hit_event_id: i32,
    parent_owner_stage: Option<i32>,
    parent_critical: Option<bool>,
    lucky_critical: Option<bool>,
    events: u64,
    distinct_attack_values: usize,
    distinct_lucky_multiplier_values: usize,
    distinct_lucky_amounts: usize,
    source_attack_multiplier_exact_events: u64,
    source_attack_multiplier_minimum_residual: Option<String>,
    source_attack_multiplier_maximum_residual: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PairExample {
    rlog: String,
    session_id: String,
    sequence: u64,
    wire_capture_sequence: u64,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    group_index: Option<u32>,
    lucky_component_index: Option<u32>,
    parent_component_index: Option<u32>,
    component_count: Option<u32>,
    lucky_amount: i64,
    lucky_probability: Option<i64>,
    lucky_damage_multiplier: Option<i64>,
    source_attack: Option<i64>,
    source_magic_attack: Option<i64>,
    parent_ability_id: i64,
    parent_hit_event_id: i32,
    parent_amount: i64,
    parent_owner_stage: Option<i32>,
    parent_damage_id: Option<String>,
    parent_damage_script: Option<String>,
    parent_coefficient: Option<i64>,
    parent_proportional_prediction: Option<i64>,
    parent_proportional_residual: Option<i64>,
    parent_multiplier_prediction: Option<i64>,
    parent_multiplier_residual: Option<i64>,
    source_attack_multiplier_prediction: Option<i64>,
    source_attack_multiplier_residual: Option<i64>,
    source_magic_attack_multiplier_prediction: Option<i64>,
    source_magic_attack_multiplier_residual: Option<i64>,
    parent_critical: Option<bool>,
    lucky_critical: Option<bool>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lucky strike formula proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let surface = load_damage_surface(&arguments.surface, &arguments.game_build)?;
    let mut accumulator = Accumulator::default();
    for rlog in &arguments.rlogs {
        read_session(
            rlog,
            &arguments.game_build,
            arguments.lucky_ability_id,
            arguments.lucky_hit_event_id,
            &surface,
            &mut accumulator,
            arguments.example_limit,
        )?;
    }

    let relation_groups = relation_group_results(&accumulator.relation_groups);
    let source_attribute_candidates = source_attribute_candidate_results(
        &accumulator.source_attribute_candidates,
        accumulator.source_attribute_candidate_events,
    );
    let report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-lucky-strike-formula-proof",
        game_build: arguments.game_build.clone(),
        selection: Selection {
            lucky_ability_id: arguments.lucky_ability_id,
            lucky_hit_event_id: arguments.lucky_hit_event_id,
            lucky_flag_required: true,
        },
        policy: Policy {
            exact_numeric_build_is_authoritative: true,
            local_or_offline_evidence_only: true,
            remote_player_only_packets_are_required: false,
            remote_player_only_packets_are_treated_as_zero: false,
            remote_player_only_packets_are_synthesized: false,
            current_character_snapshot_substitution_allowed: false,
            rlogs_are_streamed_one_at_a_time: true,
            wire_group_state_is_bounded_to_one_wire_message: true,
            examples_are_bounded: true,
            occurrence_authority: "current-decoder canonical packet events",
            parent_selection: "same wire, source, target, packet group, component count, causes_lucky=true; exact adjacency is reported separately",
            multiplier_authority: "packet-snapshotted AttrLuckDamInc (12530), interpreted at the proven 1/10000 fixed-point scale",
            formula_authority: false,
            source_attribute_candidate_family_authority: false,
            local_source_attribute_inventory_scope: "exact locally observed source actor attribute state at the Lucky Strike event; structurally unavailable remote-player-only packets are neither required nor inferred",
            unresolved_evidence_is_hidden: false,
        },
        inputs: Inputs {
            damage_surface: input_descriptor(&arguments.surface)?,
            rlogs: arguments
                .rlogs
                .iter()
                .map(|path| input_descriptor(path))
                .collect::<Result<Vec<_>, _>>()?,
        },
        coverage: Coverage {
            source_rlog_count: arguments.rlogs.len(),
            wire_messages: accumulator.wire_messages,
            lucky_events: accumulator.lucky_events,
            lucky_observed_damage: accumulator.lucky_observed_damage.to_string(),
            events_with_packet_multiplier: accumulator.events_with_packet_multiplier,
            events_with_packet_attack: accumulator.events_with_packet_attack,
            events_with_packet_magic_attack: accumulator.events_with_packet_magic_attack,
            events_with_one_same_group_parent: accumulator.events_with_one_same_group_parent,
            events_with_one_adjacent_parent: accumulator.events_with_one_adjacent_parent,
            events_with_one_immediate_following_parent: accumulator
                .events_with_one_immediate_following_parent,
            events_resolved_by_adjacency: accumulator.events_resolved_by_adjacency,
            events_with_ambiguous_parent: accumulator.events_with_ambiguous_parent,
            events_without_parent: accumulator.events_without_parent,
            source_attribute_candidate_events: accumulator.source_attribute_candidate_events,
            source_attribute_candidate_pairs: accumulator.source_attribute_candidate_pairs,
        },
        parent_identity_counts: accumulator.parent_identity_counts,
        formula_tests: FormulaTests {
            parent_coefficient_proportional: formula_result(
                "floor(parent_amount * AttrLuckDamInc / selected_parent_PVEDamageRadio)",
                accumulator.parent_proportional,
            ),
            parent_amount_times_lucky_multiplier: formula_result(
                "floor(parent_amount * AttrLuckDamInc / 10000)",
                accumulator.parent_multiplier,
            ),
            source_attack_times_lucky_multiplier: formula_result(
                "floor(AttrAttack * AttrLuckDamInc / 10000)",
                accumulator.attack_multiplier,
            ),
            source_magic_attack_times_lucky_multiplier: formula_result(
                "floor(AttrMAttack * AttrLuckDamInc / 10000)",
                accumulator.magic_attack_multiplier,
            ),
        },
        relation_groups,
        source_attribute_candidates,
        examples: accumulator.examples,
    };
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", arguments.output.display());
    Ok(())
}

fn read_session(
    path: &Path,
    expected_game_build: &str,
    lucky_ability_id: i64,
    lucky_hit_event_id: i32,
    surface: &DamageSurface,
    accumulator: &mut Accumulator,
    example_limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut active_wire = None::<WireKey>;
    let mut wire_hits = Vec::<Hit>::new();
    let mut current_run_ordinal = 0_u32;
    let mut attributes = HashMap::<(u32, i64), BTreeMap<i32, i64>>::new();
    while let Some(envelope) = reader.next_event()? {
        if envelope.region.client_build != expected_game_build {
            return Err(format!(
                "{} contains client build {} but --build requires {expected_game_build}",
                path.display(),
                envelope.region.client_build
            )
            .into());
        }
        let wire = wire_key(&envelope.provenance.source);
        if wire != active_wire {
            flush_wire(
                &wire_hits,
                lucky_ability_id,
                lucky_hit_event_id,
                accumulator,
                example_limit,
            );
            if !wire_hits.is_empty() {
                accumulator.wire_messages = accumulator.wire_messages.saturating_add(1);
            }
            active_wire = wire;
            wire_hits.clear();
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => current_run_ordinal = current_run_ordinal.saturating_add(1),
                RunState::Started if current_run_ordinal == 0 => current_run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::EntityAttributes(event) => {
                let snapshot = attributes
                    .entry((current_run_ordinal, event.actor.entity_uuid.0))
                    .or_default();
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    snapshot.clear();
                }
                for attribute in &event.attributes {
                    if let Some(value) = decode_attribute(attribute) {
                        snapshot.insert(attribute.attribute_id, value);
                    }
                }
            }
            TimelineEventKind::Damage(damage) => {
                let (Some(wire), Some(ability)) = (wire, damage.ability) else {
                    continue;
                };
                let hit_event_id = damage.hit_event_id.unwrap_or_default();
                let source_entity_uuid = damage.source.entity_uuid.0;
                let source_attributes = attributes.get(&(current_run_ordinal, source_entity_uuid));
                let damage_row = surface
                    .rows_by_key
                    .get(&(ability.0, hit_event_id))
                    .filter(|rows| rows.len() == 1)
                    .and_then(|rows| rows.first())
                    .cloned();
                wire_hits.push(Hit {
                    rlog: file_label(path),
                    session_id: envelope.session_id.clone(),
                    sequence: envelope.sequence,
                    wire,
                    run_ordinal: current_run_ordinal,
                    source_entity_uuid,
                    target_entity_uuid: damage.target.entity_uuid.0,
                    ability_id: ability.0,
                    hit_event_id,
                    amount: damage.amount,
                    critical: damage.flags.critical,
                    lucky: damage.flags.lucky,
                    causes_lucky: damage.flags.causes_lucky,
                    owner_stage: damage.packet.owner_stage,
                    group_index: damage.packet.skill_effect_group_index,
                    component_index: damage.packet.skill_effect_component_index,
                    component_count: damage.packet.skill_effect_component_count,
                    lucky_probability: source_attributes
                        .and_then(|values| values.get(&LUCKY_PROBABILITY_ATTRIBUTE_ID))
                        .copied(),
                    lucky_damage_multiplier: source_attributes
                        .and_then(|values| values.get(&LUCKY_DAMAGE_ATTRIBUTE_ID))
                        .copied(),
                    attack: source_attributes
                        .and_then(|values| values.get(&ATTACK_ATTRIBUTE_ID))
                        .copied(),
                    magic_attack: source_attributes
                        .and_then(|values| values.get(&MAGIC_ATTACK_ATTRIBUTE_ID))
                        .copied(),
                    local_source_attributes: if ability.0 == lucky_ability_id
                        && hit_event_id == lucky_hit_event_id
                        && damage.flags.lucky == Some(true)
                    {
                        source_attributes.map(|values| {
                            values
                                .iter()
                                .map(|(attribute_id, value)| (*attribute_id, *value))
                                .collect()
                        })
                    } else {
                        None
                    },
                    damage_row,
                });
            }
            _ => {}
        }
    }
    flush_wire(
        &wire_hits,
        lucky_ability_id,
        lucky_hit_event_id,
        accumulator,
        example_limit,
    );
    if !wire_hits.is_empty() {
        accumulator.wire_messages = accumulator.wire_messages.saturating_add(1);
    }
    Ok(())
}

fn flush_wire(
    hits: &[Hit],
    lucky_ability_id: i64,
    lucky_hit_event_id: i32,
    accumulator: &mut Accumulator,
    example_limit: usize,
) {
    for lucky in hits.iter().filter(|hit| {
        hit.ability_id == lucky_ability_id
            && hit.hit_event_id == lucky_hit_event_id
            && hit.lucky == Some(true)
    }) {
        accumulator.lucky_events = accumulator.lucky_events.saturating_add(1);
        accumulator.lucky_observed_damage = accumulator
            .lucky_observed_damage
            .saturating_add(i128::from(lucky.amount));
        if lucky.lucky_damage_multiplier.is_some() {
            accumulator.events_with_packet_multiplier =
                accumulator.events_with_packet_multiplier.saturating_add(1);
        }
        if lucky.attack.is_some() {
            accumulator.events_with_packet_attack =
                accumulator.events_with_packet_attack.saturating_add(1);
        }
        if lucky.magic_attack.is_some() {
            accumulator.events_with_packet_magic_attack = accumulator
                .events_with_packet_magic_attack
                .saturating_add(1);
        }
        let candidates = same_group_parent_candidates(lucky, hits);
        if candidates.is_empty() {
            accumulator.events_without_parent = accumulator.events_without_parent.saturating_add(1);
            continue;
        }
        if candidates.len() == 1 {
            accumulator.events_with_one_same_group_parent = accumulator
                .events_with_one_same_group_parent
                .saturating_add(1);
        }
        let adjacent_candidates = candidates
            .iter()
            .copied()
            .filter(|candidate| {
                components_are_adjacent(lucky.component_index, candidate.component_index)
            })
            .collect::<Vec<_>>();
        let following_candidates = candidates
            .iter()
            .copied()
            .filter(|candidate| {
                component_immediately_follows(lucky.component_index, candidate.component_index)
            })
            .collect::<Vec<_>>();
        let parent = if following_candidates.len() == 1 {
            accumulator.events_with_one_immediate_following_parent = accumulator
                .events_with_one_immediate_following_parent
                .saturating_add(1);
            if adjacent_candidates.len() == 1 {
                accumulator.events_with_one_adjacent_parent = accumulator
                    .events_with_one_adjacent_parent
                    .saturating_add(1);
            }
            if candidates.len() > 1 {
                accumulator.events_resolved_by_adjacency =
                    accumulator.events_resolved_by_adjacency.saturating_add(1);
            }
            following_candidates[0]
        } else if adjacent_candidates.len() == 1 {
            accumulator.events_with_one_adjacent_parent = accumulator
                .events_with_one_adjacent_parent
                .saturating_add(1);
            if candidates.len() > 1 {
                accumulator.events_resolved_by_adjacency =
                    accumulator.events_resolved_by_adjacency.saturating_add(1);
            }
            adjacent_candidates[0]
        } else if candidates.len() == 1 {
            candidates[0]
        } else {
            accumulator.events_with_ambiguous_parent =
                accumulator.events_with_ambiguous_parent.saturating_add(1);
            continue;
        };
        let identity = format!("{}:{}", parent.ability_id, parent.hit_event_id);
        *accumulator
            .parent_identity_counts
            .entry(identity)
            .or_default() += 1;
        let parent_coefficient = parent
            .damage_row
            .as_ref()
            .and_then(|row| select_stage_coefficient(&row.pve_damage_ratio, parent.owner_stage));
        let proportional_prediction = parent_coefficient
            .zip(lucky.lucky_damage_multiplier)
            .and_then(|(coefficient, multiplier)| {
                mul_div_floor(parent.amount, multiplier, coefficient)
            });
        let multiplier_prediction = lucky
            .lucky_damage_multiplier
            .and_then(|multiplier| mul_div_floor(parent.amount, multiplier, 10_000));
        let attack_multiplier_prediction = lucky
            .attack
            .zip(lucky.lucky_damage_multiplier)
            .and_then(|(attack, multiplier)| mul_div_floor(attack, multiplier, 10_000));
        let magic_attack_multiplier_prediction = lucky
            .magic_attack
            .zip(lucky.lucky_damage_multiplier)
            .and_then(|(attack, multiplier)| mul_div_floor(attack, multiplier, 10_000));
        let proportional_residual = proportional_prediction.map(|value| lucky.amount - value);
        let multiplier_residual = multiplier_prediction.map(|value| lucky.amount - value);
        let attack_multiplier_residual =
            attack_multiplier_prediction.map(|value| lucky.amount - value);
        let magic_attack_multiplier_residual =
            magic_attack_multiplier_prediction.map(|value| lucky.amount - value);
        if let Some(residual) = proportional_residual {
            accumulator.parent_proportional.observe(residual);
        }
        if let Some(residual) = multiplier_residual {
            accumulator.parent_multiplier.observe(residual);
        }
        if let Some(residual) = attack_multiplier_residual {
            accumulator.attack_multiplier.observe(residual);
        }
        if let Some(residual) = magic_attack_multiplier_residual {
            accumulator.magic_attack_multiplier.observe(residual);
        }
        let relation_key = RelationKey {
            parent_ability_id: parent.ability_id,
            parent_hit_event_id: parent.hit_event_id,
            parent_owner_stage: parent.owner_stage,
            parent_critical: parent.critical,
            lucky_critical: lucky.critical,
        };
        if let (Some(multiplier), Some(local_source_attributes)) = (
            lucky.lucky_damage_multiplier,
            lucky.local_source_attributes.as_deref(),
        ) {
            observe_source_attribute_candidates(
                accumulator,
                &relation_key,
                lucky.amount,
                multiplier,
                local_source_attributes,
            );
        }
        if let (Some(attack), Some(multiplier), Some(residual)) = (
            lucky.attack,
            lucky.lucky_damage_multiplier,
            attack_multiplier_residual,
        ) {
            let group = accumulator.relation_groups.entry(relation_key).or_default();
            group.events = group.events.saturating_add(1);
            group.attack_values.insert(attack);
            group.multiplier_values.insert(multiplier);
            group.lucky_amounts.insert(lucky.amount);
            group.attack_multiplier.observe(residual);
        }
        if accumulator.examples.len() < example_limit {
            accumulator.examples.push(PairExample {
                rlog: lucky.rlog.clone(),
                session_id: lucky.session_id.clone(),
                sequence: lucky.sequence,
                wire_capture_sequence: lucky.wire.capture_sequence,
                run_ordinal: lucky.run_ordinal,
                source_entity_uuid: lucky.source_entity_uuid,
                target_entity_uuid: lucky.target_entity_uuid,
                group_index: lucky.group_index,
                lucky_component_index: lucky.component_index,
                parent_component_index: parent.component_index,
                component_count: lucky.component_count,
                lucky_amount: lucky.amount,
                lucky_probability: lucky.lucky_probability,
                lucky_damage_multiplier: lucky.lucky_damage_multiplier,
                source_attack: lucky.attack,
                source_magic_attack: lucky.magic_attack,
                parent_ability_id: parent.ability_id,
                parent_hit_event_id: parent.hit_event_id,
                parent_amount: parent.amount,
                parent_owner_stage: parent.owner_stage,
                parent_damage_id: parent.damage_row.as_ref().map(|row| row.damage_id.clone()),
                parent_damage_script: parent
                    .damage_row
                    .as_ref()
                    .and_then(|row| row.damage_script.clone()),
                parent_coefficient,
                parent_proportional_prediction: proportional_prediction,
                parent_proportional_residual: proportional_residual,
                parent_multiplier_prediction: multiplier_prediction,
                parent_multiplier_residual: multiplier_residual,
                source_attack_multiplier_prediction: attack_multiplier_prediction,
                source_attack_multiplier_residual: attack_multiplier_residual,
                source_magic_attack_multiplier_prediction: magic_attack_multiplier_prediction,
                source_magic_attack_multiplier_residual: magic_attack_multiplier_residual,
                parent_critical: parent.critical,
                lucky_critical: lucky.critical,
            });
        }
    }
}

fn same_group_parent_candidates<'a>(lucky: &Hit, hits: &'a [Hit]) -> Vec<&'a Hit> {
    hits.iter()
        .filter(|candidate| {
            candidate.sequence != lucky.sequence
                && candidate.source_entity_uuid == lucky.source_entity_uuid
                && candidate.target_entity_uuid == lucky.target_entity_uuid
                && candidate.group_index == lucky.group_index
                && candidate.component_count == lucky.component_count
                && candidate.causes_lucky == Some(true)
        })
        .collect()
}

fn components_are_adjacent(left: Option<u32>, right: Option<u32>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left.abs_diff(right) == 1)
}

fn component_immediately_follows(left: Option<u32>, right: Option<u32>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left.checked_add(1) == Some(right))
}

fn select_stage_coefficient(values: &[i64], stage: Option<i32>) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let stage = stage.unwrap_or_default();
    if stage < 0 {
        return None;
    }
    if values.len() == 1 {
        return values.first().copied();
    }
    values.get(usize::try_from(stage).ok()?).copied()
}

fn mul_div_floor(value: i64, numerator: i64, denominator: i64) -> Option<i64> {
    if value < 0 || numerator < 0 || denominator <= 0 {
        return None;
    }
    i64::try_from(i128::from(value) * i128::from(numerator) / i128::from(denominator)).ok()
}

impl FormulaAccumulator {
    fn observe(&mut self, residual: i64) {
        let residual = i128::from(residual);
        self.events = self.events.saturating_add(1);
        if residual == 0 {
            self.exact_events = self.exact_events.saturating_add(1);
        }
        let absolute = residual.unsigned_abs();
        self.absolute_residual_sum = self.absolute_residual_sum.saturating_add(absolute);
        self.maximum_absolute_residual = self.maximum_absolute_residual.max(absolute);
        self.minimum_residual = Some(
            self.minimum_residual
                .map_or(residual, |value| value.min(residual)),
        );
        self.maximum_residual = Some(
            self.maximum_residual
                .map_or(residual, |value| value.max(residual)),
        );
    }
}

fn formula_result(expression: impl Into<String>, accumulator: FormulaAccumulator) -> FormulaResult {
    FormulaResult {
        expression: expression.into(),
        events: accumulator.events,
        exact_events: accumulator.exact_events,
        absolute_residual_sum: accumulator.absolute_residual_sum.to_string(),
        maximum_absolute_residual: accumulator.maximum_absolute_residual.to_string(),
        minimum_residual: accumulator.minimum_residual.map(|value| value.to_string()),
        maximum_residual: accumulator.maximum_residual.map(|value| value.to_string()),
    }
}

fn observe_source_attribute_candidates(
    accumulator: &mut Accumulator,
    relation_key: &RelationKey,
    lucky_amount: i64,
    lucky_multiplier: i64,
    local_source_attributes: &[(i32, i64)],
) {
    accumulator.source_attribute_candidate_events = accumulator
        .source_attribute_candidate_events
        .saturating_add(1);
    accumulator.source_attribute_candidate_pairs = accumulator
        .source_attribute_candidate_pairs
        .saturating_add(u64::try_from(local_source_attributes.len()).unwrap_or(u64::MAX));
    for &(attribute_id, value) in local_source_attributes {
        let candidate = accumulator
            .source_attribute_candidates
            .entry(attribute_id)
            .or_default();
        candidate.events_present = candidate.events_present.saturating_add(1);
        candidate.values.insert(value);
        candidate
            .relation_group_values
            .entry(relation_key.clone())
            .or_default()
            .insert(value);
        if let Some(prediction) = mul_div_floor(value, lucky_multiplier, 10_000) {
            candidate
                .multiplier_formula
                .observe(lucky_amount.saturating_sub(prediction));
        }
    }
}

fn source_attribute_candidate_results(
    candidates: &BTreeMap<i32, SourceAttributeCandidateAccumulator>,
    candidate_events: u64,
) -> Vec<SourceAttributeCandidateResult> {
    candidates
        .iter()
        .map(|(attribute_id, candidate)| SourceAttributeCandidateResult {
            attribute_id: *attribute_id,
            events_present: candidate.events_present,
            missing_candidate_events: candidate_events.saturating_sub(candidate.events_present),
            distinct_values: candidate.values.len(),
            minimum_value: candidate.values.first().map(ToString::to_string),
            maximum_value: candidate.values.last().map(ToString::to_string),
            relation_groups_present: candidate.relation_group_values.len(),
            relation_groups_with_within_group_variation: candidate
                .relation_group_values
                .values()
                .filter(|values| values.len() > 1)
                .count(),
            floor_attribute_times_lucky_multiplier: formula_result(
                format!("floor(Attr[{attribute_id}] * AttrLuckDamInc / 10000)"),
                candidate.multiplier_formula.clone(),
            ),
        })
        .collect()
}

fn relation_group_results(
    groups: &BTreeMap<RelationKey, RelationGroupAccumulator>,
) -> Vec<RelationGroupResult> {
    groups
        .iter()
        .map(|(key, group)| RelationGroupResult {
            parent_ability_id: key.parent_ability_id,
            parent_hit_event_id: key.parent_hit_event_id,
            parent_owner_stage: key.parent_owner_stage,
            parent_critical: key.parent_critical,
            lucky_critical: key.lucky_critical,
            events: group.events,
            distinct_attack_values: group.attack_values.len(),
            distinct_lucky_multiplier_values: group.multiplier_values.len(),
            distinct_lucky_amounts: group.lucky_amounts.len(),
            source_attack_multiplier_exact_events: group.attack_multiplier.exact_events,
            source_attack_multiplier_minimum_residual: group
                .attack_multiplier
                .minimum_residual
                .map(|value| value.to_string()),
            source_attack_multiplier_maximum_residual: group
                .attack_multiplier
                .maximum_residual
                .map(|value| value.to_string()),
        })
        .collect()
}

fn load_damage_surface(
    path: &Path,
    expected_game_build: &str,
) -> Result<DamageSurface, Box<dyn std::error::Error>> {
    let surface: Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let game_build = surface
        .get("game_build")
        .and_then(Value::as_str)
        .ok_or("damage surface is missing game_build")?;
    if game_build != expected_game_build {
        return Err(format!(
            "damage surface build {game_build} differs from --build {expected_game_build}"
        )
        .into());
    }
    let rows = surface
        .get("rows")
        .and_then(Value::as_object)
        .ok_or("damage surface is missing rows")?;
    let lookup = surface
        .get("linked_hit_event_candidate_lookup")
        .and_then(Value::as_object)
        .ok_or("damage surface is missing linked_hit_event_candidate_lookup")?;
    let mut rows_by_key = BTreeMap::new();
    for (key, candidate_ids) in lookup {
        let Some((ability, hit)) = key.split_once(':') else {
            continue;
        };
        let (Ok(ability_id), Ok(hit_event_id)) = (ability.parse::<i64>(), hit.parse::<i32>())
        else {
            continue;
        };
        let Some(candidate_ids) = candidate_ids.as_array() else {
            continue;
        };
        let mut candidates = Vec::new();
        for candidate_id in candidate_ids {
            let damage_id = match candidate_id {
                Value::String(value) => value.clone(),
                Value::Number(value) => value.to_string(),
                _ => continue,
            };
            let Some(row) = rows.get(&damage_id) else {
                continue;
            };
            candidates.push(DamageRow {
                damage_id,
                damage_script: row
                    .get("string_pool_6_candidates_by_offset")
                    .and_then(|value| value.get("24"))
                    .and_then(|value| value.get("value"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                pve_damage_ratio: surface_array_values(
                    row.get("int_array_pool_1_candidates_by_offset")
                        .and_then(|value| value.get("28")),
                )?,
            });
        }
        rows_by_key.insert((ability_id, hit_event_id), candidates);
    }
    Ok(DamageSurface { rows_by_key })
}

fn input_descriptor(path: &Path) -> Result<InputDescriptor, std::io::Error> {
    Ok(InputDescriptor {
        path: path.display().to_string(),
        bytes: fs::metadata(path)?.len(),
        sha256: sha256_file(path)?,
    })
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn surface_array_values(value: Option<&Value>) -> Result<Vec<i64>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "surface array candidate is missing values".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "surface array value is not an integer".to_owned())
        })
        .collect()
}

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
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
            return (index + 1 == bytes.len()).then_some(value);
        }
    }
    None
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

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let game_build = take_value(&mut values, "--build")?
        .to_string_lossy()
        .into_owned();
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".to_owned());
    }
    let surface = PathBuf::from(take_value(&mut values, "--surface")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let lucky_ability_id = take_optional_value(&mut values, "--lucky-ability-id")
        .map(|value| parse_i64(value, "--lucky-ability-id"))
        .transpose()?
        .unwrap_or(DEFAULT_LUCKY_ABILITY_ID);
    let lucky_hit_event_id = take_optional_value(&mut values, "--lucky-hit-event-id")
        .map(|value| parse_i32(value, "--lucky-hit-event-id"))
        .transpose()?
        .unwrap_or(DEFAULT_LUCKY_HIT_EVENT_ID);
    if lucky_ability_id <= 0 || lucky_hit_event_id < 0 {
        return Err("Lucky ability/hit identifiers must be positive/nonnegative".to_owned());
    }
    let example_limit = take_optional_value(&mut values, "--example-limit")
        .map(|value| parse_usize(value, "--example-limit"))
        .transpose()?
        .unwrap_or(DEFAULT_EXAMPLE_LIMIT);
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        values.remove(position);
        rlogs.push(PathBuf::from(values.remove(position)));
    }
    if rlogs.is_empty() {
        return Err("at least one --rlog is required".to_owned());
    }
    if !values.is_empty() {
        return Err(format!("unrecognized arguments: {values:?}"));
    }
    Ok(Arguments {
        game_build,
        lucky_ability_id,
        lucky_hit_event_id,
        surface,
        rlogs,
        output,
        example_limit,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let position = values
        .iter()
        .position(|value| value == flag)
        .ok_or_else(|| format!("missing {flag}"))?;
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

fn parse_usize(value: OsString, flag: &str) -> Result<usize, String> {
    value
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|error| format!("invalid {flag}: {error}"))
}

fn parse_i64(value: OsString, flag: &str) -> Result<i64, String> {
    value
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|error| format!("invalid {flag}: {error}"))
}

fn parse_i32(value: OsString, flag: &str) -> Result<i32, String> {
    value
        .to_string_lossy()
        .parse::<i32>()
        .map_err(|error| format!("invalid {flag}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(index: u32, causes_lucky: bool) -> Hit {
        Hit {
            rlog: "test.rlog".to_owned(),
            session_id: "test".to_owned(),
            sequence: u64::from(index),
            wire: WireKey {
                capture_sequence: 1,
                connection_id: 1,
                stream_id: 1,
            },
            run_ordinal: 1,
            source_entity_uuid: 10,
            target_entity_uuid: 20,
            ability_id: if causes_lucky {
                100
            } else {
                DEFAULT_LUCKY_ABILITY_ID
            },
            hit_event_id: if causes_lucky {
                1
            } else {
                DEFAULT_LUCKY_HIT_EVENT_ID
            },
            amount: 100,
            critical: Some(false),
            lucky: Some(!causes_lucky),
            causes_lucky: Some(causes_lucky),
            owner_stage: None,
            group_index: Some(7),
            component_index: Some(index),
            component_count: Some(2),
            lucky_probability: Some(800),
            lucky_damage_multiplier: Some(4_540),
            attack: Some(50_000),
            magic_attack: Some(40_000),
            local_source_attributes: Some(vec![(ATTACK_ATTRIBUTE_ID, 50_000)]),
            damage_row: None,
        }
    }

    #[test]
    fn exact_parent_requires_same_packet_group_and_causes_lucky_marker() {
        let lucky = hit(0, false);
        let parent = hit(1, true);
        let hits = vec![lucky.clone(), parent];
        let candidates = same_group_parent_candidates(&lucky, &hits);
        assert_eq!(candidates.len(), 1);
        assert!(components_are_adjacent(
            lucky.component_index,
            candidates[0].component_index
        ));
        assert!(component_immediately_follows(
            lucky.component_index,
            candidates[0].component_index
        ));
    }

    #[test]
    fn fixed_point_floor_is_integer_exact() {
        assert_eq!(mul_div_floor(41_800, 4_540, 10_000), Some(18_977));
    }

    #[test]
    fn source_attribute_candidates_preserve_exact_ids_and_group_variation() {
        let key = RelationKey {
            parent_ability_id: 100,
            parent_hit_event_id: 1,
            parent_owner_stage: Some(2),
            parent_critical: Some(false),
            lucky_critical: Some(false),
        };
        let mut accumulator = Accumulator::default();
        observe_source_attribute_candidates(
            &mut accumulator,
            &key,
            30_000,
            5_000,
            &[(ATTACK_ATTRIBUTE_ID, 50_000), (12_999, 2)],
        );
        observe_source_attribute_candidates(
            &mut accumulator,
            &key,
            35_000,
            5_000,
            &[(ATTACK_ATTRIBUTE_ID, 60_000)],
        );

        let results = source_attribute_candidate_results(
            &accumulator.source_attribute_candidates,
            accumulator.source_attribute_candidate_events,
        );
        let attack = results
            .iter()
            .find(|candidate| candidate.attribute_id == ATTACK_ATTRIBUTE_ID)
            .unwrap();
        assert_eq!(attack.events_present, 2);
        assert_eq!(attack.missing_candidate_events, 0);
        assert_eq!(attack.distinct_values, 2);
        assert_eq!(attack.relation_groups_present, 1);
        assert_eq!(attack.relation_groups_with_within_group_variation, 1);
        assert_eq!(attack.floor_attribute_times_lucky_multiplier.events, 2);
        assert_eq!(
            attack.floor_attribute_times_lucky_multiplier.exact_events,
            0
        );

        let sparse = results
            .iter()
            .find(|candidate| candidate.attribute_id == 12_999)
            .unwrap();
        assert_eq!(sparse.events_present, 1);
        assert_eq!(sparse.missing_candidate_events, 1);
    }
}
