use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, RunState, StatusState,
    TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 2;
const DEFAULT_MAX_GAP_MICROS: u64 = 2_000_000;
const DEFAULT_EXAMPLE_LIMIT: usize = 8;
const RECENT_PER_CONTEXT: usize = 8;
const FIXED_POINT_SCALE: i128 = 10_000;

#[derive(Debug)]
struct Arguments {
    catalog: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    max_gap_micros: u64,
    example_limit: usize,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    client_build: String,
    source: CatalogSource,
    attributes: Vec<CatalogAttribute>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CatalogSource {
    table: String,
    table_hash: u64,
    row_count: u64,
    row_size: u64,
    package: String,
}

#[derive(Debug, Deserialize)]
struct CatalogAttribute {
    id: i32,
    internal_name: Option<String>,
    design_description_zh_cn: Option<String>,
    table_unit_hint: String,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    client_build: String,
    source: CatalogSource,
    policy: AuditPolicy,
    max_pair_gap_micros: u64,
    sessions: Vec<SessionSummary>,
    coverage: Coverage,
    attributes: Vec<AttributeReport>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_formula_authority: bool,
    formula_stage_inference_from_names: bool,
    localization_is_formula_authority: bool,
    pair_scope: &'static str,
    attribute_control: &'static str,
    status_control: &'static str,
    formula_scope: &'static str,
    exact_event_authority: &'static str,
    unresolved_packet_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    rlog: String,
    session_id: String,
    run_ordinals_observed: u32,
    entity_attribute_values: u64,
    catalog_current_values: u64,
    damage_events: u64,
    damage_events_with_source_attributes: u64,
    controlled_pairs: u64,
    rejected_multiple_attribute_changes: u64,
    rejected_attribute_key_set_changes: u64,
}

#[derive(Debug, Serialize)]
struct Coverage {
    catalog_attributes: usize,
    attributes_observed_in_packet_snapshots: usize,
    attributes_with_controlled_pairs: usize,
    controlled_pairs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum AttributeLocus {
    Source,
    Target,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AttributeKey {
    locus: AttributeLocus,
    attribute_id: i32,
}

#[derive(Debug, Default)]
struct AttributeAccumulator {
    pairs: u64,
    transitions: BTreeSet<(i64, i64)>,
    formulas: BTreeMap<&'static str, FormulaAccumulator>,
    temporal: TemporalAccumulator,
    examples: Vec<PairExample>,
}

#[derive(Debug, Serialize)]
struct AttributeReport {
    locus: AttributeLocus,
    attribute_id: i32,
    internal_name: Option<String>,
    design_description_zh_cn: Option<String>,
    table_unit_hint: String,
    formula_stage: Option<String>,
    formula_proof: Option<String>,
    controlled_pairs: u64,
    distinct_transitions: usize,
    temporal_evidence: TemporalEvidenceReport,
    formulas: Vec<FormulaReport>,
    examples: Vec<PairExample>,
}

#[derive(Debug, Default)]
struct TemporalAccumulator {
    attribute_updates_between_hits: u64,
    updated_state_available_before_second_hit: u64,
    first_hit_hp_loss_observed: u64,
    first_hit_shield_loss_observed: u64,
    delta_equals_negative_first_hp_loss: u64,
    delta_equals_negative_first_shield_loss: u64,
}

#[derive(Debug, Serialize)]
struct TemporalEvidenceReport {
    attribute_updates_between_hits: u64,
    updated_state_available_before_second_hit: u64,
    first_hit_hp_loss_observed: u64,
    first_hit_shield_loss_observed: u64,
    delta_equals_negative_first_hp_loss: u64,
    delta_equals_negative_first_shield_loss: u64,
    interpretation_policy: &'static str,
}

#[derive(Debug, Default)]
struct FormulaAccumulator {
    evaluable_pairs: u64,
    exact_matches: u64,
    within_one_matches: u64,
    mismatches: u64,
    absolute_residual_sum: u128,
    maximum_absolute_residual: u128,
    residual_examples: BTreeSet<i64>,
}

#[derive(Debug, Serialize)]
struct FormulaReport {
    formula: &'static str,
    evaluable_pairs: u64,
    exact_matches: u64,
    within_one_matches: u64,
    mismatches: u64,
    mean_absolute_residual: Option<f64>,
    maximum_absolute_residual: u128,
    residual_examples: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct PairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    first_sequence: u64,
    second_sequence: u64,
    gap_micros: u64,
    first_amount: i64,
    second_amount: i64,
    first_hp_loss: Option<i64>,
    second_hp_loss: Option<i64>,
    first_shield_loss: Option<i64>,
    second_shield_loss: Option<i64>,
    first_attribute_value: i64,
    second_attribute_value: i64,
    attribute_delta: i64,
    first_attribute_observed_sequence: u64,
    second_attribute_observed_sequence: u64,
    first_attribute_observed_micros: u64,
    second_attribute_observed_micros: u64,
    attribute_updated_between_hits: bool,
    updated_state_available_before_second_hit: bool,
    delta_equals_negative_first_hp_loss: Option<bool>,
    delta_equals_negative_first_shield_loss: Option<bool>,
    formula_residuals: BTreeMap<&'static str, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DamageContext {
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
    source_status_fingerprint: u64,
    target_status_fingerprint: u64,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    hit_part_ids: Vec<Option<i32>>,
    damage_weight_bits: Option<(Option<u32>, Option<u32>)>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
}

#[derive(Debug, Clone)]
struct DamageSample {
    rlog: String,
    session_id: String,
    sequence: u64,
    observed_micros: u64,
    amount: i64,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    source_attributes: BTreeMap<i32, ObservedAttribute>,
    target_attributes: BTreeMap<i32, ObservedAttribute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObservedAttribute {
    value: i64,
    sequence: u64,
    observed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StatusKey {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusValue {
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
}

#[derive(Debug, Default)]
struct StatusTracker {
    active: BTreeMap<StatusKey, StatusValue>,
}

impl StatusTracker {
    fn observe(&mut self, key: StatusKey, value: StatusValue, state: StatusState) {
        match state {
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                self.active.insert(key, value);
            }
            StatusState::Consumed | StatusState::Removed => {
                self.active.remove(&key);
            }
        }
    }

    fn semantic_fingerprint(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for (key, value) in &self.active {
            for scalar in [
                key.effect_id,
                key.source_entity_uuid.unwrap_or(i64::MIN),
                i64::from(value.stacks.unwrap_or(u32::MAX)),
                i64::from(value.level.unwrap_or(i32::MIN)),
                i64::from(value.part_id.unwrap_or(i32::MIN + 1)),
                i64::from(value.count.unwrap_or(i32::MIN + 2)),
            ] {
                for byte in scalar.to_le_bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x100000001b3);
                }
            }
        }
        hash
    }
}

#[derive(Debug)]
struct ChangedAttribute {
    locus: AttributeLocus,
    attribute_id: i32,
    first: ObservedAttribute,
    second: ObservedAttribute,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("damage formula stage proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let catalog: Catalog = serde_json::from_reader(BufReader::new(File::open(&args.catalog)?))?;
    let catalog_ids = catalog
        .attributes
        .iter()
        .map(|attribute| attribute.id)
        .collect::<BTreeSet<_>>();
    let catalog_by_id = catalog
        .attributes
        .iter()
        .map(|attribute| (attribute.id, attribute))
        .collect::<BTreeMap<_, _>>();
    let mut accumulators = BTreeMap::<AttributeKey, AttributeAccumulator>::new();
    let mut observed_attribute_ids = BTreeSet::<i32>::new();
    let mut sessions = Vec::new();

    for path in &args.rlogs {
        sessions.push(read_session(
            path,
            &args,
            &catalog_ids,
            &mut observed_attribute_ids,
            &mut accumulators,
        )?);
    }

    let controlled_pairs = accumulators.values().map(|value| value.pairs).sum::<u64>();
    let attributes = accumulators
        .into_iter()
        .map(|(key, accumulator)| {
            let catalog_attribute = catalog_by_id.get(&key.attribute_id).copied();
            AttributeReport {
                locus: key.locus,
                attribute_id: key.attribute_id,
                internal_name: catalog_attribute.and_then(|value| value.internal_name.clone()),
                design_description_zh_cn: catalog_attribute
                    .and_then(|value| value.design_description_zh_cn.clone()),
                table_unit_hint: catalog_attribute
                    .map(|value| value.table_unit_hint.clone())
                    .unwrap_or_else(|| "outside_catalog".to_owned()),
                formula_stage: None,
                formula_proof: None,
                controlled_pairs: accumulator.pairs,
                distinct_transitions: accumulator.transitions.len(),
                temporal_evidence: temporal_evidence_report(accumulator.temporal),
                formulas: formula_reports(accumulator.formulas),
                examples: accumulator.examples,
            }
        })
        .collect::<Vec<_>>();
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-damage-formula-stage-proof",
        client_build: catalog.client_build,
        source: catalog.source,
        policy: AuditPolicy {
            runtime_formula_authority: false,
            formula_stage_inference_from_names: false,
            localization_is_formula_authority: false,
            pair_scope: "same session, run, source, direct source, target, ability, hit identity, damage source/type, hit flags, packet dimensions, and semantic source/target status fingerprints within the configured gap",
            attribute_control: "source and target catalog-current key sets must match and exactly one observed value may change; HP, shield, and resource attributes remain eligible evidence",
            status_control: "effect ID, provider, stacks, level, part, and count must match; transient effect instance IDs are excluded",
            formula_scope: "candidate ratio residuals prioritize controlled experiments and are never runtime authority by themselves",
            exact_event_authority: "input .rlog canonical EntityAttributes, Status, and Damage events",
            unresolved_packet_evidence_is_hidden: false,
        },
        max_pair_gap_micros: args.max_gap_micros,
        sessions,
        coverage: Coverage {
            catalog_attributes: catalog.attributes.len(),
            attributes_observed_in_packet_snapshots: observed_attribute_ids.len(),
            attributes_with_controlled_pairs: attributes.len(),
            controlled_pairs,
        },
        attributes,
    };
    let mut writer = BufWriter::new(File::create(args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_session(
    path: &Path,
    args: &Arguments,
    catalog_ids: &BTreeSet<i32>,
    observed_attribute_ids: &mut BTreeSet<i32>,
    accumulators: &mut BTreeMap<AttributeKey, AttributeAccumulator>,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = None::<String>;
    let mut current_run_ordinal = 0_u32;
    let mut maximum_run_ordinal = 0_u32;
    let mut entity_attribute_values = 0_u64;
    let mut catalog_current_values = 0_u64;
    let mut damage_events = 0_u64;
    let mut damage_events_with_source_attributes = 0_u64;
    let mut controlled_pairs = 0_u64;
    let mut rejected_multiple_attribute_changes = 0_u64;
    let mut rejected_attribute_key_set_changes = 0_u64;
    let mut attributes = HashMap::<(u32, i64), BTreeMap<i32, ObservedAttribute>>::new();
    let mut statuses = HashMap::<(u32, i64), StatusTracker>::new();
    let mut recent = BTreeMap::<DamageContext, VecDeque<DamageSample>>::new();

    while let Some(envelope) = reader.next_event()? {
        if let Some(expected) = &session_id {
            if expected != &envelope.session_id {
                return Err(format!(
                    "{} contains multiple sessions: {expected} and {}",
                    path.display(),
                    envelope.session_id
                )
                .into());
            }
        } else {
            session_id = Some(envelope.session_id.clone());
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => {
                    current_run_ordinal = current_run_ordinal.saturating_add(1);
                    maximum_run_ordinal = maximum_run_ordinal.max(current_run_ordinal);
                }
                RunState::Started if current_run_ordinal == 0 => {
                    current_run_ordinal = 1;
                    maximum_run_ordinal = 1;
                }
                _ => {}
            },
            TimelineEventKind::EntityAttributes(event) => {
                entity_attribute_values = entity_attribute_values
                    .saturating_add(u64::try_from(event.attributes.len()).unwrap_or(u64::MAX));
                let snapshot = attributes
                    .entry((current_run_ordinal, event.actor.entity_uuid.0))
                    .or_default();
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    snapshot.clear();
                }
                for attribute in &event.attributes {
                    if !catalog_ids.contains(&attribute.attribute_id) {
                        continue;
                    }
                    let Some(value) = decode_attribute(attribute) else {
                        continue;
                    };
                    snapshot.insert(
                        attribute.attribute_id,
                        ObservedAttribute {
                            value,
                            sequence: envelope.sequence,
                            observed_micros: envelope.time.observed_micros,
                        },
                    );
                    observed_attribute_ids.insert(attribute.attribute_id);
                    catalog_current_values = catalog_current_values.saturating_add(1);
                }
            }
            TimelineEventKind::Status(status) => {
                statuses
                    .entry((current_run_ordinal, status.target.entity_uuid.0))
                    .or_default()
                    .observe(
                        StatusKey {
                            effect_id: status.effect.0,
                            source_entity_uuid: status.source.map(|value| value.entity_uuid.0),
                        },
                        StatusValue {
                            stacks: status.stacks,
                            level: status.level,
                            part_id: status.part_id,
                            count: status.count,
                        },
                        status.state,
                    );
            }
            TimelineEventKind::Damage(damage) => {
                damage_events = damage_events.saturating_add(1);
                let source_uuid = damage.source.entity_uuid.0;
                let target_uuid = damage.target.entity_uuid.0;
                let source_attributes = attributes
                    .get(&(current_run_ordinal, source_uuid))
                    .cloned()
                    .unwrap_or_default();
                if source_attributes.is_empty() {
                    continue;
                }
                damage_events_with_source_attributes =
                    damage_events_with_source_attributes.saturating_add(1);
                let target_attributes = attributes
                    .get(&(current_run_ordinal, target_uuid))
                    .cloned()
                    .unwrap_or_default();
                let source_status_fingerprint = statuses
                    .get(&(current_run_ordinal, source_uuid))
                    .map(StatusTracker::semantic_fingerprint)
                    .unwrap_or(EMPTY_STATUS_FINGERPRINT);
                let target_status_fingerprint = statuses
                    .get(&(current_run_ordinal, target_uuid))
                    .map(StatusTracker::semantic_fingerprint)
                    .unwrap_or(EMPTY_STATUS_FINGERPRINT);
                let context = DamageContext {
                    run_ordinal: current_run_ordinal,
                    source_entity_uuid: source_uuid,
                    direct_source_entity_uuid: damage
                        .direct_source
                        .map(|value| value.entity_uuid.0),
                    raw_attacker_uuid: damage.packet.attacker_uuid,
                    raw_top_summoner_uuid: damage.packet.top_summoner_uuid,
                    raw_owner_id: damage.packet.owner_id,
                    target_entity_uuid: target_uuid,
                    ability_id: damage.ability.map(|value| value.0),
                    hit_event_id: damage.hit_event_id,
                    damage_source: damage.damage_source,
                    damage_type: damage.damage_type,
                    critical: damage.flags.critical == Some(true),
                    lucky: damage.flags.lucky == Some(true),
                    blocked: damage.flags.blocked == Some(true),
                    periodic: damage.flags.periodic == Some(true),
                    source_status_fingerprint,
                    target_status_fingerprint,
                    owner_level: damage.packet.owner_level,
                    owner_stage: damage.packet.owner_stage,
                    normal_hit: damage.packet.normal_hit,
                    property: damage.packet.property,
                    hit_part_ids: damage
                        .packet
                        .hit_parts
                        .iter()
                        .map(|part| part.part_id)
                        .collect(),
                    damage_weight_bits: damage
                        .packet
                        .damage_weight
                        .map(|weight| (weight.x.map(f32::to_bits), weight.y.map(f32::to_bits))),
                    passive_uuid: damage.packet.passive_uuid,
                    rainbow: damage.packet.rainbow,
                    damage_mode: damage.packet.damage_mode,
                };
                let sample = DamageSample {
                    rlog: file_label(path),
                    session_id: envelope.session_id.clone(),
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    amount: damage.amount,
                    hp_loss: damage.hp_loss,
                    shield_loss: damage.shield_loss,
                    source_attributes,
                    target_attributes,
                };
                let samples = recent.entry(context.clone()).or_default();
                for previous in samples.iter().rev() {
                    let gap = sample
                        .observed_micros
                        .saturating_sub(previous.observed_micros);
                    if gap > args.max_gap_micros {
                        break;
                    }
                    let changed = match one_changed_attribute(previous, &sample) {
                        Ok(Some(changed)) => changed,
                        Ok(None) => continue,
                        Err(ChangeRejection::KeySet) => {
                            rejected_attribute_key_set_changes =
                                rejected_attribute_key_set_changes.saturating_add(1);
                            continue;
                        }
                        Err(ChangeRejection::Multiple) => {
                            rejected_multiple_attribute_changes =
                                rejected_multiple_attribute_changes.saturating_add(1);
                            continue;
                        }
                    };
                    controlled_pairs = controlled_pairs.saturating_add(1);
                    observe_pair(args, &context, previous, &sample, changed, accumulators);
                    break;
                }
                samples.push_back(sample);
                while samples.len() > RECENT_PER_CONTEXT {
                    samples.pop_front();
                }
            }
            _ => {}
        }
    }

    Ok(SessionSummary {
        rlog: file_label(path),
        session_id: session_id.unwrap_or_else(|| "unobserved".to_owned()),
        run_ordinals_observed: maximum_run_ordinal,
        entity_attribute_values,
        catalog_current_values,
        damage_events,
        damage_events_with_source_attributes,
        controlled_pairs,
        rejected_multiple_attribute_changes,
        rejected_attribute_key_set_changes,
    })
}

#[derive(Debug)]
enum ChangeRejection {
    KeySet,
    Multiple,
}

fn one_changed_attribute(
    first: &DamageSample,
    second: &DamageSample,
) -> Result<Option<ChangedAttribute>, ChangeRejection> {
    if first
        .source_attributes
        .keys()
        .ne(second.source_attributes.keys())
        || first
            .target_attributes
            .keys()
            .ne(second.target_attributes.keys())
    {
        return Err(ChangeRejection::KeySet);
    }
    let mut changed = None::<ChangedAttribute>;
    for (locus, first_values, second_values) in [
        (
            AttributeLocus::Source,
            &first.source_attributes,
            &second.source_attributes,
        ),
        (
            AttributeLocus::Target,
            &first.target_attributes,
            &second.target_attributes,
        ),
    ] {
        for (attribute_id, first_value) in first_values {
            let second_value = second_values
                .get(attribute_id)
                .copied()
                .ok_or(ChangeRejection::KeySet)?;
            if first_value.value == second_value.value {
                continue;
            }
            if changed.is_some() {
                return Err(ChangeRejection::Multiple);
            }
            changed = Some(ChangedAttribute {
                locus,
                attribute_id: *attribute_id,
                first: *first_value,
                second: second_value,
            });
        }
    }
    Ok(changed)
}

fn observe_pair(
    args: &Arguments,
    context: &DamageContext,
    first: &DamageSample,
    second: &DamageSample,
    changed: ChangedAttribute,
    accumulators: &mut BTreeMap<AttributeKey, AttributeAccumulator>,
) {
    let accumulator = accumulators
        .entry(AttributeKey {
            locus: changed.locus,
            attribute_id: changed.attribute_id,
        })
        .or_default();
    accumulator.pairs = accumulator.pairs.saturating_add(1);
    accumulator
        .transitions
        .insert((changed.first.value, changed.second.value));
    let attribute_delta = changed.second.value.saturating_sub(changed.first.value);
    let attribute_updated_between_hits =
        changed.second.sequence > first.sequence && changed.second.sequence <= second.sequence;
    let updated_state_available_before_second_hit = changed.second.sequence <= second.sequence;
    if attribute_updated_between_hits {
        accumulator.temporal.attribute_updates_between_hits = accumulator
            .temporal
            .attribute_updates_between_hits
            .saturating_add(1);
    }
    if updated_state_available_before_second_hit {
        accumulator
            .temporal
            .updated_state_available_before_second_hit = accumulator
            .temporal
            .updated_state_available_before_second_hit
            .saturating_add(1);
    }
    if let Some(hp_loss) = first.hp_loss {
        accumulator.temporal.first_hit_hp_loss_observed = accumulator
            .temporal
            .first_hit_hp_loss_observed
            .saturating_add(1);
        if attribute_delta == hp_loss.saturating_neg() {
            accumulator.temporal.delta_equals_negative_first_hp_loss = accumulator
                .temporal
                .delta_equals_negative_first_hp_loss
                .saturating_add(1);
        }
    }
    if let Some(shield_loss) = first.shield_loss {
        accumulator.temporal.first_hit_shield_loss_observed = accumulator
            .temporal
            .first_hit_shield_loss_observed
            .saturating_add(1);
        if attribute_delta == shield_loss.saturating_neg() {
            accumulator.temporal.delta_equals_negative_first_shield_loss = accumulator
                .temporal
                .delta_equals_negative_first_shield_loss
                .saturating_add(1);
        }
    }
    let mut formula_residuals = BTreeMap::new();
    for formula in formula_names() {
        let Some(residual) = formula_residual(
            formula,
            changed.first.value,
            changed.second.value,
            first.amount,
            second.amount,
        ) else {
            continue;
        };
        formula_residuals.insert(formula, residual);
        observe_formula_residual(accumulator.formulas.entry(formula).or_default(), residual);
    }
    if accumulator.examples.len() < args.example_limit {
        accumulator.examples.push(PairExample {
            rlog: second.rlog.clone(),
            session_id: second.session_id.clone(),
            run_ordinal: context.run_ordinal,
            source_entity_uuid: context.source_entity_uuid,
            target_entity_uuid: context.target_entity_uuid,
            ability_id: context.ability_id,
            first_sequence: first.sequence,
            second_sequence: second.sequence,
            gap_micros: second.observed_micros.saturating_sub(first.observed_micros),
            first_amount: first.amount,
            second_amount: second.amount,
            first_hp_loss: first.hp_loss,
            second_hp_loss: second.hp_loss,
            first_shield_loss: first.shield_loss,
            second_shield_loss: second.shield_loss,
            first_attribute_value: changed.first.value,
            second_attribute_value: changed.second.value,
            attribute_delta,
            first_attribute_observed_sequence: changed.first.sequence,
            second_attribute_observed_sequence: changed.second.sequence,
            first_attribute_observed_micros: changed.first.observed_micros,
            second_attribute_observed_micros: changed.second.observed_micros,
            attribute_updated_between_hits,
            updated_state_available_before_second_hit,
            delta_equals_negative_first_hp_loss: first
                .hp_loss
                .map(|value| attribute_delta == value.saturating_neg()),
            delta_equals_negative_first_shield_loss: first
                .shield_loss
                .map(|value| attribute_delta == value.saturating_neg()),
            formula_residuals,
        });
    }
}

fn temporal_evidence_report(accumulator: TemporalAccumulator) -> TemporalEvidenceReport {
    TemporalEvidenceReport {
        attribute_updates_between_hits: accumulator.attribute_updates_between_hits,
        updated_state_available_before_second_hit: accumulator
            .updated_state_available_before_second_hit,
        first_hit_hp_loss_observed: accumulator.first_hit_hp_loss_observed,
        first_hit_shield_loss_observed: accumulator.first_hit_shield_loss_observed,
        delta_equals_negative_first_hp_loss: accumulator.delta_equals_negative_first_hp_loss,
        delta_equals_negative_first_shield_loss: accumulator
            .delta_equals_negative_first_shield_loss,
        interpretation_policy: "A state change may be a consequence of the first hit and still be a valid pre-hit input, threshold, or resource predicate for the second hit; temporal evidence is retained and is not itself formula proof.",
    }
}

fn formula_names() -> [&'static str; 7] {
    [
        "no_damage_effect",
        "raw_ratio",
        "inverse_raw_ratio",
        "fixed_point_bonus_10000",
        "inverse_fixed_point_bonus_10000",
        "fixed_point_remaining_10000",
        "inverse_fixed_point_remaining_10000",
    ]
}

fn formula_residual(
    formula: &'static str,
    first_value: i64,
    second_value: i64,
    first_amount: i64,
    second_amount: i64,
) -> Option<i64> {
    let first_value = i128::from(first_value);
    let second_value = i128::from(second_value);
    let (numerator, denominator) = match formula {
        "no_damage_effect" => (1, 1),
        "raw_ratio" => (second_value, first_value),
        "inverse_raw_ratio" => (first_value, second_value),
        "fixed_point_bonus_10000" => (
            FIXED_POINT_SCALE.checked_add(second_value)?,
            FIXED_POINT_SCALE.checked_add(first_value)?,
        ),
        "inverse_fixed_point_bonus_10000" => (
            FIXED_POINT_SCALE.checked_add(first_value)?,
            FIXED_POINT_SCALE.checked_add(second_value)?,
        ),
        "fixed_point_remaining_10000" => (
            FIXED_POINT_SCALE.checked_sub(second_value)?,
            FIXED_POINT_SCALE.checked_sub(first_value)?,
        ),
        "inverse_fixed_point_remaining_10000" => (
            FIXED_POINT_SCALE.checked_sub(first_value)?,
            FIXED_POINT_SCALE.checked_sub(second_value)?,
        ),
        _ => return None,
    };
    let predicted = predict_ratio_amount(first_amount, numerator, denominator)?;
    i64::try_from(i128::from(second_amount).checked_sub(predicted)?).ok()
}

fn predict_ratio_amount(amount: i64, numerator: i128, denominator: i128) -> Option<i128> {
    if denominator == 0 {
        return None;
    }
    i128::from(amount)
        .checked_mul(numerator)?
        .checked_div(denominator)
}

fn observe_formula_residual(accumulator: &mut FormulaAccumulator, residual: i64) {
    accumulator.evaluable_pairs = accumulator.evaluable_pairs.saturating_add(1);
    let absolute = u128::from(residual.unsigned_abs());
    accumulator.absolute_residual_sum = accumulator.absolute_residual_sum.saturating_add(absolute);
    accumulator.maximum_absolute_residual = accumulator.maximum_absolute_residual.max(absolute);
    match residual.unsigned_abs() {
        0 => accumulator.exact_matches = accumulator.exact_matches.saturating_add(1),
        1 => accumulator.within_one_matches = accumulator.within_one_matches.saturating_add(1),
        _ => accumulator.mismatches = accumulator.mismatches.saturating_add(1),
    }
    if accumulator.residual_examples.len() < 12 {
        accumulator.residual_examples.insert(residual);
    }
}

fn formula_reports(formulas: BTreeMap<&'static str, FormulaAccumulator>) -> Vec<FormulaReport> {
    formulas
        .into_iter()
        .map(|(formula, accumulator)| FormulaReport {
            formula,
            evaluable_pairs: accumulator.evaluable_pairs,
            exact_matches: accumulator.exact_matches,
            within_one_matches: accumulator.within_one_matches,
            mismatches: accumulator.mismatches,
            mean_absolute_residual: (accumulator.evaluable_pairs > 0).then(|| {
                accumulator.absolute_residual_sum as f64 / accumulator.evaluable_pairs as f64
            }),
            maximum_absolute_residual: accumulator.maximum_absolute_residual,
            residual_examples: accumulator.residual_examples.into_iter().collect(),
        })
        .collect()
}

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
    match attribute.decoded.clone().or_else(|| {
        decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value)
    }) {
        Some(EntityAttributeValue::Integer(value)) => Some(value),
        Some(EntityAttributeValue::Text(_)) | Some(EntityAttributeValue::Position { .. }) => None,
        None => decode_varint(&attribute.raw_value).map(|value| value as i64),
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

const EMPTY_STATUS_FINGERPRINT: u64 = 0xcbf29ce484222325_u64;

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let catalog = PathBuf::from(take_value(&mut values, "--catalog")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let max_gap_micros = take_optional_value(&mut values, "--max-gap-micros")
        .map(|value| parse_u64(value, "--max-gap-micros"))
        .transpose()?
        .unwrap_or(DEFAULT_MAX_GAP_MICROS);
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
        catalog,
        rlogs,
        output,
        max_gap_micros,
        example_limit,
    })
}

fn parse_u64(value: OsString, flag: &str) -> Result<u64, String> {
    value
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
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
    "usage: rlogs-bpsr-damage-formula-stage-proof --catalog <fight-attributes.json> --rlog <current-decoder.rlog> [--rlog <current-decoder.rlog> ...] --output <audit.json> [--max-gap-micros <micros>] [--example-limit <count>]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{DamageSample, ObservedAttribute, formula_residual, one_changed_attribute};
    use std::collections::BTreeMap;

    fn sample(amount: i64, attributes: &[(i32, i64)]) -> DamageSample {
        DamageSample {
            rlog: "fixture.rlog".to_owned(),
            session_id: "fixture".to_owned(),
            sequence: 1,
            observed_micros: 1,
            amount,
            hp_loss: None,
            shield_loss: None,
            source_attributes: attributes
                .iter()
                .map(|(attribute_id, value)| {
                    (
                        *attribute_id,
                        ObservedAttribute {
                            value: *value,
                            sequence: 1,
                            observed_micros: 1,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
            target_attributes: BTreeMap::new(),
        }
    }

    #[test]
    fn rejects_more_than_one_changed_attribute() {
        let first = sample(100, &[(11010, 1000), (12510, 12000)]);
        let second = sample(110, &[(11010, 1100), (12510, 12520)]);
        assert!(one_changed_attribute(&first, &second).is_err());
    }

    #[test]
    fn fixed_point_bonus_is_percentage_points_over_base() {
        let residual = formula_residual("fixed_point_bonus_10000", 12058, 12578, 67428, 69017);
        assert_eq!(residual, Some(0));
    }
}
