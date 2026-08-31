use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{CanonicalEvent, TimelineEventKind};
use rlogs_game_bpsr::{BpsrDamageSourceKind, BpsrDamageType};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 5;
const DEFAULT_EXAMPLE_LIMIT: usize = 12;

#[derive(Debug)]
struct Arguments {
    game_build: String,
    surface: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    example_limit: usize,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    policy: AuditPolicy,
    surface_source: Value,
    sessions: Vec<SessionSummary>,
    coverage: Coverage,
    observed_keys: Vec<KeyReport>,
    missing_examples: Vec<EventExample>,
    ambiguous_examples: Vec<EventExample>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_formula_authority: bool,
    row_identity_authority: &'static str,
    correlation_rule: &'static str,
    field_semantics_inferred_from_offsets: bool,
    field_semantics_bound_by_typed_schema_and_current_formula_text: bool,
    unresolved_packet_evidence_is_hidden: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    rlog: String,
    session_id: String,
    combat_result_events: u64,
    damage_events: u64,
    healing_events: u64,
    events_with_ability_and_hit: u64,
    unique_row_matches: u64,
    ambiguous_row_matches: u64,
    missing_row_matches: u64,
}

#[derive(Debug, Serialize)]
struct Coverage {
    combat_result_events: u64,
    damage_events: u64,
    healing_events: u64,
    events_with_ability_and_hit: u64,
    events_without_ability_or_hit: u64,
    unique_row_matches: u64,
    ambiguous_row_matches: u64,
    missing_row_matches: u64,
    unique_match_basis_points: Option<u64>,
    observed_ability_hit_keys: usize,
    uniquely_mapped_ability_hit_keys: usize,
    ambiguously_mapped_ability_hit_keys: usize,
    missing_ability_hit_keys: usize,
    unique_rows_with_pve_damage_ratio: usize,
}

#[derive(Debug, Default)]
struct KeyAccumulator {
    events: u64,
    candidate_damage_ids: Vec<String>,
    source_entities: BTreeSet<i64>,
    direct_source_entities: BTreeSet<i64>,
    target_entities: BTreeSet<i64>,
    raw_attacker_uuids: BTreeMap<Option<i64>, u64>,
    raw_top_summoner_uuids: BTreeMap<Option<i64>, u64>,
    raw_owner_ids: BTreeMap<Option<i32>, u64>,
    owner_levels: BTreeMap<Option<i32>, u64>,
    owner_stages: BTreeMap<Option<i32>, u64>,
    damage_sources: BTreeMap<Option<i32>, u64>,
    damage_types: BTreeMap<Option<i32>, u64>,
    damage_properties: BTreeMap<Option<i32>, u64>,
    damage_modes: BTreeMap<Option<i32>, u64>,
    passive_uuids: BTreeMap<Option<u32>, u64>,
    skill_effect_uuids: BTreeMap<Option<i64>, u64>,
    normal_hits: BTreeMap<Option<bool>, u64>,
    critical_events: u64,
    lucky_events: u64,
    minimum_damage: Option<i64>,
    maximum_damage: Option<i64>,
}

#[derive(Debug, Serialize)]
struct KeyReport {
    ability_id: i64,
    hit_event_id: i32,
    events: u64,
    match_status: &'static str,
    candidate_damage_ids: Vec<String>,
    source_entities: usize,
    direct_source_entities: usize,
    target_entities: usize,
    raw_attacker_uuids: Vec<OptionalWideValueCount>,
    raw_top_summoner_uuids: Vec<OptionalWideValueCount>,
    raw_owner_ids: Vec<OptionalValueCount>,
    owner_levels: Vec<OptionalValueCount>,
    owner_stages: Vec<OptionalValueCount>,
    damage_sources: Vec<LabeledOptionalValueCount>,
    damage_types: Vec<LabeledOptionalValueCount>,
    damage_properties: Vec<OptionalValueCount>,
    damage_modes: Vec<OptionalValueCount>,
    passive_uuids: Vec<OptionalUnsignedValueCount>,
    skill_effect_uuids: Vec<OptionalWideValueCount>,
    normal_hits: Vec<OptionalBooleanCount>,
    critical_events: u64,
    lucky_events: u64,
    minimum_damage: Option<i64>,
    maximum_damage: Option<i64>,
    unique_row: Option<RowEvidence>,
}

#[derive(Debug, Serialize)]
struct OptionalValueCount {
    value: Option<i32>,
    events: u64,
}

#[derive(Debug, Serialize)]
struct LabeledOptionalValueCount {
    value: Option<i32>,
    label: Option<&'static str>,
    events: u64,
}

#[derive(Debug, Serialize)]
struct OptionalUnsignedValueCount {
    value: Option<u32>,
    events: u64,
}

#[derive(Debug, Serialize)]
struct OptionalWideValueCount {
    value: Option<i64>,
    events: u64,
}

#[derive(Debug, Serialize)]
struct OptionalBooleanCount {
    value: Option<bool>,
    events: u64,
}

#[derive(Debug, Serialize)]
struct RowEvidence {
    damage_id: String,
    level: Option<i32>,
    name: Option<String>,
    damage_type: Option<i32>,
    type_enum: Option<i32>,
    hit_event_suffix_candidate: Option<u64>,
    damage_script: Option<String>,
    pve_damage_ratio: Option<Vec<i32>>,
    pve_fixed_parameter: Option<Vec<i32>>,
    pve_loop_time: Option<i32>,
    pve_stunned_damage: Option<Vec<i32>>,
    pve_extinction_damage: Option<i32>,
    part_damage_ratio: Option<Vec<i32>>,
    damage_property: Option<i32>,
    part_damage_type: Option<i32>,
    tags: Option<Vec<i32>>,
    behit_light_is_open: Option<bool>,
    unresolved_nested_fields: [&'static str; 2],
}

#[derive(Debug, Serialize)]
struct EventExample {
    rlog: String,
    session_id: String,
    sequence: u64,
    ability_id: i64,
    hit_event_id: i32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    amount: i64,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    damage_property: Option<i32>,
    damage_mode: Option<i32>,
    passive_uuid: Option<u32>,
    skill_effect_uuid: Option<i64>,
    normal_hit: Option<bool>,
    candidate_damage_ids: Vec<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("DamageAttr packet correlation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let surface: Value = serde_json::from_reader(BufReader::new(File::open(&args.surface)?))?;
    let lookup = parse_lookup(&surface)?;
    let rows = surface
        .get("rows")
        .and_then(Value::as_object)
        .ok_or("surface is missing rows")?;
    let mut keys = BTreeMap::<(i64, i32), KeyAccumulator>::new();
    let mut sessions = Vec::new();
    let mut missing_examples = Vec::new();
    let mut ambiguous_examples = Vec::new();

    for rlog in &args.rlogs {
        sessions.push(read_session(
            rlog,
            &args.game_build,
            &lookup,
            &mut keys,
            &mut missing_examples,
            &mut ambiguous_examples,
            args.example_limit,
        )?);
    }

    let combat_result_events = sessions
        .iter()
        .map(|value| value.combat_result_events)
        .sum::<u64>();
    let damage_events = sessions
        .iter()
        .map(|value| value.damage_events)
        .sum::<u64>();
    let healing_events = sessions
        .iter()
        .map(|value| value.healing_events)
        .sum::<u64>();
    let events_with_ability_and_hit = sessions
        .iter()
        .map(|value| value.events_with_ability_and_hit)
        .sum::<u64>();
    let unique_row_matches = sessions
        .iter()
        .map(|value| value.unique_row_matches)
        .sum::<u64>();
    let ambiguous_row_matches = sessions
        .iter()
        .map(|value| value.ambiguous_row_matches)
        .sum::<u64>();
    let missing_row_matches = sessions
        .iter()
        .map(|value| value.missing_row_matches)
        .sum::<u64>();

    let mut unique_rows_with_pve_damage_ratio = 0_usize;
    let observed_keys = keys
        .into_iter()
        .map(|((ability_id, hit_event_id), value)| {
            let unique_row = if value.candidate_damage_ids.len() == 1 {
                let damage_id = &value.candidate_damage_ids[0];
                let row = rows.get(damage_id);
                if row.and_then(|value| row_int_array(value, 28)).is_some() {
                    unique_rows_with_pve_damage_ratio += 1;
                }
                row.map(|row| row_evidence(damage_id.clone(), row))
            } else {
                None
            };
            KeyReport {
                ability_id,
                hit_event_id,
                events: value.events,
                match_status: match value.candidate_damage_ids.len() {
                    0 => "missing",
                    1 => "unique",
                    _ => "ambiguous",
                },
                candidate_damage_ids: value.candidate_damage_ids,
                source_entities: value.source_entities.len(),
                direct_source_entities: value.direct_source_entities.len(),
                target_entities: value.target_entities.len(),
                raw_attacker_uuids: wide_counts(value.raw_attacker_uuids),
                raw_top_summoner_uuids: wide_counts(value.raw_top_summoner_uuids),
                raw_owner_ids: counts(value.raw_owner_ids),
                owner_levels: counts(value.owner_levels),
                owner_stages: counts(value.owner_stages),
                damage_sources: labeled_counts(value.damage_sources, damage_source_label),
                damage_types: labeled_counts(value.damage_types, damage_type_label),
                damage_properties: counts(value.damage_properties),
                damage_modes: counts(value.damage_modes),
                passive_uuids: unsigned_counts(value.passive_uuids),
                skill_effect_uuids: wide_counts(value.skill_effect_uuids),
                normal_hits: boolean_counts(value.normal_hits),
                critical_events: value.critical_events,
                lucky_events: value.lucky_events,
                minimum_damage: value.minimum_damage,
                maximum_damage: value.maximum_damage,
                unique_row,
            }
        })
        .collect::<Vec<_>>();
    let uniquely_mapped_ability_hit_keys = observed_keys
        .iter()
        .filter(|value| value.match_status == "unique")
        .count();
    let ambiguously_mapped_ability_hit_keys = observed_keys
        .iter()
        .filter(|value| value.match_status == "ambiguous")
        .count();
    let missing_ability_hit_keys = observed_keys
        .iter()
        .filter(|value| value.match_status == "missing")
        .count();

    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        game_build: args.game_build,
        generated_by: "rlogs-bpsr-damage-attr-packet-correlation",
        policy: AuditPolicy {
            runtime_formula_authority: false,
            row_identity_authority: "current-build DamageAttr table plus current-decoder canonical packet events",
            correlation_rule: "packet ability id + hit_event_id equals DamageAttr linked_id + damage_id modulo 100",
            field_semantics_inferred_from_offsets: false,
            field_semantics_bound_by_typed_schema_and_current_formula_text: true,
            unresolved_packet_evidence_is_hidden: false,
            promotion_requirement: "ambiguous/missing keys must be resolved and every formula field must be replay-proved before runtime attribution",
        },
        surface_source: surface.get("source").cloned().unwrap_or(Value::Null),
        sessions,
        coverage: Coverage {
            combat_result_events,
            damage_events,
            healing_events,
            events_with_ability_and_hit,
            events_without_ability_or_hit: combat_result_events
                .saturating_sub(events_with_ability_and_hit),
            unique_row_matches,
            ambiguous_row_matches,
            missing_row_matches,
            unique_match_basis_points: (events_with_ability_and_hit > 0)
                .then(|| unique_row_matches.saturating_mul(10_000) / events_with_ability_and_hit),
            observed_ability_hit_keys: observed_keys.len(),
            uniquely_mapped_ability_hit_keys,
            ambiguously_mapped_ability_hit_keys,
            missing_ability_hit_keys,
            unique_rows_with_pve_damage_ratio,
        },
        observed_keys,
        missing_examples,
        ambiguous_examples,
    };
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn row_evidence(damage_id: String, row: &Value) -> RowEvidence {
    RowEvidence {
        damage_id,
        level: row_i32(row, 8),
        name: row_string(row, 12),
        damage_type: row_i32(row, 16),
        type_enum: row_i32(row, 20),
        hit_event_suffix_candidate: row
            .get("hit_event_suffix_candidate")
            .and_then(Value::as_u64),
        damage_script: row_string(row, 24),
        pve_damage_ratio: row_int_array(row, 28),
        pve_fixed_parameter: row_int_array(row, 32),
        pve_loop_time: row_i32(row, 36),
        pve_stunned_damage: row_int_array(row, 40),
        pve_extinction_damage: row_i32(row, 44),
        part_damage_ratio: row_int_array(row, 48),
        damage_property: row_i32(row, 56),
        part_damage_type: row_i32(row, 60),
        tags: row_int_array(row, 68),
        behit_light_is_open: row
            .get("trailing_bytes_hex")
            .and_then(Value::as_str)
            .and_then(|value| value.get(0..2))
            .and_then(|value| u8::from_str_radix(value, 16).ok())
            .map(|value| value != 0),
        unresolved_nested_fields: ["AbnormalDamage", "DamageWeight"],
    }
}

fn row_i32(row: &Value, offset: u8) -> Option<i32> {
    row.pointer(&format!("/aligned_scalars_by_offset/{offset}/i32"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn row_string(row: &Value, offset: u8) -> Option<String> {
    let pointer = row
        .pointer(&format!("/aligned_scalars_by_offset/{offset}/u32"))
        .and_then(Value::as_u64)?;
    if pointer == 0 {
        return Some(String::new());
    }
    row.pointer(&format!(
        "/string_pool_6_candidates_by_offset/{offset}/value"
    ))
    .and_then(Value::as_str)
    .map(str::to_owned)
}

fn row_int_array(row: &Value, offset: u8) -> Option<Vec<i32>> {
    let pointer = row
        .pointer(&format!("/aligned_scalars_by_offset/{offset}/u32"))
        .and_then(Value::as_u64)?;
    if pointer == 0 {
        return Some(Vec::new());
    }
    row.pointer(&format!(
        "/int_array_pool_1_candidates_by_offset/{offset}/values"
    ))
    .and_then(Value::as_array)
    .map(|values| {
        values
            .iter()
            .filter_map(Value::as_i64)
            .filter_map(|value| i32::try_from(value).ok())
            .collect()
    })
}

fn parse_lookup(surface: &Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let object = surface
        .get("linked_hit_event_candidate_lookup")
        .and_then(Value::as_object)
        .ok_or_else(|| "surface is missing linked_hit_event_candidate_lookup".to_owned())?;
    object
        .iter()
        .map(|(key, values)| {
            let values = values
                .as_array()
                .ok_or_else(|| format!("lookup {key} is not an array"))?
                .iter()
                .map(value_as_id)
                .collect::<Result<Vec<_>, _>>()?;
            Ok((key.clone(), values))
        })
        .collect()
}

fn value_as_id(value: &Value) -> Result<String, String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err("damage id is not a string or number".to_owned()),
    }
}

fn read_session(
    path: &Path,
    expected_game_build: &str,
    lookup: &BTreeMap<String, Vec<String>>,
    keys: &mut BTreeMap<(i64, i32), KeyAccumulator>,
    missing_examples: &mut Vec<EventExample>,
    ambiguous_examples: &mut Vec<EventExample>,
    example_limit: usize,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = None::<String>;
    let mut combat_result_events = 0_u64;
    let mut damage_events = 0_u64;
    let mut healing_events = 0_u64;
    let mut events_with_ability_and_hit = 0_u64;
    let mut unique_row_matches = 0_u64;
    let mut ambiguous_row_matches = 0_u64;
    let mut missing_row_matches = 0_u64;
    while let Some(envelope) = reader.next_event()? {
        if envelope.region.client_build != expected_game_build {
            return Err(format!(
                "{} contains client build {} but --build requires {}",
                file_label(path),
                envelope.region.client_build,
                expected_game_build
            )
            .into());
        }
        session_id.get_or_insert_with(|| envelope.session_id.clone());
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        let (
            source,
            direct_source,
            target,
            ability,
            amount,
            hit_event_id,
            damage_source,
            damage_type,
            packet,
            critical,
            lucky,
        ) = match &timeline.kind {
            TimelineEventKind::Damage(damage) => {
                damage_events = damage_events.saturating_add(1);
                (
                    &damage.source,
                    damage.direct_source.as_ref(),
                    &damage.target,
                    damage.ability,
                    damage.amount,
                    damage.hit_event_id,
                    damage.damage_source,
                    damage.damage_type,
                    &damage.packet,
                    damage.flags.critical == Some(true),
                    damage.flags.lucky == Some(true),
                )
            }
            TimelineEventKind::Healing(healing) => {
                healing_events = healing_events.saturating_add(1);
                (
                    &healing.source,
                    healing.direct_source.as_ref(),
                    &healing.target,
                    healing.ability,
                    healing.amount,
                    healing.hit_event_id,
                    healing.damage_source,
                    healing.damage_type,
                    &healing.packet,
                    healing.critical == Some(true),
                    healing.packet.lucky_value.is_some_and(|value| value != 0),
                )
            }
            _ => continue,
        };
        combat_result_events = combat_result_events.saturating_add(1);
        let (Some(ability), Some(hit_event_id)) = (ability, hit_event_id) else {
            continue;
        };
        events_with_ability_and_hit = events_with_ability_and_hit.saturating_add(1);
        let lookup_key = format!("{}:{hit_event_id}", ability.0);
        let candidates = lookup.get(&lookup_key).cloned().unwrap_or_default();
        match candidates.len() {
            0 => missing_row_matches = missing_row_matches.saturating_add(1),
            1 => unique_row_matches = unique_row_matches.saturating_add(1),
            _ => ambiguous_row_matches = ambiguous_row_matches.saturating_add(1),
        }
        let accumulator = keys.entry((ability.0, hit_event_id)).or_default();
        accumulator.events = accumulator.events.saturating_add(1);
        accumulator.candidate_damage_ids = candidates.clone();
        accumulator.source_entities.insert(source.entity_uuid.0);
        if let Some(direct_source) = direct_source {
            accumulator
                .direct_source_entities
                .insert(direct_source.entity_uuid.0);
        }
        accumulator.target_entities.insert(target.entity_uuid.0);
        *accumulator
            .raw_attacker_uuids
            .entry(packet.attacker_uuid)
            .or_default() += 1;
        *accumulator
            .raw_top_summoner_uuids
            .entry(packet.top_summoner_uuid)
            .or_default() += 1;
        *accumulator
            .raw_owner_ids
            .entry(packet.owner_id)
            .or_default() += 1;
        *accumulator
            .owner_levels
            .entry(packet.owner_level)
            .or_default() += 1;
        *accumulator
            .owner_stages
            .entry(packet.owner_stage)
            .or_default() += 1;
        *accumulator.damage_sources.entry(damage_source).or_default() += 1;
        *accumulator.damage_types.entry(damage_type).or_default() += 1;
        *accumulator
            .damage_properties
            .entry(packet.property)
            .or_default() += 1;
        *accumulator
            .damage_modes
            .entry(packet.damage_mode)
            .or_default() += 1;
        *accumulator
            .passive_uuids
            .entry(packet.passive_uuid)
            .or_default() += 1;
        *accumulator
            .skill_effect_uuids
            .entry(packet.skill_effect_uuid)
            .or_default() += 1;
        *accumulator
            .normal_hits
            .entry(packet.normal_hit)
            .or_default() += 1;
        if critical {
            accumulator.critical_events = accumulator.critical_events.saturating_add(1);
        }
        if lucky {
            accumulator.lucky_events = accumulator.lucky_events.saturating_add(1);
        }
        accumulator.minimum_damage = Some(
            accumulator
                .minimum_damage
                .map_or(amount, |value| value.min(amount)),
        );
        accumulator.maximum_damage = Some(
            accumulator
                .maximum_damage
                .map_or(amount, |value| value.max(amount)),
        );
        let example = || EventExample {
            rlog: file_label(path),
            session_id: envelope.session_id.clone(),
            sequence: envelope.sequence,
            ability_id: ability.0,
            hit_event_id,
            source_entity_uuid: source.entity_uuid.0,
            direct_source_entity_uuid: direct_source.map(|source| source.entity_uuid.0),
            target_entity_uuid: target.entity_uuid.0,
            raw_attacker_uuid: packet.attacker_uuid,
            raw_top_summoner_uuid: packet.top_summoner_uuid,
            raw_owner_id: packet.owner_id,
            amount,
            owner_level: packet.owner_level,
            owner_stage: packet.owner_stage,
            damage_source,
            damage_type,
            damage_property: packet.property,
            damage_mode: packet.damage_mode,
            passive_uuid: packet.passive_uuid,
            skill_effect_uuid: packet.skill_effect_uuid,
            normal_hit: packet.normal_hit,
            candidate_damage_ids: candidates.clone(),
        };
        if candidates.is_empty() && missing_examples.len() < example_limit {
            missing_examples.push(example());
        } else if candidates.len() > 1 && ambiguous_examples.len() < example_limit {
            ambiguous_examples.push(example());
        }
    }
    Ok(SessionSummary {
        rlog: file_label(path),
        session_id: session_id.unwrap_or_else(|| "unobserved".to_owned()),
        combat_result_events,
        damage_events,
        healing_events,
        events_with_ability_and_hit,
        unique_row_matches,
        ambiguous_row_matches,
        missing_row_matches,
    })
}

fn counts(values: BTreeMap<Option<i32>, u64>) -> Vec<OptionalValueCount> {
    values
        .into_iter()
        .map(|(value, events)| OptionalValueCount { value, events })
        .collect()
}

fn labeled_counts(
    values: BTreeMap<Option<i32>, u64>,
    label: fn(i32) -> Option<&'static str>,
) -> Vec<LabeledOptionalValueCount> {
    values
        .into_iter()
        .map(|(value, events)| LabeledOptionalValueCount {
            label: value.and_then(label),
            value,
            events,
        })
        .collect()
}

fn damage_source_label(value: i32) -> Option<&'static str> {
    BpsrDamageSourceKind::from_protocol_id(value).map(BpsrDamageSourceKind::as_str)
}

fn damage_type_label(value: i32) -> Option<&'static str> {
    BpsrDamageType::from_protocol_id(value).map(BpsrDamageType::as_str)
}

fn unsigned_counts(values: BTreeMap<Option<u32>, u64>) -> Vec<OptionalUnsignedValueCount> {
    values
        .into_iter()
        .map(|(value, events)| OptionalUnsignedValueCount { value, events })
        .collect()
}

fn wide_counts(values: BTreeMap<Option<i64>, u64>) -> Vec<OptionalWideValueCount> {
    values
        .into_iter()
        .map(|(value, events)| OptionalWideValueCount { value, events })
        .collect()
}

fn boolean_counts(values: BTreeMap<Option<bool>, u64>) -> Vec<OptionalBooleanCount> {
    values
        .into_iter()
        .map(|(value, events)| OptionalBooleanCount { value, events })
        .collect()
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
    let example_limit = take_optional_value(&mut values, "--example-limit")
        .map(|value| parse_usize(value, "--example-limit"))
        .transpose()?
        .unwrap_or(DEFAULT_EXAMPLE_LIMIT);
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
        game_build,
        surface,
        rlogs,
        output,
        example_limit,
    })
}

fn parse_usize(value: OsString, flag: &str) -> Result<usize, String> {
    value
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
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
        return None;
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn usage() -> String {
    "usage: rlogs-bpsr-damage-attr-packet-correlation --build <numeric-client-build> --surface <DamageFormulaSurface.json> --rlog <current-decoder.rlog> [--rlog <current-decoder.rlog> ...] --output <audit.json> [--example-limit <count>]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{damage_source_label, damage_type_label, parse_lookup, value_as_id};
    use serde_json::json;

    #[test]
    fn accepts_numeric_and_string_damage_ids() {
        assert_eq!(value_as_id(&json!(123)).unwrap(), "123");
        assert_eq!(
            value_as_id(&json!("9007199254740993")).unwrap(),
            "9007199254740993"
        );
    }

    #[test]
    fn parses_candidate_lookup() {
        let surface = json!({
            "linked_hit_event_candidate_lookup": { "2352:3": [123520103] }
        });
        assert_eq!(parse_lookup(&surface).unwrap()["2352:3"], vec!["123520103"]);
    }

    #[test]
    fn labels_exact_sync_damage_protocol_discriminants_without_coercing_unknowns() {
        assert_eq!(damage_source_label(0), Some("skill"));
        assert_eq!(damage_source_label(1), Some("bullet"));
        assert_eq!(damage_source_label(2), Some("buff"));
        assert_eq!(damage_source_label(99), None);
        assert_eq!(damage_type_label(2), Some("heal"));
        assert_eq!(damage_type_label(5), Some("absorbed"));
        assert_eq!(damage_type_label(99), None);
    }
}
