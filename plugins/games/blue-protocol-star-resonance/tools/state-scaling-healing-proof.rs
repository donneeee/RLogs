#![allow(clippy::enum_variant_names, clippy::field_reassign_with_default)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorEvent, CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, EvidenceSource,
    RunState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 5;
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const MAX_HP_ATTRIBUTE_ID: i32 = 11_320;
const MAX_HP_TOTAL_ATTRIBUTE_ID: i32 = 11_321;
const MAX_HP_ADD_ATTRIBUTE_ID: i32 = 11_322;
const MAX_HP_EXTRA_ADD_ATTRIBUTE_ID: i32 = 11_323;
const MAX_HP_PERCENT_ATTRIBUTE_ID: i32 = 11_324;
const MAX_HP_EXTRA_PERCENT_ATTRIBUTE_ID: i32 = 11_325;
const DEFAULT_EXAMPLE_LIMIT: usize = 12;
const CANDIDATE_INTERVAL_LIMIT: i64 = 64;

#[derive(Debug)]
struct Arguments {
    game_build: String,
    rlogs: Vec<PathBuf>,
    abilities: BTreeSet<i64>,
    all_abilities: bool,
    output: PathBuf,
    example_limit: usize,
}

#[derive(Debug, Default, Clone)]
struct ActorSnapshot {
    name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WireMessageKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct HealingFormulaFamily {
    ability_id: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    passive_uuid: Option<u32>,
    critical: Option<bool>,
    periodic: Option<bool>,
    missed: Option<bool>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    normal_hit: Option<bool>,
    lucky: Option<bool>,
    rainbow: Option<bool>,
}

impl HealingFormulaFamily {
    fn from_event(healing: &rlogs_events::HealingEvent, ability_id: i64) -> Self {
        Self {
            ability_id,
            raw_attacker_uuid: healing.packet.attacker_uuid,
            raw_top_summoner_uuid: healing.packet.top_summoner_uuid,
            raw_owner_id: healing.packet.owner_id,
            hit_event_id: healing.hit_event_id,
            damage_source: healing.damage_source,
            damage_type: healing.damage_type,
            property: healing.packet.property,
            damage_mode: healing.packet.damage_mode,
            owner_level: healing.packet.owner_level,
            owner_stage: healing.packet.owner_stage,
            passive_uuid: healing.packet.passive_uuid,
            critical: healing.critical,
            periodic: healing.periodic,
            missed: healing.packet.missed,
            reported_critical: healing.packet.reported_critical,
            type_flags: healing.packet.type_flags,
            normal_hit: healing.packet.normal_hit,
            lucky: healing.packet.lucky_value.map(|value| value != 0),
            rainbow: healing.packet.rainbow,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
struct HpState {
    current_hp: Option<i64>,
    max_hp_final: Option<i64>,
    max_hp_total: Option<i64>,
    max_hp_add: Option<i64>,
    max_hp_extra_add: Option<i64>,
    max_hp_percent: Option<i64>,
    max_hp_extra_percent: Option<i64>,
}

impl HpState {
    fn missing_hp(self) -> Option<i64> {
        Some(self.max_hp_final?.saturating_sub(self.current_hp?))
    }
}

#[derive(Debug, Default)]
struct CandidateAccumulator {
    events_with_denominator: u64,
    events_with_positive_denominator: u64,
    events_with_bounded_candidate_interval: u64,
    events_with_unbounded_candidate_interval: u64,
    exact_candidate_counts: BTreeMap<i64, CandidateSupport>,
}

#[derive(Debug, Default)]
struct CandidateSupport {
    events: u64,
    numerators: BTreeSet<i64>,
    denominators: BTreeSet<i64>,
}

#[derive(Debug, Default)]
struct FormulaFamilyAccumulator {
    events: u64,
    amount_sum: i128,
    amount_min: Option<i64>,
    amount_max: Option<i64>,
    critical_events: u64,
    source_entities: BTreeSet<i64>,
    target_entities: BTreeSet<i64>,
    self_target_events: u64,
    events_with_wire_start_target_hp: u64,
    events_with_event_order_target_hp: u64,
    homogeneous_wire_cohorts: u64,
    mixed_formula_family_wire_cohorts: u64,
    homogeneous_wire_cohorts_with_current_hp_transition: u64,
    observed_effective_gain_sum: i128,
    reported_amount_equals_observed_gain: u64,
    reported_amount_exceeds_observed_gain: u64,
    reported_amount_below_observed_gain: u64,
    reported_amount_candidates: BTreeMap<FormulaBasis, CandidateAccumulator>,
    effective_gain_candidates: BTreeMap<FormulaBasis, CandidateAccumulator>,
    examples: Vec<HealingExample>,
    cohort_examples: Vec<HealingCohortExample>,
}

#[derive(Debug, Default)]
struct PendingHealingCohort {
    target_entity_uuid: i64,
    target_state_at_wire_message_start: HpState,
    target_state_was_snapshotted_at_wire_start: bool,
    amount_by_formula_family: BTreeMap<HealingFormulaFamily, i128>,
    events_by_formula_family: BTreeMap<HealingFormulaFamily, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum FormulaBasis {
    WireStartSourceCurrentHp,
    WireStartSourceMaxHp,
    WireStartSourceMissingHp,
    WireStartTargetCurrentHp,
    WireStartTargetMaxHp,
    WireStartTargetMissingHp,
    EventOrderSourceCurrentHp,
    EventOrderSourceMaxHp,
    EventOrderSourceMissingHp,
    EventOrderTargetCurrentHp,
    EventOrderTargetMaxHp,
    EventOrderTargetMissingHp,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    source_rlogs: Vec<String>,
    inputs: Vec<InputDescriptor>,
    selection: Selection,
    policy: Policy,
    summary: Summary,
    formula_families: Vec<HealingFormulaFamilyReport>,
}

#[derive(Debug, Serialize)]
struct Selection {
    all_abilities: bool,
    ability_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct InputDescriptor {
    path: String,
    bytes: u64,
    sha256: String,
    game_build: String,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_input_build_is_authoritative: bool,
    exact_input_hashes_are_embedded: bool,
    healing_events_are_discarded: bool,
    unresolved_hp_formulas_are_hidden: bool,
    event_kind_is_conflated_with_damage_or_casts: bool,
    reported_amount_semantics: &'static str,
    effective_gain_semantics: &'static str,
    percentage_unit: &'static str,
    formula_promotion: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    healing_events_scanned: u64,
    healing_events_selected: u64,
    healing_events_without_ability_id: u64,
    selected_ability_ids: usize,
    selected_formula_families: usize,
    selected_amount_sum: i128,
    selected_events_with_wire_start_target_hp: u64,
    selected_homogeneous_wire_cohorts: u64,
    selected_mixed_formula_family_wire_cohorts: u64,
    selected_homogeneous_wire_cohorts_with_current_hp_transition: u64,
}

#[derive(Debug, Serialize)]
struct HealingFormulaFamilyReport {
    family: HealingFormulaFamily,
    events: u64,
    amount_sum: i128,
    amount_min: Option<i64>,
    amount_max: Option<i64>,
    critical_events: u64,
    source_entities: Vec<i64>,
    target_entities: Vec<i64>,
    self_target_events: u64,
    events_with_wire_start_target_hp: u64,
    events_with_event_order_target_hp: u64,
    homogeneous_wire_cohorts: u64,
    mixed_formula_family_wire_cohorts: u64,
    homogeneous_wire_cohorts_with_current_hp_transition: u64,
    observed_effective_gain_sum: i128,
    reported_amount_equals_observed_gain: u64,
    reported_amount_exceeds_observed_gain: u64,
    reported_amount_below_observed_gain: u64,
    reported_amount_candidates: Vec<BasisCandidateReport>,
    effective_gain_candidates: Vec<BasisCandidateReport>,
    retained_in_canonical_timeline: bool,
    runtime_rdps_attribution_enabled: bool,
    examples: Vec<HealingExample>,
    cohort_examples: Vec<HealingCohortExample>,
}

#[derive(Debug, Serialize)]
struct BasisCandidateReport {
    basis: FormulaBasis,
    events_with_denominator: u64,
    events_with_positive_denominator: u64,
    events_with_bounded_candidate_interval: u64,
    events_with_unbounded_candidate_interval: u64,
    candidates: Vec<PercentageCandidate>,
}

#[derive(Debug, Serialize)]
struct PercentageCandidate {
    basis_points: i64,
    percent: f64,
    events: u64,
    coverage_basis_points: u64,
    distinct_numerators: usize,
    distinct_denominators: usize,
    numerator_examples: Vec<i64>,
    denominator_examples: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct HealingExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    ability_id: i64,
    hit_event_id: Option<i32>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    source_name: Option<String>,
    source_class_id: Option<i32>,
    source_specialization_id: Option<i32>,
    target_entity_uuid: i64,
    target_name: Option<String>,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    canonical_effective_amount: Option<i64>,
    canonical_overheal: Option<i64>,
    critical: Option<bool>,
    periodic: Option<bool>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    packet: rlogs_events::DamagePacketDetail,
    source_state_at_wire_message_start: HpState,
    target_state_at_wire_message_start: HpState,
    source_state_at_event_order: HpState,
    target_state_at_event_order: HpState,
}

#[derive(Debug, Clone, Serialize)]
struct HealingCohortExample {
    target_entity_uuid: i64,
    family: HealingFormulaFamily,
    healing_events: u64,
    reported_amount_sum: i128,
    target_state_at_wire_message_start: HpState,
    target_state_at_wire_message_end: HpState,
    observed_target_current_hp_gain: Option<i64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR state-scaling healing proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_arguments(env::args_os().skip(1))?;
    let inputs = args
        .rlogs
        .iter()
        .map(|path| input_descriptor(path, &args.game_build))
        .collect::<Result<Vec<_>, _>>()?;
    let mut summary = Summary::default();
    let mut formula_families = BTreeMap::<HealingFormulaFamily, FormulaFamilyAccumulator>::new();
    for path in &args.rlogs {
        scan_rlog(path, &args, &mut summary, &mut formula_families)?;
    }
    summary.selected_ability_ids = formula_families
        .keys()
        .map(|family| family.ability_id)
        .collect::<BTreeSet<_>>()
        .len();
    summary.selected_formula_families = formula_families.len();
    let report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-state-scaling-healing-proof",
        game_build: args.game_build.clone(),
        source_rlogs: args.rlogs.iter().map(|path| display_path(path)).collect(),
        inputs,
        selection: Selection {
            all_abilities: args.all_abilities,
            ability_ids: args.abilities.iter().copied().collect(),
        },
        policy: Policy {
            exact_input_build_is_authoritative: true,
            exact_input_hashes_are_embedded: true,
            healing_events_are_discarded: false,
            unresolved_hp_formulas_are_hidden: false,
            event_kind_is_conflated_with_damage_or_casts: false,
            reported_amount_semantics: "the packet healing amount is retained exactly even when no HP snapshot or formula is available",
            effective_gain_semantics: "all healing notifications for one target in one wire message are compared as a cohort with that message's CurrentHP transition; only homogeneous exact packet-formula families contribute a family-specific effective-gain proof, while mixed families remain retained and explicitly unresolved",
            percentage_unit: "10000 basis points equals 100%; candidate P is tested with floor(HP * P / 10000), not by treating raw packet integers as percentages",
            formula_promotion: "a candidate is evidence only; exact runtime attribution additionally requires packet-proved source, recipient, calculation-time state, operation order, stacking, and marginal replay conservation",
        },
        summary,
        formula_families: formula_families
            .into_iter()
            .map(|(family, accumulator)| formula_family_report(family, accumulator))
            .collect(),
    };
    let mut writer = BufWriter::new(File::create(args.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn input_descriptor(
    path: &Path,
    expected_build: &str,
) -> Result<InputDescriptor, Box<dyn std::error::Error>> {
    let reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let game_build = reader.header().region.client_build.clone();
    if game_build != expected_build {
        return Err(format!(
            "{} contains client build {game_build} but --build requires {expected_build}",
            display_path(path),
        )
        .into());
    }
    Ok(InputDescriptor {
        path: display_path(path),
        bytes: fs::metadata(path)?.len(),
        sha256: sha256_file(path)?,
        game_build,
    })
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn scan_rlog(
    path: &Path,
    args: &Arguments,
    summary: &mut Summary,
    formula_families: &mut BTreeMap<HealingFormulaFamily, FormulaFamilyAccumulator>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let client_build = reader.header().region.client_build.clone();
    if client_build != args.game_build {
        return Err(format!(
            "{} contains client build {} but --build requires {}",
            display_path(path),
            client_build,
            args.game_build
        )
        .into());
    }
    let mut run_ordinal = 0_u32;
    let mut actors = HashMap::<(u32, i64), ActorSnapshot>::new();
    let mut attributes = HashMap::<(u32, i64), BTreeMap<i32, i64>>::new();
    let mut active_wire_message = None;
    let mut attributes_at_wire_message_start = HashMap::<(u32, i64), BTreeMap<i32, i64>>::new();
    let mut pending_cohorts = BTreeMap::<i64, PendingHealingCohort>::new();

    while let Some(envelope) = reader.next_event()? {
        let wire_message = wire_message_key(&envelope.provenance.source);
        if wire_message != active_wire_message {
            flush_healing_cohorts(
                &mut pending_cohorts,
                &attributes,
                run_ordinal,
                args,
                summary,
                formula_families,
            );
            active_wire_message = wire_message;
            attributes_at_wire_message_start.clear();
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::Actor(actor) => observe_actor(&mut actors, run_ordinal, actor),
            TimelineEventKind::EntityAttributes(event) => {
                let key = (run_ordinal, event.actor.entity_uuid.0);
                if active_wire_message.is_some() {
                    attributes_at_wire_message_start
                        .entry(key)
                        .or_insert_with(|| attributes.get(&key).cloned().unwrap_or_default());
                    if let Some(cohort) = pending_cohorts.get_mut(&event.actor.entity_uuid.0) {
                        cohort.target_state_at_wire_message_start =
                            hp_state(attributes_at_wire_message_start.get(&key));
                        cohort.target_state_was_snapshotted_at_wire_start = true;
                    }
                }
                let values = attributes.entry(key).or_default();
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    values.clear();
                }
                for attribute in &event.attributes {
                    if is_hp_attribute(attribute.attribute_id)
                        && let Some(value) = integer_attribute(attribute)
                    {
                        values.insert(attribute.attribute_id, value);
                    }
                }
            }
            TimelineEventKind::Healing(healing) => {
                summary.healing_events_scanned = summary.healing_events_scanned.saturating_add(1);
                let Some(ability_id) = healing.ability.map(|value| value.0) else {
                    summary.healing_events_without_ability_id =
                        summary.healing_events_without_ability_id.saturating_add(1);
                    continue;
                };
                let formula_family = HealingFormulaFamily::from_event(healing, ability_id);
                let source_uuid = healing.source.entity_uuid.0;
                let target_uuid = healing.target.entity_uuid.0;
                let source_key = (run_ordinal, source_uuid);
                let target_key = (run_ordinal, target_uuid);
                let source_event_state = hp_state(attributes.get(&source_key));
                let target_event_state = hp_state(attributes.get(&target_key));
                let source_wire_state = hp_state(
                    attributes_at_wire_message_start
                        .get(&source_key)
                        .or_else(|| attributes.get(&source_key)),
                );
                let target_wire_state = hp_state(
                    attributes_at_wire_message_start
                        .get(&target_key)
                        .or_else(|| attributes.get(&target_key)),
                );
                let cohort =
                    pending_cohorts
                        .entry(target_uuid)
                        .or_insert_with(|| PendingHealingCohort {
                            target_entity_uuid: target_uuid,
                            target_state_at_wire_message_start: target_wire_state,
                            target_state_was_snapshotted_at_wire_start:
                                attributes_at_wire_message_start.contains_key(&target_key),
                            ..PendingHealingCohort::default()
                        });
                let amount = cohort
                    .amount_by_formula_family
                    .entry(formula_family)
                    .or_default();
                *amount = amount.saturating_add(i128::from(healing.amount));
                let event_count = cohort
                    .events_by_formula_family
                    .entry(formula_family)
                    .or_default();
                *event_count = event_count.saturating_add(1);

                if !args.all_abilities && !args.abilities.contains(&ability_id) {
                    continue;
                }
                summary.healing_events_selected = summary.healing_events_selected.saturating_add(1);
                summary.selected_amount_sum = summary
                    .selected_amount_sum
                    .saturating_add(i128::from(healing.amount));
                let source_actor = actors.get(&source_key);
                let target_actor = actors.get(&target_key);
                let example = HealingExample {
                    rlog: file_label(path),
                    session_id: envelope.session_id.clone(),
                    run_ordinal,
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    wire_capture_sequence: active_wire_message.map(|value| value.capture_sequence),
                    ability_id,
                    hit_event_id: healing.hit_event_id,
                    source_entity_uuid: source_uuid,
                    direct_source_entity_uuid: healing
                        .direct_source
                        .map(|value| value.entity_uuid.0),
                    source_name: source_actor.and_then(|value| value.name.clone()),
                    source_class_id: source_actor.and_then(|value| value.class_id),
                    source_specialization_id: source_actor
                        .and_then(|value| value.specialization_id),
                    target_entity_uuid: target_uuid,
                    target_name: target_actor.and_then(|value| value.name.clone()),
                    amount: healing.amount,
                    actual_amount: healing.actual_amount,
                    hp_loss: healing.hp_loss,
                    shield_loss: healing.shield_loss,
                    canonical_effective_amount: healing.effective_amount,
                    canonical_overheal: healing.overheal,
                    critical: healing.critical,
                    periodic: healing.periodic,
                    damage_source: healing.damage_source,
                    damage_type: healing.damage_type,
                    packet: healing.packet.clone(),
                    source_state_at_wire_message_start: source_wire_state,
                    target_state_at_wire_message_start: target_wire_state,
                    source_state_at_event_order: source_event_state,
                    target_state_at_event_order: target_event_state,
                };
                observe_healing(
                    formula_families.entry(formula_family).or_default(),
                    example,
                    args.example_limit,
                );
                if target_wire_state.current_hp.is_some()
                    && target_wire_state.max_hp_final.is_some()
                {
                    summary.selected_events_with_wire_start_target_hp = summary
                        .selected_events_with_wire_start_target_hp
                        .saturating_add(1);
                }
            }
            _ => {}
        }
    }
    flush_healing_cohorts(
        &mut pending_cohorts,
        &attributes,
        run_ordinal,
        args,
        summary,
        formula_families,
    );
    Ok(())
}

fn flush_healing_cohorts(
    pending: &mut BTreeMap<i64, PendingHealingCohort>,
    attributes: &HashMap<(u32, i64), BTreeMap<i32, i64>>,
    run_ordinal: u32,
    args: &Arguments,
    summary: &mut Summary,
    formula_families: &mut BTreeMap<HealingFormulaFamily, FormulaFamilyAccumulator>,
) {
    for (_, cohort) in std::mem::take(pending) {
        let selected_families = cohort
            .amount_by_formula_family
            .keys()
            .copied()
            .filter(|family| args.all_abilities || args.abilities.contains(&family.ability_id))
            .collect::<Vec<_>>();
        if selected_families.is_empty() {
            continue;
        }
        if cohort.amount_by_formula_family.len() != 1 {
            summary.selected_mixed_formula_family_wire_cohorts = summary
                .selected_mixed_formula_family_wire_cohorts
                .saturating_add(1);
            for family in selected_families {
                if let Some(accumulator) = formula_families.get_mut(&family) {
                    accumulator.mixed_formula_family_wire_cohorts = accumulator
                        .mixed_formula_family_wire_cohorts
                        .saturating_add(1);
                }
            }
            continue;
        }
        let family = *cohort
            .amount_by_formula_family
            .keys()
            .next()
            .expect("non-empty homogeneous cohort");
        if !args.all_abilities && !args.abilities.contains(&family.ability_id) {
            continue;
        }
        let Some(accumulator) = formula_families.get_mut(&family) else {
            continue;
        };
        summary.selected_homogeneous_wire_cohorts =
            summary.selected_homogeneous_wire_cohorts.saturating_add(1);
        accumulator.homogeneous_wire_cohorts =
            accumulator.homogeneous_wire_cohorts.saturating_add(1);
        let reported_amount_sum = cohort.amount_by_formula_family[&family];
        let target_state_at_wire_message_end =
            hp_state(attributes.get(&(run_ordinal, cohort.target_entity_uuid)));
        let observed_gain = if cohort.target_state_was_snapshotted_at_wire_start {
            match (
                cohort.target_state_at_wire_message_start.current_hp,
                target_state_at_wire_message_end.current_hp,
            ) {
                (Some(before), Some(after)) => Some(after.saturating_sub(before)),
                _ => None,
            }
        } else {
            None
        };
        if let Some(gain) = observed_gain {
            summary.selected_homogeneous_wire_cohorts_with_current_hp_transition = summary
                .selected_homogeneous_wire_cohorts_with_current_hp_transition
                .saturating_add(1);
            accumulator.homogeneous_wire_cohorts_with_current_hp_transition = accumulator
                .homogeneous_wire_cohorts_with_current_hp_transition
                .saturating_add(1);
            accumulator.observed_effective_gain_sum = accumulator
                .observed_effective_gain_sum
                .saturating_add(i128::from(gain));
            match reported_amount_sum.cmp(&i128::from(gain)) {
                std::cmp::Ordering::Equal => {
                    accumulator.reported_amount_equals_observed_gain = accumulator
                        .reported_amount_equals_observed_gain
                        .saturating_add(1)
                }
                std::cmp::Ordering::Greater => {
                    accumulator.reported_amount_exceeds_observed_gain = accumulator
                        .reported_amount_exceeds_observed_gain
                        .saturating_add(1)
                }
                std::cmp::Ordering::Less => {
                    accumulator.reported_amount_below_observed_gain = accumulator
                        .reported_amount_below_observed_gain
                        .saturating_add(1)
                }
            }
            if gain >= 0 {
                observe_target_candidates(
                    &mut accumulator.effective_gain_candidates,
                    gain,
                    cohort.target_state_at_wire_message_start,
                    target_state_at_wire_message_end,
                );
            }
        }
        if accumulator.cohort_examples.len() < args.example_limit {
            accumulator.cohort_examples.push(HealingCohortExample {
                target_entity_uuid: cohort.target_entity_uuid,
                family,
                healing_events: cohort.events_by_formula_family[&family],
                reported_amount_sum,
                target_state_at_wire_message_start: cohort.target_state_at_wire_message_start,
                target_state_at_wire_message_end,
                observed_target_current_hp_gain: observed_gain,
            });
        }
    }
}

fn observe_target_candidates(
    candidates: &mut BTreeMap<FormulaBasis, CandidateAccumulator>,
    numerator: i64,
    wire_start: HpState,
    event_order: HpState,
) {
    let values = [
        (
            FormulaBasis::WireStartTargetCurrentHp,
            wire_start.current_hp,
        ),
        (FormulaBasis::WireStartTargetMaxHp, wire_start.max_hp_final),
        (
            FormulaBasis::WireStartTargetMissingHp,
            wire_start.missing_hp(),
        ),
        (
            FormulaBasis::EventOrderTargetCurrentHp,
            event_order.current_hp,
        ),
        (
            FormulaBasis::EventOrderTargetMaxHp,
            event_order.max_hp_final,
        ),
        (
            FormulaBasis::EventOrderTargetMissingHp,
            event_order.missing_hp(),
        ),
    ];
    for (basis, denominator) in values {
        observe_percentage_candidate(candidates.entry(basis).or_default(), numerator, denominator);
    }
}

fn observe_healing(
    accumulator: &mut FormulaFamilyAccumulator,
    example: HealingExample,
    example_limit: usize,
) {
    accumulator.events = accumulator.events.saturating_add(1);
    accumulator.amount_sum = accumulator
        .amount_sum
        .saturating_add(i128::from(example.amount));
    accumulator.amount_min = Some(
        accumulator
            .amount_min
            .map_or(example.amount, |value| value.min(example.amount)),
    );
    accumulator.amount_max = Some(
        accumulator
            .amount_max
            .map_or(example.amount, |value| value.max(example.amount)),
    );
    accumulator
        .source_entities
        .insert(example.source_entity_uuid);
    accumulator
        .target_entities
        .insert(example.target_entity_uuid);
    if example.source_entity_uuid == example.target_entity_uuid {
        accumulator.self_target_events = accumulator.self_target_events.saturating_add(1);
    }
    if example.critical == Some(true) {
        accumulator.critical_events = accumulator.critical_events.saturating_add(1);
    }
    if example
        .target_state_at_wire_message_start
        .current_hp
        .is_some()
        && example
            .target_state_at_wire_message_start
            .max_hp_final
            .is_some()
    {
        accumulator.events_with_wire_start_target_hp = accumulator
            .events_with_wire_start_target_hp
            .saturating_add(1);
    }
    if example.target_state_at_event_order.current_hp.is_some()
        && example.target_state_at_event_order.max_hp_final.is_some()
    {
        accumulator.events_with_event_order_target_hp = accumulator
            .events_with_event_order_target_hp
            .saturating_add(1);
    }
    observe_candidates(
        &mut accumulator.reported_amount_candidates,
        example.amount,
        &example,
    );
    if accumulator.examples.len() < example_limit {
        accumulator.examples.push(example);
    }
}

fn observe_candidates(
    candidates: &mut BTreeMap<FormulaBasis, CandidateAccumulator>,
    numerator: i64,
    example: &HealingExample,
) {
    let values = [
        (
            FormulaBasis::WireStartSourceCurrentHp,
            example.source_state_at_wire_message_start.current_hp,
        ),
        (
            FormulaBasis::WireStartSourceMaxHp,
            example.source_state_at_wire_message_start.max_hp_final,
        ),
        (
            FormulaBasis::WireStartSourceMissingHp,
            example.source_state_at_wire_message_start.missing_hp(),
        ),
        (
            FormulaBasis::WireStartTargetCurrentHp,
            example.target_state_at_wire_message_start.current_hp,
        ),
        (
            FormulaBasis::WireStartTargetMaxHp,
            example.target_state_at_wire_message_start.max_hp_final,
        ),
        (
            FormulaBasis::WireStartTargetMissingHp,
            example.target_state_at_wire_message_start.missing_hp(),
        ),
        (
            FormulaBasis::EventOrderSourceCurrentHp,
            example.source_state_at_event_order.current_hp,
        ),
        (
            FormulaBasis::EventOrderSourceMaxHp,
            example.source_state_at_event_order.max_hp_final,
        ),
        (
            FormulaBasis::EventOrderSourceMissingHp,
            example.source_state_at_event_order.missing_hp(),
        ),
        (
            FormulaBasis::EventOrderTargetCurrentHp,
            example.target_state_at_event_order.current_hp,
        ),
        (
            FormulaBasis::EventOrderTargetMaxHp,
            example.target_state_at_event_order.max_hp_final,
        ),
        (
            FormulaBasis::EventOrderTargetMissingHp,
            example.target_state_at_event_order.missing_hp(),
        ),
    ];
    for (basis, denominator) in values {
        observe_percentage_candidate(candidates.entry(basis).or_default(), numerator, denominator);
    }
}

fn observe_percentage_candidate(
    accumulator: &mut CandidateAccumulator,
    numerator: i64,
    denominator: Option<i64>,
) {
    let Some(denominator) = denominator else {
        return;
    };
    accumulator.events_with_denominator = accumulator.events_with_denominator.saturating_add(1);
    if numerator < 0 || denominator <= 0 {
        return;
    }
    accumulator.events_with_positive_denominator = accumulator
        .events_with_positive_denominator
        .saturating_add(1);
    let Some((minimum, maximum)) = percentage_candidate_interval(numerator, denominator) else {
        return;
    };
    if maximum.saturating_sub(minimum) > CANDIDATE_INTERVAL_LIMIT {
        accumulator.events_with_unbounded_candidate_interval = accumulator
            .events_with_unbounded_candidate_interval
            .saturating_add(1);
        return;
    }
    accumulator.events_with_bounded_candidate_interval = accumulator
        .events_with_bounded_candidate_interval
        .saturating_add(1);
    for basis_points in minimum..=maximum {
        let support = accumulator
            .exact_candidate_counts
            .entry(basis_points)
            .or_default();
        support.events = support.events.saturating_add(1);
        support.numerators.insert(numerator);
        support.denominators.insert(denominator);
    }
}

fn percentage_candidate_interval(numerator: i64, denominator: i64) -> Option<(i64, i64)> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    let numerator = i128::from(numerator);
    let denominator = i128::from(denominator);
    let minimum = ceil_div(numerator.checked_mul(10_000)?, denominator);
    let maximum = ceil_div((numerator + 1).checked_mul(10_000)?, denominator) - 1;
    Some((i64::try_from(minimum).ok()?, i64::try_from(maximum).ok()?))
}

fn ceil_div(numerator: i128, denominator: i128) -> i128 {
    numerator / denominator + i128::from(numerator % denominator != 0)
}

fn formula_family_report(
    family: HealingFormulaFamily,
    accumulator: FormulaFamilyAccumulator,
) -> HealingFormulaFamilyReport {
    HealingFormulaFamilyReport {
        family,
        events: accumulator.events,
        amount_sum: accumulator.amount_sum,
        amount_min: accumulator.amount_min,
        amount_max: accumulator.amount_max,
        critical_events: accumulator.critical_events,
        source_entities: accumulator.source_entities.into_iter().collect(),
        target_entities: accumulator.target_entities.into_iter().collect(),
        self_target_events: accumulator.self_target_events,
        events_with_wire_start_target_hp: accumulator.events_with_wire_start_target_hp,
        events_with_event_order_target_hp: accumulator.events_with_event_order_target_hp,
        homogeneous_wire_cohorts: accumulator.homogeneous_wire_cohorts,
        mixed_formula_family_wire_cohorts: accumulator.mixed_formula_family_wire_cohorts,
        homogeneous_wire_cohorts_with_current_hp_transition: accumulator
            .homogeneous_wire_cohorts_with_current_hp_transition,
        observed_effective_gain_sum: accumulator.observed_effective_gain_sum,
        reported_amount_equals_observed_gain: accumulator.reported_amount_equals_observed_gain,
        reported_amount_exceeds_observed_gain: accumulator.reported_amount_exceeds_observed_gain,
        reported_amount_below_observed_gain: accumulator.reported_amount_below_observed_gain,
        reported_amount_candidates: candidate_reports(accumulator.reported_amount_candidates),
        effective_gain_candidates: candidate_reports(accumulator.effective_gain_candidates),
        retained_in_canonical_timeline: true,
        runtime_rdps_attribution_enabled: false,
        examples: accumulator.examples,
        cohort_examples: accumulator.cohort_examples,
    }
}

fn candidate_reports(
    candidates: BTreeMap<FormulaBasis, CandidateAccumulator>,
) -> Vec<BasisCandidateReport> {
    candidates
        .into_iter()
        .map(|(basis, accumulator)| {
            let denominator_events = accumulator.events_with_positive_denominator;
            let mut values = accumulator
                .exact_candidate_counts
                .into_iter()
                .map(|(basis_points, support)| PercentageCandidate {
                    basis_points,
                    percent: basis_points as f64 / 100.0,
                    events: support.events,
                    coverage_basis_points: support
                        .events
                        .saturating_mul(10_000)
                        .checked_div(denominator_events)
                        .unwrap_or_default(),
                    distinct_numerators: support.numerators.len(),
                    distinct_denominators: support.denominators.len(),
                    numerator_examples: support.numerators.into_iter().take(8).collect(),
                    denominator_examples: support.denominators.into_iter().take(8).collect(),
                })
                .collect::<Vec<_>>();
            values.sort_by(|left, right| {
                right
                    .events
                    .cmp(&left.events)
                    .then_with(|| left.basis_points.cmp(&right.basis_points))
            });
            values.truncate(64);
            BasisCandidateReport {
                basis,
                events_with_denominator: accumulator.events_with_denominator,
                events_with_positive_denominator: denominator_events,
                events_with_bounded_candidate_interval: accumulator
                    .events_with_bounded_candidate_interval,
                events_with_unbounded_candidate_interval: accumulator
                    .events_with_unbounded_candidate_interval,
                candidates: values,
            }
        })
        .collect()
}

fn observe_actor(
    actors: &mut HashMap<(u32, i64), ActorSnapshot>,
    run_ordinal: u32,
    actor: &ActorEvent,
) {
    let snapshot = actors
        .entry((run_ordinal, actor.actor.entity_uuid.0))
        .or_default();
    if actor.display_name.is_some() {
        snapshot.name = actor.display_name.clone();
    }
    if actor.class_id.is_some() {
        snapshot.class_id = actor.class_id;
    }
    if actor.specialization_id.is_some() {
        snapshot.specialization_id = actor.specialization_id;
    }
}

fn hp_state(values: Option<&BTreeMap<i32, i64>>) -> HpState {
    let value = |id| values.and_then(|entries| entries.get(&id)).copied();
    HpState {
        current_hp: value(CURRENT_HP_ATTRIBUTE_ID),
        max_hp_final: value(MAX_HP_ATTRIBUTE_ID),
        max_hp_total: value(MAX_HP_TOTAL_ATTRIBUTE_ID),
        max_hp_add: value(MAX_HP_ADD_ATTRIBUTE_ID),
        max_hp_extra_add: value(MAX_HP_EXTRA_ADD_ATTRIBUTE_ID),
        max_hp_percent: value(MAX_HP_PERCENT_ATTRIBUTE_ID),
        max_hp_extra_percent: value(MAX_HP_EXTRA_PERCENT_ATTRIBUTE_ID),
    }
}

fn is_hp_attribute(attribute_id: i32) -> bool {
    matches!(
        attribute_id,
        CURRENT_HP_ATTRIBUTE_ID
            | MAX_HP_ATTRIBUTE_ID
            | MAX_HP_TOTAL_ATTRIBUTE_ID
            | MAX_HP_ADD_ATTRIBUTE_ID
            | MAX_HP_EXTRA_ADD_ATTRIBUTE_ID
            | MAX_HP_PERCENT_ATTRIBUTE_ID
            | MAX_HP_EXTRA_PERCENT_ATTRIBUTE_ID
    )
}

fn integer_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
    match &attribute.decoded {
        Some(EntityAttributeValue::Integer(value)) => Some(*value),
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
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn wire_message_key(source: &EvidenceSource) -> Option<WireMessageKey> {
    match source {
        EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } => Some(WireMessageKey {
            capture_sequence: *capture_sequence,
            connection_id: *connection_id,
            stream_id: *stream_id,
        }),
        EvidenceSource::Derived { .. } | EvidenceSource::Manual { .. } => None,
    }
}

fn parse_arguments(values: impl Iterator<Item = OsString>) -> Result<Arguments, String> {
    let mut values = values.collect::<Vec<_>>();
    let mut game_build = None;
    let mut rlogs = Vec::new();
    let mut abilities = BTreeSet::new();
    let mut all_abilities = false;
    let mut output = None;
    let mut example_limit = DEFAULT_EXAMPLE_LIMIT;
    while !values.is_empty() {
        let flag = values.remove(0).to_string_lossy().into_owned();
        match flag.as_str() {
            "--build" => {
                let value = take_value(&mut values, "--build")?
                    .to_string_lossy()
                    .into_owned();
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("--build requires a numeric client build".to_owned());
                }
                game_build = Some(value);
            }
            "--rlog" => rlogs.push(PathBuf::from(take_value(&mut values, "--rlog")?)),
            "--ability" => {
                abilities.insert(parse_i64(
                    take_value(&mut values, "--ability")?,
                    "--ability",
                )?);
            }
            "--all-abilities" => all_abilities = true,
            "--output" => output = Some(PathBuf::from(take_value(&mut values, "--output")?)),
            "--example-limit" => {
                example_limit = take_value(&mut values, "--example-limit")?
                    .to_string_lossy()
                    .parse()
                    .map_err(|_| "--example-limit requires a non-negative integer".to_owned())?;
            }
            _ => return Err(usage()),
        }
    }
    if rlogs.is_empty() || (!all_abilities && abilities.is_empty()) {
        return Err(usage());
    }
    Ok(Arguments {
        game_build: game_build.ok_or_else(usage)?,
        rlogs,
        abilities,
        all_abilities,
        output: output.ok_or_else(usage)?,
        example_limit,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    if values.is_empty() {
        return Err(format!("missing value after {flag}"));
    }
    Ok(values.remove(0))
}

fn parse_i64(value: OsString, flag: &str) -> Result<i64, String> {
    value
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("{flag} requires an integer"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-state-scaling-healing-proof --build <client-build> --rlog <current-decoder.rlog> [--rlog ...] [--all-abilities | --ability <packet-ability-id> ...] --output <proof.json> [--example-limit <count>]".to_owned()
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("capture.rlog")
        .to_owned()
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_ability_selection_is_retained_even_without_an_observed_family() {
        let arguments = parse_arguments(
            [
                "--build",
                "24687926",
                "--rlog",
                "capture.rlog",
                "--ability",
                "2206241",
                "--output",
                "proof.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("exact ability selection should parse");
        assert!(!arguments.all_abilities);
        assert_eq!(arguments.abilities, BTreeSet::from([2_206_241]));
    }

    fn formula_family(ability_id: i64) -> HealingFormulaFamily {
        HealingFormulaFamily {
            ability_id,
            raw_attacker_uuid: None,
            raw_top_summoner_uuid: None,
            raw_owner_id: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            property: None,
            damage_mode: None,
            owner_level: None,
            owner_stage: None,
            passive_uuid: None,
            critical: None,
            periodic: None,
            missed: None,
            reported_critical: None,
            type_flags: None,
            normal_hit: None,
            lucky: None,
            rainbow: None,
        }
    }

    fn all_ability_arguments() -> Arguments {
        Arguments {
            game_build: "24568685".to_owned(),
            rlogs: Vec::new(),
            abilities: BTreeSet::new(),
            all_abilities: true,
            output: PathBuf::from("unused.json"),
            example_limit: 4,
        }
    }

    #[test]
    fn percentage_interval_recovers_exact_floor_scaled_basis_points() {
        let denominator = 864_974;
        let numerator = denominator * 200 / 10_000;
        assert_eq!(
            percentage_candidate_interval(numerator, denominator),
            Some((200, 200))
        );
    }

    #[test]
    fn percentage_interval_retains_rounding_ambiguity_instead_of_guessing() {
        assert_eq!(percentage_candidate_interval(1, 3), Some((3_334, 6_666)));
    }

    #[test]
    fn missing_hp_is_signed_packet_state_and_not_clamped() {
        let state = HpState {
            current_hp: Some(1_100),
            max_hp_final: Some(1_000),
            ..HpState::default()
        };
        assert_eq!(state.missing_hp(), Some(-100));
    }

    #[test]
    fn unresolved_heal_remains_retained_without_rdps_attribution() {
        let mut accumulator = FormulaFamilyAccumulator::default();
        observe_healing(
            &mut accumulator,
            HealingExample {
                rlog: "fixture.rlog".to_owned(),
                session_id: "fixture".to_owned(),
                run_ordinal: 1,
                sequence: 1,
                observed_micros: 1,
                wire_capture_sequence: None,
                ability_id: 123,
                hit_event_id: None,
                source_entity_uuid: 1,
                direct_source_entity_uuid: None,
                source_name: None,
                source_class_id: None,
                source_specialization_id: None,
                target_entity_uuid: 2,
                target_name: None,
                amount: 500,
                actual_amount: None,
                hp_loss: None,
                shield_loss: None,
                canonical_effective_amount: None,
                canonical_overheal: None,
                critical: None,
                periodic: None,
                damage_source: None,
                damage_type: None,
                packet: rlogs_events::DamagePacketDetail::default(),
                source_state_at_wire_message_start: HpState::default(),
                target_state_at_wire_message_start: HpState::default(),
                source_state_at_event_order: HpState::default(),
                target_state_at_event_order: HpState::default(),
            },
            1,
        );
        let report = formula_family_report(formula_family(123), accumulator);
        assert_eq!(report.events, 1);
        assert_eq!(report.amount_sum, 500);
        assert!(report.retained_in_canonical_timeline);
        assert!(!report.runtime_rdps_attribution_enabled);
    }

    #[test]
    fn homogeneous_wire_cohort_compares_the_summed_heal_to_one_hp_transition() {
        let family = formula_family(3059210);
        let mut pending = BTreeMap::from([(
            22,
            PendingHealingCohort {
                target_entity_uuid: 22,
                target_state_at_wire_message_start: HpState {
                    current_hp: Some(1_000),
                    max_hp_final: Some(5_000),
                    ..HpState::default()
                },
                target_state_was_snapshotted_at_wire_start: true,
                amount_by_formula_family: BTreeMap::from([(family, 100)]),
                events_by_formula_family: BTreeMap::from([(family, 2)]),
            },
        )]);
        let attributes = HashMap::from([(
            (1, 22),
            BTreeMap::from([
                (CURRENT_HP_ATTRIBUTE_ID, 1_100),
                (MAX_HP_ATTRIBUTE_ID, 5_000),
            ]),
        )]);
        let mut summary = Summary::default();
        let mut abilities = BTreeMap::from([(family, FormulaFamilyAccumulator::default())]);

        flush_healing_cohorts(
            &mut pending,
            &attributes,
            1,
            &all_ability_arguments(),
            &mut summary,
            &mut abilities,
        );

        let ability = &abilities[&family];
        assert!(pending.is_empty());
        assert_eq!(summary.selected_homogeneous_wire_cohorts, 1);
        assert_eq!(
            summary.selected_homogeneous_wire_cohorts_with_current_hp_transition,
            1
        );
        assert_eq!(ability.reported_amount_equals_observed_gain, 1);
        assert_eq!(ability.observed_effective_gain_sum, 100);
        assert_eq!(ability.cohort_examples[0].healing_events, 2);
        assert_eq!(ability.cohort_examples[0].reported_amount_sum, 100);
        assert_eq!(
            ability.cohort_examples[0].observed_target_current_hp_gain,
            Some(100)
        );
    }

    #[test]
    fn mixed_formula_family_wire_cohort_is_retained_but_not_used_as_formula_proof() {
        let first_family = formula_family(100);
        let second_family = formula_family(200);
        let mut pending = BTreeMap::from([(
            22,
            PendingHealingCohort {
                target_entity_uuid: 22,
                target_state_at_wire_message_start: HpState {
                    current_hp: Some(1_000),
                    max_hp_final: Some(5_000),
                    ..HpState::default()
                },
                target_state_was_snapshotted_at_wire_start: true,
                amount_by_formula_family: BTreeMap::from([(first_family, 75), (second_family, 25)]),
                events_by_formula_family: BTreeMap::from([(first_family, 1), (second_family, 1)]),
            },
        )]);
        let attributes = HashMap::from([(
            (1, 22),
            BTreeMap::from([
                (CURRENT_HP_ATTRIBUTE_ID, 1_100),
                (MAX_HP_ATTRIBUTE_ID, 5_000),
            ]),
        )]);
        let mut summary = Summary::default();
        let mut first = FormulaFamilyAccumulator::default();
        first.events = 1;
        let mut second = FormulaFamilyAccumulator::default();
        second.events = 1;
        let mut abilities = BTreeMap::from([(first_family, first), (second_family, second)]);

        flush_healing_cohorts(
            &mut pending,
            &attributes,
            1,
            &all_ability_arguments(),
            &mut summary,
            &mut abilities,
        );

        assert_eq!(summary.selected_mixed_formula_family_wire_cohorts, 1);
        assert_eq!(summary.selected_homogeneous_wire_cohorts, 0);
        for family in [first_family, second_family] {
            let ability = &abilities[&family];
            assert_eq!(ability.events, 1);
            assert_eq!(ability.mixed_formula_family_wire_cohorts, 1);
            assert_eq!(ability.homogeneous_wire_cohorts, 0);
            assert!(ability.effective_gain_candidates.is_empty());
            assert!(ability.cohort_examples.is_empty());
        }
    }

    #[test]
    fn one_ability_with_distinct_hits_never_becomes_one_formula_cohort() {
        let first_family = formula_family(3059210);
        let mut second_family = first_family;
        second_family.hit_event_id = Some(2);
        let mut pending = BTreeMap::from([(
            22,
            PendingHealingCohort {
                target_entity_uuid: 22,
                target_state_at_wire_message_start: HpState {
                    current_hp: Some(1_000),
                    max_hp_final: Some(5_000),
                    ..HpState::default()
                },
                target_state_was_snapshotted_at_wire_start: true,
                amount_by_formula_family: BTreeMap::from([(first_family, 75), (second_family, 25)]),
                events_by_formula_family: BTreeMap::from([(first_family, 1), (second_family, 1)]),
            },
        )]);
        let attributes = HashMap::from([(
            (1, 22),
            BTreeMap::from([
                (CURRENT_HP_ATTRIBUTE_ID, 1_100),
                (MAX_HP_ATTRIBUTE_ID, 5_000),
            ]),
        )]);
        let mut families = BTreeMap::from([
            (first_family, FormulaFamilyAccumulator::default()),
            (second_family, FormulaFamilyAccumulator::default()),
        ]);
        let mut summary = Summary::default();

        flush_healing_cohorts(
            &mut pending,
            &attributes,
            1,
            &all_ability_arguments(),
            &mut summary,
            &mut families,
        );

        assert_eq!(summary.selected_mixed_formula_family_wire_cohorts, 1);
        assert_eq!(summary.selected_homogeneous_wire_cohorts, 0);
        assert!(
            families
                .values()
                .all(|family| family.effective_gain_candidates.is_empty())
        );
    }
}
