#![allow(
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, DamageEvent, EntityAttributeUpdateKind, EntityAttributeValue, EvidenceSource,
    RunState, StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const SCHEMA_VERSION: u16 = 26;
const DEFAULT_MAX_GAP_MICROS: u64 = 2_000_000;
const DEFAULT_EXAMPLE_LIMIT: usize = 12;
const RECENT_PER_KEY: usize = 24;
const PERCENT_SCALE: i128 = 10_000;

#[derive(Debug)]
struct Arguments {
    effect_id: i64,
    crit_attribute_id: i32,
    luck_attribute_id: i32,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    max_gap_micros: u64,
    example_limit: usize,
    effect_active_crit_delta: Option<i64>,
    effect_active_luck_delta: Option<i64>,
    selected_effect_bonus_raw: Option<i64>,
    ignore_packet_normal_hit_in_pair_key: bool,
    ignore_source_status_in_hit_flag_pair_key: bool,
    ignore_target_status_in_hit_flag_pair_key: bool,
    selected_effect_on_target: bool,
    selected_effect_provider_is_attacker: bool,
    compare_selected_effect_stacks: bool,
    ignored_status_effect_ids: BTreeSet<i64>,
    attacker_scoped_target_effect_ids: BTreeSet<i64>,
    context_attribute_ids: BTreeSet<i32>,
    diagnostic_attribute_ids: BTreeSet<i32>,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    selected_effect_id: i64,
    crit_damage_attribute_id: i32,
    lucky_damage_attribute_id: i32,
    max_pair_gap_micros: u64,
    expected_effect_active_crit_delta: Option<i64>,
    expected_effect_active_luck_delta: Option<i64>,
    selected_effect_bonus_raw: Option<i64>,
    ignored_packet_normal_hit_in_pair_key: bool,
    ignored_source_status_in_hit_flag_pair_key: bool,
    ignored_target_status_in_hit_flag_pair_key: bool,
    selected_effect_scope: &'static str,
    selected_effect_provider_is_attacker: bool,
    compare_selected_effect_stacks: bool,
    ignored_status_effect_ids: Vec<i64>,
    attacker_scoped_target_effect_ids: Vec<i64>,
    context_attribute_ids: Vec<i32>,
    diagnostic_attribute_ids: Vec<i32>,
    sessions: Vec<SessionSummary>,
    pair_groups: Vec<PairGroupReport>,
    candidate_formulas: Vec<CandidateFormulaReport>,
    hit_flag_pair_groups: Vec<HitFlagPairGroupReport>,
    hit_flag_candidate_formulas: Vec<CandidateFormulaReport>,
    hit_flag_diagnostic_tiers: Vec<HitFlagDiagnosticTierReport>,
    hit_flag_observations: HitFlagObservationReport,
    packet_normal_hit_outcomes: Vec<PacketNormalHitOutcomeReport>,
    packet_field_outcomes: Vec<PacketFieldOutcomeReport>,
    same_wire_damage_groups: Vec<SameWireDamageGroupReport>,
    selected_effect_counterfactuals: SelectedEffectCounterfactualReport,
    selected_effect_proof_gap: SelectedEffectProofGapReport,
}

#[derive(Debug, Clone, Serialize)]
struct EffectCountReport {
    effect_id: i64,
    count: u64,
}

#[derive(Debug, Serialize)]
struct SelectedEffectProofGapReport {
    candidates_before_status_control: u64,
    status_controlled_toggle_pairs: u64,
    controlled_toggle_pairs: u64,
    fully_attribute_stable_toggle_pairs: u64,
    attribute_adjusted_toggle_pairs: u64,
    confounded_toggle_pairs: u64,
    top_source_status_confounders: Vec<EffectCountReport>,
    top_target_status_confounders: Vec<EffectCountReport>,
    minimum_exact_selector: Vec<&'static str>,
    controlled_selector_satisfied: bool,
    fully_controlled_selector_satisfied: bool,
    exact_candidate_formula_observed: bool,
    formula_placement_status: &'static str,
    runtime_promotion_allowed: bool,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_use: &'static str,
    pair_scope: &'static str,
    status_control: &'static str,
    hit_flag_status_control: &'static str,
    attribute_scope: &'static str,
    amount_scope: &'static str,
    packet_normal_hit_control: &'static str,
    hit_flag_status_pair_control: &'static str,
    formula_scope: &'static str,
    same_wire_scope: &'static str,
    counterfactual_scope: &'static str,
    exact_accounting_scope: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Default)]
struct SelectedEffectCounterfactualAccumulator {
    selected_effect_transition_wire_damage_events_excluded: u64,
    external_provider_impact_events: u64,
    unique_external_provider_impact_events: u64,
    ambiguous_provider_impact_events: u64,
    critical_only_events: u64,
    lucky_only_events: u64,
    critical_lucky_events: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
    exact_attributed_damage_sum: i128,
    exact_fraction_events: u64,
    exact_fraction_buckets: BTreeMap<ExactFractionBucketKey, ExactFractionBucketAccumulator>,
    exact_examples: Vec<SelectedEffectCounterfactualExample>,
    unresolved_examples: Vec<SelectedEffectCounterfactualExample>,
}

#[derive(Debug, Serialize)]
struct SelectedEffectCounterfactualReport {
    selected_effect_transition_wire_damage_events_excluded: u64,
    external_provider_impact_events: u64,
    unique_external_provider_impact_events: u64,
    ambiguous_provider_impact_events: u64,
    critical_only_events: u64,
    lucky_only_events: u64,
    critical_lucky_events: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
    exact_attributed_damage_sum: i128,
    exact_fraction_events: u64,
    exact_fraction_buckets: Vec<ExactFractionBucketReport>,
    exact_examples: Vec<SelectedEffectCounterfactualExample>,
    unresolved_examples: Vec<SelectedEffectCounterfactualExample>,
}

#[derive(Debug, Clone, Serialize)]
struct SelectedEffectCounterfactualExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    provider_entity_uuids: Vec<i64>,
    ability_id: Option<i64>,
    critical: bool,
    lucky: bool,
    observed_amount: i64,
    current_critical_factor: i64,
    current_lucky_factor: i64,
    provider_removed_critical_factor: i64,
    provider_removed_lucky_factor: i64,
    candidate_counterfactuals: Vec<CounterfactualCandidate>,
    exact_stage_independent_amount: Option<i64>,
    exact_attributed_damage: Option<i64>,
    exact_accounting_fraction: Option<ExactFractionReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ExactFractionBucketKey {
    current_critical_factor: i64,
    current_lucky_factor: i64,
    provider_removed_critical_factor: i64,
    provider_removed_lucky_factor: i64,
}

#[derive(Debug, Default)]
struct ExactFractionBucketAccumulator {
    event_count: u64,
    observed_damage_sum: i128,
}

#[derive(Debug, Clone, Serialize)]
struct ExactFractionReport {
    numerator: String,
    denominator: String,
    floor: String,
    ceil: String,
    decimal_9_places: String,
}

#[derive(Debug, Serialize)]
struct ExactFractionBucketReport {
    current_critical_factor: i64,
    current_lucky_factor: i64,
    provider_removed_critical_factor: i64,
    provider_removed_lucky_factor: i64,
    event_count: u64,
    observed_damage_sum: String,
    provider_attribution: ExactFractionReport,
    recipient_retained: ExactFractionReport,
    conservation_identity_holds: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CounterfactualCandidate {
    formula: &'static str,
    latent_base_min: i64,
    latent_base_max: i64,
    counterfactual_min: i64,
    counterfactual_max: i64,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    rlog: String,
    session_id: String,
    run_ordinals_observed: u32,
    damage_events: u64,
    damage_events_with_relevant_multiplier_flag: u64,
    damage_events_with_both_attributes_known: u64,
    selected_status_events: u64,
    effect_transition_candidates_before_status_control: u64,
    strict_candidate_pairs: u64,
    fully_attribute_stable_candidate_pairs: u64,
    attribute_adjusted_candidate_pairs: u64,
    confounded_effect_transition_pairs: u64,
    source_status_confounders: Vec<EffectCountReport>,
    target_status_confounders: Vec<EffectCountReport>,
    confounded_effect_transition_examples: Vec<ConfoundedEffectTransitionExample>,
    confounded_expected_delta_pairs: u64,
    confounded_expected_delta_examples: Vec<ConfoundedEffectTransitionExample>,
    damage_events_eligible_for_hit_flag_comparison: u64,
    strict_hit_flag_pairs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PairGroupKey {
    critical: bool,
    lucky: bool,
    crit_raw_delta: i64,
    luck_raw_delta: i64,
    effect_transition: &'static str,
    first_effect_stacks: Option<u32>,
    second_effect_stacks: Option<u32>,
}

#[derive(Debug, Default)]
struct PairGroupAccumulator {
    count: u64,
    examples: Vec<PairExample>,
}

#[derive(Debug, Serialize)]
struct PairGroupReport {
    critical: bool,
    lucky: bool,
    crit_raw_delta: i64,
    luck_raw_delta: i64,
    effect_transition: &'static str,
    first_effect_stacks: Option<u32>,
    second_effect_stacks: Option<u32>,
    count: u64,
    examples: Vec<PairExample>,
}

#[derive(Debug, Clone, Serialize)]
struct PairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    missed: Option<bool>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    hit_part_ids: Vec<Option<i32>>,
    first_position_bits: Option<(Option<u32>, Option<u32>, Option<u32>)>,
    second_position_bits: Option<(Option<u32>, Option<u32>, Option<u32>)>,
    first_hit_part_context: Vec<(Option<i32>, Option<(Option<u32>, Option<u32>, Option<u32>)>)>,
    second_hit_part_context: Vec<(Option<i32>, Option<(Option<u32>, Option<u32>, Option<u32>)>)>,
    damage_weight_bits: Option<(Option<u32>, Option<u32>)>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_component: SkillEffectComponentIdentity,
    first_skill_effect_total_damage: Option<i64>,
    second_skill_effect_total_damage: Option<i64>,
    context_attribute_values: Vec<(i32, Option<i64>)>,
    diagnostic_attribute_transitions: Vec<AttributeTransitionReport>,
    critical: bool,
    lucky: bool,
    first_sequence: u64,
    second_sequence: u64,
    gap_micros: u64,
    first_effect_active: bool,
    second_effect_active: bool,
    first_effect_stacks: Option<u32>,
    second_effect_stacks: Option<u32>,
    first_amount: i64,
    second_amount: i64,
    first_actual_amount: Option<i64>,
    second_actual_amount: Option<i64>,
    first_hp_loss: Option<i64>,
    second_hp_loss: Option<i64>,
    first_shield_loss: Option<i64>,
    second_shield_loss: Option<i64>,
    first_normal_value: Option<i64>,
    second_normal_value: Option<i64>,
    first_lucky_value: Option<i64>,
    second_lucky_value: Option<i64>,
    first_reported_critical: Option<bool>,
    second_reported_critical: Option<bool>,
    first_type_flags: Option<i32>,
    second_type_flags: Option<i32>,
    first_normal_hit: Option<bool>,
    second_normal_hit: Option<bool>,
    first_property: Option<i32>,
    second_property: Option<i32>,
    first_passive_uuid: Option<u32>,
    second_passive_uuid: Option<u32>,
    first_rainbow: Option<bool>,
    second_rainbow: Option<bool>,
    first_damage_mode: Option<i32>,
    second_damage_mode: Option<i32>,
    first_crit_damage_raw: i64,
    second_crit_damage_raw: i64,
    first_lucky_damage_raw: i64,
    second_lucky_damage_raw: i64,
    crit_raw_delta: i64,
    luck_raw_delta: i64,
    source_status_fingerprint: String,
    target_status_fingerprint: String,
    source_statuses: Vec<SemanticStatusEntry>,
    target_statuses: Vec<SemanticStatusEntry>,
    formula_residuals: BTreeMap<&'static str, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AttributeTransitionReport {
    attribute_id: i32,
    first_value: Option<i64>,
    second_value: Option<i64>,
    delta: Option<i64>,
}

#[derive(Debug, Default)]
struct CandidateFormulaAccumulator {
    evaluable_pairs: u64,
    exact_matches: u64,
    within_one_matches: u64,
    mismatches: u64,
    absolute_residual_sum: u128,
    maximum_absolute_residual: u128,
    residual_examples: BTreeSet<i64>,
}

#[derive(Debug, Serialize)]
struct CandidateFormulaReport {
    formula: &'static str,
    evaluable_pairs: u64,
    exact_matches: u64,
    within_one_matches: u64,
    mismatches: u64,
    mean_absolute_residual: Option<f64>,
    maximum_absolute_residual: u128,
    residual_examples: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct SkillEffectComponentIdentity {
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DamageKey {
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
    missed: Option<bool>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    hit_part_ids: Vec<Option<i32>>,
    damage_weight_bits: Option<(Option<u32>, Option<u32>)>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_component: SkillEffectComponentIdentity,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
    context_attribute_values: Vec<(i32, Option<i64>)>,
}

#[derive(Debug, Clone)]
struct DamageSample {
    rlog: String,
    session_id: String,
    sequence: u64,
    observed_micros: u64,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_total_damage: Option<i64>,
    position_bits: Option<(Option<u32>, Option<u32>, Option<u32>)>,
    hit_part_context: Vec<(Option<i32>, Option<(Option<u32>, Option<u32>, Option<u32>)>)>,
    effect_active: bool,
    effect_stacks: Option<u32>,
    crit_damage_raw: i64,
    lucky_damage_raw: i64,
    source_status_fingerprint: u64,
    target_status_fingerprint: u64,
    source_status_snapshot: Vec<SemanticStatusEntry>,
    target_status_snapshot: Vec<SemanticStatusEntry>,
    diagnostic_attribute_values: Vec<(i32, Option<i64>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct SemanticStatusEntry {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
struct SemanticStatusCount {
    status: SemanticStatusEntry,
    count: u32,
}

#[derive(Debug, Clone, Serialize)]
struct ConfoundedEffectTransitionExample {
    first_sequence: u64,
    second_sequence: u64,
    gap_micros: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    critical: bool,
    lucky: bool,
    first_effect_active: bool,
    second_effect_active: bool,
    first_effect_stacks: Option<u32>,
    second_effect_stacks: Option<u32>,
    first_amount: i64,
    second_amount: i64,
    first_crit_damage_raw: i64,
    second_crit_damage_raw: i64,
    first_lucky_damage_raw: i64,
    second_lucky_damage_raw: i64,
    source_status_removed: Vec<SemanticStatusCount>,
    source_status_added: Vec<SemanticStatusCount>,
    target_status_removed: Vec<SemanticStatusCount>,
    target_status_added: Vec<SemanticStatusCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HitFlagContextKey {
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
    blocked: bool,
    periodic: bool,
    crit_damage_raw: i64,
    lucky_damage_raw: i64,
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
    skill_effect_component: SkillEffectComponentIdentity,
}

#[derive(Debug, Clone)]
struct HitFlagSample {
    rlog: String,
    session_id: String,
    sequence: u64,
    observed_micros: u64,
    amount: i64,
    actual_amount: Option<i64>,
    critical: bool,
    lucky: bool,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    normal_hit: Option<bool>,
}

#[derive(Debug, Default)]
struct HitFlagDiagnosticContextAccumulator {
    flag_counts: [u64; 4],
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HitFlagDiagnosticKey {
    session_id: String,
    context: HitFlagContextKey,
}

#[derive(Debug, Serialize)]
struct HitFlagDiagnosticTierReport {
    tier: &'static str,
    eligible_events: u64,
    distinct_contexts: u64,
    contexts_with_multiple_flag_states: u64,
    cross_flag_pairs: u64,
    normal_lucky_pairs: u64,
    normal_critical_pairs: u64,
    normal_critical_lucky_pairs: u64,
    lucky_critical_pairs: u64,
    lucky_critical_lucky_pairs: u64,
    critical_critical_lucky_pairs: u64,
}

#[derive(Debug, Default)]
struct HitFlagObservationAccumulator {
    flag_counts: [u64; 4],
    normal_value_present: u64,
    lucky_value_present: u64,
    both_packet_values_present: u64,
    neither_packet_value_present: u64,
    amount_matches_normal_value: u64,
    amount_matches_lucky_value: u64,
    amount_matches_neither_packet_value: u64,
    normal_hit_true_by_flag: [u64; 4],
    normal_hit_false_by_flag: [u64; 4],
    normal_hit_absent_by_flag: [u64; 4],
    packet_field_counts: BTreeMap<(&'static str, String), [u64; 4]>,
}

#[derive(Debug, Serialize)]
struct HitFlagObservationReport {
    normal_events: u64,
    lucky_events: u64,
    critical_events: u64,
    critical_lucky_events: u64,
    normal_value_present: u64,
    lucky_value_present: u64,
    both_packet_values_present: u64,
    neither_packet_value_present: u64,
    amount_matches_normal_value: u64,
    amount_matches_lucky_value: u64,
    amount_matches_neither_packet_value: u64,
}

#[derive(Debug, Serialize)]
struct PacketNormalHitOutcomeReport {
    flags: &'static str,
    packet_normal_hit_true: u64,
    packet_normal_hit_false: u64,
    packet_normal_hit_absent: u64,
}

#[derive(Debug, Serialize)]
struct PacketFieldOutcomeReport {
    field: &'static str,
    value: String,
    normal: u64,
    lucky: u64,
    critical: u64,
    critical_lucky: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct WireMessageKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SameWireDamageKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    wire: WireMessageKey,
}

#[derive(Debug, Clone, Serialize)]
struct SameWireDamageSample {
    sequence: u64,
    observed_micros: u64,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    blocked: bool,
    periodic: bool,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    property: Option<i32>,
    hit_part_ids: Vec<Option<i32>>,
    damage_weight_bits: Option<(Option<u32>, Option<u32>)>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_component: SkillEffectComponentIdentity,
    skill_effect_total_damage: Option<i64>,
    amount: i64,
    actual_amount: Option<i64>,
    critical: bool,
    lucky: bool,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    normal_hit: Option<bool>,
    crit_damage_raw: i64,
    lucky_damage_raw: i64,
    source_status_fingerprint: String,
    target_status_fingerprint: String,
}

#[derive(Debug, Serialize)]
struct SameWireDamageGroupReport {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
    samples: Vec<SameWireDamageSample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HitFlagPairGroupKey {
    first_flags: &'static str,
    second_flags: &'static str,
    crit_damage_raw: i64,
    lucky_damage_raw: i64,
}

#[derive(Debug, Default)]
struct HitFlagPairGroupAccumulator {
    count: u64,
    examples: Vec<HitFlagPairExample>,
}

#[derive(Debug, Serialize)]
struct HitFlagPairGroupReport {
    first_flags: &'static str,
    second_flags: &'static str,
    crit_damage_raw: i64,
    lucky_damage_raw: i64,
    count: u64,
    examples: Vec<HitFlagPairExample>,
}

#[derive(Debug, Clone, Serialize)]
struct HitFlagPairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    first_sequence: u64,
    second_sequence: u64,
    gap_micros: u64,
    first_flags: &'static str,
    second_flags: &'static str,
    first_amount: i64,
    second_amount: i64,
    first_actual_amount: Option<i64>,
    second_actual_amount: Option<i64>,
    first_normal_value: Option<i64>,
    second_normal_value: Option<i64>,
    first_lucky_value: Option<i64>,
    second_lucky_value: Option<i64>,
    crit_damage_raw: i64,
    lucky_damage_raw: i64,
    formula_residuals: BTreeMap<&'static str, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StatusKey {
    effect_id: i64,
    instance_id: Option<i64>,
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
            // Current-build BPSR also uses `Consumed` for a stack decrement.
            // The packet's stack scalar is the remaining count, so a nonzero
            // consumed event updates the active snapshot instead of ending it.
            StatusState::Consumed if value.stacks.is_some_and(|stacks| stacks > 0) => {
                self.active.insert(key, value);
            }
            StatusState::Consumed | StatusState::Removed => {
                self.active.remove(&key);
            }
        }
    }

    fn contains_effect(&self, effect_id: i64) -> bool {
        self.active.keys().any(|key| key.effect_id == effect_id)
    }

    /// Return the selected effect's exact remaining stack count.
    ///
    /// No active instance is an exact zero. More than one matching instance is
    /// deliberately unresolved because summing or choosing one would invent a
    /// stacking rule that the packet does not state.
    fn exact_effect_stacks(
        &self,
        effect_id: i64,
        provider_entity_uuid: Option<i64>,
        owner_by_direct_entity: &HashMap<i64, i64>,
    ) -> Option<u32> {
        let mut matches = self.active.iter().filter(|(key, _)| {
            if key.effect_id != effect_id {
                return false;
            }
            provider_entity_uuid.is_none_or(|provider| {
                key.source_entity_uuid
                    .map(|raw| owner_by_direct_entity.get(&raw).copied().unwrap_or(raw))
                    == Some(provider)
            })
        });
        let Some((_, value)) = matches.next() else {
            return Some(0);
        };
        if matches.next().is_some() {
            return None;
        }
        value.stacks.or(Some(1))
    }

    fn effect_providers(&self, effect_id: i64, target_entity_uuid: i64) -> Vec<i64> {
        self.active
            .keys()
            .filter(|key| key.effect_id == effect_id)
            .filter_map(|key| key.source_entity_uuid)
            .filter(|provider| *provider != target_entity_uuid)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn semantic_fingerprint_without(&self, excluded_effect_ids: &BTreeSet<i64>) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for (key, value) in &self.active {
            if excluded_effect_ids.contains(&key.effect_id) {
                continue;
            }
            for scalar in [
                key.effect_id,
                key.source_entity_uuid.unwrap_or(i64::MIN + 1),
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

    fn attacker_scoped_semantic_fingerprint_without(
        &self,
        excluded_effect_ids: &BTreeSet<i64>,
        attacker_scoped_effect_ids: &BTreeSet<i64>,
        attacker_entity_uuid: i64,
        owner_by_direct_entity: &HashMap<i64, i64>,
    ) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for (key, value) in &self.active {
            if excluded_effect_ids.contains(&key.effect_id) {
                continue;
            }
            if attacker_scoped_effect_ids.contains(&key.effect_id) {
                let provider = key
                    .source_entity_uuid
                    .map(|raw| owner_by_direct_entity.get(&raw).copied().unwrap_or(raw));
                if provider != Some(attacker_entity_uuid) {
                    continue;
                }
            }
            for scalar in [
                key.effect_id,
                key.source_entity_uuid.unwrap_or(i64::MIN + 1),
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

    fn semantic_snapshot_without(
        &self,
        excluded_effect_ids: &BTreeSet<i64>,
    ) -> Vec<SemanticStatusEntry> {
        let mut snapshot = self
            .active
            .iter()
            .filter(|(key, _)| !excluded_effect_ids.contains(&key.effect_id))
            .map(|(key, value)| SemanticStatusEntry {
                effect_id: key.effect_id,
                source_entity_uuid: key.source_entity_uuid,
                stacks: value.stacks,
                level: value.level,
                part_id: value.part_id,
                count: value.count,
            })
            .collect::<Vec<_>>();
        snapshot.sort();
        snapshot
    }

    fn attacker_scoped_semantic_snapshot_without(
        &self,
        excluded_effect_ids: &BTreeSet<i64>,
        attacker_scoped_effect_ids: &BTreeSet<i64>,
        attacker_entity_uuid: i64,
        owner_by_direct_entity: &HashMap<i64, i64>,
    ) -> Vec<SemanticStatusEntry> {
        let mut snapshot = self
            .active
            .iter()
            .filter(|(key, _)| !excluded_effect_ids.contains(&key.effect_id))
            .filter(|(key, _)| {
                if !attacker_scoped_effect_ids.contains(&key.effect_id) {
                    return true;
                }
                key.source_entity_uuid
                    .map(|raw| owner_by_direct_entity.get(&raw).copied().unwrap_or(raw))
                    == Some(attacker_entity_uuid)
            })
            .map(|(key, value)| SemanticStatusEntry {
                effect_id: key.effect_id,
                source_entity_uuid: key.source_entity_uuid,
                stacks: value.stacks,
                level: value.level,
                part_id: value.part_id,
                count: value.count,
            })
            .collect::<Vec<_>>();
        snapshot.sort();
        snapshot
    }

    fn semantic_fingerprint(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for (key, value) in &self.active {
            for scalar in [
                key.effect_id,
                key.source_entity_uuid.unwrap_or(i64::MIN + 1),
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

fn observe_selected_effect_counterfactual(
    accumulator: &mut SelectedEffectCounterfactualAccumulator,
    args: &Arguments,
    path: &Path,
    session_id: &str,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    damage: &DamageEvent,
    source_tracker: &StatusTracker,
    crit_damage_raw: i64,
    lucky_damage_raw: i64,
    selected_effect_transition_wire: bool,
) {
    let critical = damage.flags.critical == Some(true);
    let lucky = damage.flags.lucky == Some(true);
    if !critical && !lucky {
        return;
    }
    if selected_effect_transition_wire {
        accumulator.selected_effect_transition_wire_damage_events_excluded = accumulator
            .selected_effect_transition_wire_damage_events_excluded
            .saturating_add(1);
        return;
    }
    let providers = source_tracker.effect_providers(args.effect_id, damage.source.entity_uuid.0);
    if providers.is_empty() {
        return;
    }
    let crit_delta = if critical {
        args.effect_active_crit_delta.unwrap_or(0)
    } else {
        0
    };
    let luck_delta = if lucky {
        args.effect_active_luck_delta.unwrap_or(0)
    } else {
        0
    };
    if crit_delta == 0 && luck_delta == 0 {
        return;
    }

    let current_critical_factor = if critical {
        PERCENT_SCALE as i64 + crit_damage_raw
    } else {
        PERCENT_SCALE as i64
    };
    let current_lucky_factor = if lucky {
        lucky_damage_raw
    } else {
        PERCENT_SCALE as i64
    };
    let provider_removed_critical_factor = current_critical_factor - crit_delta;
    let provider_removed_lucky_factor = current_lucky_factor - luck_delta;
    if current_critical_factor <= 0
        || current_lucky_factor <= 0
        || provider_removed_critical_factor <= 0
        || provider_removed_lucky_factor <= 0
        || damage.amount < 0
    {
        return;
    }

    let candidates = if critical && lucky {
        vec![
            nested_counterfactual_candidate(
                "floor(floor(base * lucky_raw / 10000) * (10000 + crit_raw) / 10000)",
                damage.amount,
                current_lucky_factor,
                current_critical_factor,
                provider_removed_lucky_factor,
                provider_removed_critical_factor,
            ),
            nested_counterfactual_candidate(
                "floor(floor(base * (10000 + crit_raw) / 10000) * lucky_raw / 10000)",
                damage.amount,
                current_critical_factor,
                current_lucky_factor,
                provider_removed_critical_factor,
                provider_removed_lucky_factor,
            ),
            combined_counterfactual_candidate(
                "floor(base * lucky_raw * (10000 + crit_raw) / 100000000)",
                damage.amount,
                current_lucky_factor,
                current_critical_factor,
                provider_removed_lucky_factor,
                provider_removed_critical_factor,
            ),
        ]
    } else if critical {
        vec![single_counterfactual_candidate(
            "floor(base * (10000 + crit_raw) / 10000)",
            damage.amount,
            current_critical_factor,
            provider_removed_critical_factor,
        )]
    } else {
        vec![single_counterfactual_candidate(
            "floor(base * lucky_raw / 10000)",
            damage.amount,
            current_lucky_factor,
            provider_removed_lucky_factor,
        )]
    };
    let Some(candidates) = candidates.into_iter().collect::<Option<Vec<_>>>() else {
        return;
    };
    accumulator.external_provider_impact_events = accumulator
        .external_provider_impact_events
        .saturating_add(1);
    match (critical, lucky) {
        (true, true) => {
            accumulator.critical_lucky_events = accumulator.critical_lucky_events.saturating_add(1)
        }
        (true, false) => {
            accumulator.critical_only_events = accumulator.critical_only_events.saturating_add(1)
        }
        (false, true) => {
            accumulator.lucky_only_events = accumulator.lucky_only_events.saturating_add(1)
        }
        (false, false) => unreachable!(),
    }
    if providers.len() == 1 {
        accumulator.unique_external_provider_impact_events = accumulator
            .unique_external_provider_impact_events
            .saturating_add(1);
    } else {
        accumulator.ambiguous_provider_impact_events = accumulator
            .ambiguous_provider_impact_events
            .saturating_add(1);
    }

    let exact_stage_independent_amount = candidates
        .first()
        .filter(|candidate| candidate.counterfactual_min == candidate.counterfactual_max)
        .map(|candidate| candidate.counterfactual_min)
        .filter(|amount| {
            candidates.iter().all(|candidate| {
                candidate.counterfactual_min == *amount && candidate.counterfactual_max == *amount
            })
        });
    let exact_attributed_damage = exact_stage_independent_amount
        .filter(|_| providers.len() == 1)
        .map(|counterfactual| damage.amount.saturating_sub(counterfactual));
    let exact_accounting_fraction = (providers.len() == 1)
        .then(|| {
            exact_observed_share(
                i128::from(damage.amount),
                i128::from(current_critical_factor)
                    .checked_mul(i128::from(current_lucky_factor))?,
                i128::from(provider_removed_critical_factor)
                    .checked_mul(i128::from(provider_removed_lucky_factor))?,
            )
        })
        .flatten();
    if exact_accounting_fraction.is_some() {
        accumulator.exact_fraction_events = accumulator.exact_fraction_events.saturating_add(1);
        let bucket = accumulator
            .exact_fraction_buckets
            .entry(ExactFractionBucketKey {
                current_critical_factor,
                current_lucky_factor,
                provider_removed_critical_factor,
                provider_removed_lucky_factor,
            })
            .or_default();
        bucket.event_count = bucket.event_count.saturating_add(1);
        bucket.observed_damage_sum = bucket
            .observed_damage_sum
            .saturating_add(i128::from(damage.amount));
    }
    if exact_stage_independent_amount.is_some() {
        accumulator.exact_stage_independent_events =
            accumulator.exact_stage_independent_events.saturating_add(1);
    } else {
        accumulator.unresolved_stage_or_rounding_events = accumulator
            .unresolved_stage_or_rounding_events
            .saturating_add(1);
    }
    if let Some(attributed) = exact_attributed_damage {
        accumulator.exact_attributed_damage_sum = accumulator
            .exact_attributed_damage_sum
            .saturating_add(i128::from(attributed));
    }
    let example = SelectedEffectCounterfactualExample {
        rlog: path.display().to_string(),
        session_id: session_id.to_owned(),
        run_ordinal,
        sequence,
        observed_micros,
        source_entity_uuid: damage.source.entity_uuid.0,
        target_entity_uuid: damage.target.entity_uuid.0,
        provider_entity_uuids: providers,
        ability_id: damage.ability.map(|value| value.0),
        critical,
        lucky,
        observed_amount: damage.amount,
        current_critical_factor,
        current_lucky_factor,
        provider_removed_critical_factor,
        provider_removed_lucky_factor,
        candidate_counterfactuals: candidates,
        exact_stage_independent_amount,
        exact_attributed_damage,
        exact_accounting_fraction,
    };
    if exact_stage_independent_amount.is_some() {
        if accumulator.exact_examples.len() < args.example_limit {
            accumulator.exact_examples.push(example);
        }
    } else if accumulator.unresolved_examples.len() < args.example_limit {
        accumulator.unresolved_examples.push(example);
    }
}

fn single_counterfactual_candidate(
    formula: &'static str,
    amount: i64,
    current_factor: i64,
    removed_factor: i64,
) -> Option<CounterfactualCandidate> {
    let (base_min, base_max) = inverse_floor_interval(
        i128::from(amount),
        i128::from(current_factor),
        PERCENT_SCALE,
    )?;
    let (counterfactual_min, counterfactual_max) = forward_range(
        base_min,
        base_max,
        i128::from(removed_factor),
        PERCENT_SCALE,
    );
    Some(CounterfactualCandidate {
        formula,
        latent_base_min: i64::try_from(base_min).ok()?,
        latent_base_max: i64::try_from(base_max).ok()?,
        counterfactual_min: i64::try_from(counterfactual_min).ok()?,
        counterfactual_max: i64::try_from(counterfactual_max).ok()?,
    })
}

fn nested_counterfactual_candidate(
    formula: &'static str,
    amount: i64,
    current_first_factor: i64,
    current_second_factor: i64,
    removed_first_factor: i64,
    removed_second_factor: i64,
) -> Option<CounterfactualCandidate> {
    let (intermediate_min, intermediate_max) = inverse_floor_interval(
        i128::from(amount),
        i128::from(current_second_factor),
        PERCENT_SCALE,
    )?;
    let mut base_min = i128::MAX;
    let mut base_max = i128::MIN;
    for intermediate in intermediate_min..=intermediate_max {
        let (candidate_min, candidate_max) = inverse_floor_interval(
            intermediate,
            i128::from(current_first_factor),
            PERCENT_SCALE,
        )?;
        base_min = base_min.min(candidate_min);
        base_max = base_max.max(candidate_max);
    }
    let mut counterfactual_min = i128::MAX;
    let mut counterfactual_max = i128::MIN;
    for base in base_min..=base_max {
        let intermediate = floor_mul_div(base, i128::from(removed_first_factor), PERCENT_SCALE);
        let counterfactual = floor_mul_div(
            intermediate,
            i128::from(removed_second_factor),
            PERCENT_SCALE,
        );
        counterfactual_min = counterfactual_min.min(counterfactual);
        counterfactual_max = counterfactual_max.max(counterfactual);
    }
    Some(CounterfactualCandidate {
        formula,
        latent_base_min: i64::try_from(base_min).ok()?,
        latent_base_max: i64::try_from(base_max).ok()?,
        counterfactual_min: i64::try_from(counterfactual_min).ok()?,
        counterfactual_max: i64::try_from(counterfactual_max).ok()?,
    })
}

fn combined_counterfactual_candidate(
    formula: &'static str,
    amount: i64,
    current_first_factor: i64,
    current_second_factor: i64,
    removed_first_factor: i64,
    removed_second_factor: i64,
) -> Option<CounterfactualCandidate> {
    let denominator = PERCENT_SCALE.saturating_mul(PERCENT_SCALE);
    let current_factor =
        i128::from(current_first_factor).saturating_mul(i128::from(current_second_factor));
    let removed_factor =
        i128::from(removed_first_factor).saturating_mul(i128::from(removed_second_factor));
    let (base_min, base_max) =
        inverse_floor_interval(i128::from(amount), current_factor, denominator)?;
    let (counterfactual_min, counterfactual_max) =
        forward_range(base_min, base_max, removed_factor, denominator);
    Some(CounterfactualCandidate {
        formula,
        latent_base_min: i64::try_from(base_min).ok()?,
        latent_base_max: i64::try_from(base_max).ok()?,
        counterfactual_min: i64::try_from(counterfactual_min).ok()?,
        counterfactual_max: i64::try_from(counterfactual_max).ok()?,
    })
}

fn inverse_floor_interval(amount: i128, factor: i128, denominator: i128) -> Option<(i128, i128)> {
    if amount < 0 || factor <= 0 || denominator <= 0 {
        return None;
    }
    let minimum = ceil_div(amount.saturating_mul(denominator), factor);
    let maximum =
        ceil_div(amount.saturating_add(1).saturating_mul(denominator), factor).saturating_sub(1);
    (minimum <= maximum).then_some((minimum, maximum))
}

fn forward_range(base_min: i128, base_max: i128, factor: i128, denominator: i128) -> (i128, i128) {
    (
        floor_mul_div(base_min, factor, denominator),
        floor_mul_div(base_max, factor, denominator),
    )
}

fn floor_mul_div(value: i128, factor: i128, denominator: i128) -> i128 {
    value.saturating_mul(factor) / denominator
}

fn ceil_div(numerator: i128, denominator: i128) -> i128 {
    if numerator == 0 {
        0
    } else {
        numerator.saturating_add(denominator - 1) / denominator
    }
}

fn exact_observed_share(
    observed_damage: i128,
    current_factor: i128,
    provider_removed_factor: i128,
) -> Option<ExactFractionReport> {
    if observed_damage < 0
        || current_factor <= 0
        || provider_removed_factor <= 0
        || provider_removed_factor > current_factor
    {
        return None;
    }
    fraction_report(
        observed_damage.checked_mul(current_factor - provider_removed_factor)?,
        current_factor,
    )
}

fn fraction_report(numerator: i128, denominator: i128) -> Option<ExactFractionReport> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    let divisor = greatest_common_divisor(numerator, denominator);
    let reduced_numerator = numerator / divisor;
    let reduced_denominator = denominator / divisor;
    let floor = reduced_numerator / reduced_denominator;
    let ceil = if reduced_numerator % reduced_denominator == 0 {
        floor
    } else {
        floor.saturating_add(1)
    };
    let scaled = reduced_numerator
        .checked_mul(1_000_000_000)?
        .checked_div(reduced_denominator)?;
    let decimal_whole = scaled / 1_000_000_000;
    let decimal_fraction = scaled % 1_000_000_000;
    Some(ExactFractionReport {
        numerator: reduced_numerator.to_string(),
        denominator: reduced_denominator.to_string(),
        floor: floor.to_string(),
        ceil: ceil.to_string(),
        decimal_9_places: format!("{decimal_whole}.{decimal_fraction:09}"),
    })
}

fn greatest_common_divisor(mut left: i128, mut right: i128) -> i128 {
    left = left.abs();
    right = right.abs();
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn exact_fraction_bucket_report(
    key: ExactFractionBucketKey,
    accumulator: ExactFractionBucketAccumulator,
) -> ExactFractionBucketReport {
    let current_factor = i128::from(key.current_critical_factor)
        .saturating_mul(i128::from(key.current_lucky_factor));
    let removed_factor = i128::from(key.provider_removed_critical_factor)
        .saturating_mul(i128::from(key.provider_removed_lucky_factor));
    let provider_numerator = accumulator
        .observed_damage_sum
        .saturating_mul(current_factor.saturating_sub(removed_factor));
    let recipient_numerator = accumulator
        .observed_damage_sum
        .saturating_mul(removed_factor);
    let observed_numerator = accumulator
        .observed_damage_sum
        .saturating_mul(current_factor);
    ExactFractionBucketReport {
        current_critical_factor: key.current_critical_factor,
        current_lucky_factor: key.current_lucky_factor,
        provider_removed_critical_factor: key.provider_removed_critical_factor,
        provider_removed_lucky_factor: key.provider_removed_lucky_factor,
        event_count: accumulator.event_count,
        observed_damage_sum: accumulator.observed_damage_sum.to_string(),
        provider_attribution: fraction_report(provider_numerator, current_factor)
            .expect("validated positive fixed-point factors"),
        recipient_retained: fraction_report(recipient_numerator, current_factor)
            .expect("validated positive fixed-point factors"),
        conservation_identity_holds: provider_numerator.saturating_add(recipient_numerator)
            == observed_numerator,
    }
}

fn selected_effect_counterfactual_report(
    accumulator: SelectedEffectCounterfactualAccumulator,
) -> SelectedEffectCounterfactualReport {
    SelectedEffectCounterfactualReport {
        selected_effect_transition_wire_damage_events_excluded: accumulator
            .selected_effect_transition_wire_damage_events_excluded,
        external_provider_impact_events: accumulator.external_provider_impact_events,
        unique_external_provider_impact_events: accumulator.unique_external_provider_impact_events,
        ambiguous_provider_impact_events: accumulator.ambiguous_provider_impact_events,
        critical_only_events: accumulator.critical_only_events,
        lucky_only_events: accumulator.lucky_only_events,
        critical_lucky_events: accumulator.critical_lucky_events,
        exact_stage_independent_events: accumulator.exact_stage_independent_events,
        unresolved_stage_or_rounding_events: accumulator.unresolved_stage_or_rounding_events,
        exact_attributed_damage_sum: accumulator.exact_attributed_damage_sum,
        exact_fraction_events: accumulator.exact_fraction_events,
        exact_fraction_buckets: accumulator
            .exact_fraction_buckets
            .into_iter()
            .map(|(key, value)| exact_fraction_bucket_report(key, value))
            .collect(),
        exact_examples: accumulator.exact_examples,
        unresolved_examples: accumulator.unresolved_examples,
    }
}

fn selected_effect_proof_gap_report(
    sessions: &[SessionSummary],
    candidate_formulas: &[CandidateFormulaReport],
    selected_effect_bonus_raw: Option<i64>,
) -> SelectedEffectProofGapReport {
    let candidates_before_status_control = sessions
        .iter()
        .map(|session| session.effect_transition_candidates_before_status_control)
        .sum();
    let status_controlled_toggle_pairs = sessions
        .iter()
        .map(|session| session.strict_candidate_pairs)
        .sum();
    let fully_attribute_stable_toggle_pairs = sessions
        .iter()
        .map(|session| session.fully_attribute_stable_candidate_pairs)
        .sum();
    let attribute_adjusted_toggle_pairs = sessions
        .iter()
        .map(|session| session.attribute_adjusted_candidate_pairs)
        .sum();
    let confounded_toggle_pairs = sessions
        .iter()
        .map(|session| session.confounded_effect_transition_pairs)
        .sum();
    let top_source_status_confounders = aggregate_effect_counts(
        sessions
            .iter()
            .flat_map(|session| session.source_status_confounders.iter()),
    );
    let top_target_status_confounders = aggregate_effect_counts(
        sessions
            .iter()
            .flat_map(|session| session.target_status_confounders.iter()),
    );
    let exact_candidate_formula_observed = candidate_formulas.iter().any(|formula| {
        let selected_formula_required = selected_effect_bonus_raw.is_some();
        (!selected_formula_required || formula.formula.ends_with("_x_selected_effect_final"))
            && formula.evaluable_pairs > 0
            && formula.exact_matches == formula.evaluable_pairs
            && formula.mismatches == 0
    });
    let formula_placement_status = formula_placement_status(
        status_controlled_toggle_pairs,
        fully_attribute_stable_toggle_pairs,
        exact_candidate_formula_observed,
    );
    SelectedEffectProofGapReport {
        candidates_before_status_control,
        status_controlled_toggle_pairs,
        controlled_toggle_pairs: status_controlled_toggle_pairs,
        fully_attribute_stable_toggle_pairs,
        attribute_adjusted_toggle_pairs,
        confounded_toggle_pairs,
        top_source_status_confounders,
        top_target_status_confounders,
        minimum_exact_selector: vec![
            "same-session-and-run",
            "same-source-and-direct-source-ownership",
            "same-target",
            "same-ability-and-complete-packet-damage-identity",
            "same-critical-lucky-blocked-and-periodic-outcome",
            "same-context-attribute-values",
            "same-all-other-source-status-fingerprint",
            "same-all-other-target-status-fingerprint",
            "only-selected-effect-active-state-or-stack-count-differs",
            "selected-effect-transition-wire-damage-excluded",
            "critical-and-lucky-attributes-must-either-remain-stable-or-be-exactly-modeled",
        ],
        controlled_selector_satisfied: status_controlled_toggle_pairs > 0,
        fully_controlled_selector_satisfied: fully_attribute_stable_toggle_pairs > 0,
        exact_candidate_formula_observed,
        formula_placement_status,
        runtime_promotion_allowed: false,
        unresolved_evidence_is_hidden: false,
    }
}

fn formula_placement_status(
    status_controlled_toggle_pairs: u64,
    fully_attribute_stable_toggle_pairs: u64,
    exact_candidate_formula_observed: bool,
) -> &'static str {
    if status_controlled_toggle_pairs == 0 {
        "status-controlled-effect-toggle-pair-unavailable"
    } else if !exact_candidate_formula_observed {
        if fully_attribute_stable_toggle_pairs == 0 {
            "status-controlled-toggle-observed-with-attribute-change-but-exact-formula-unresolved"
        } else {
            "fully-controlled-toggle-observed-but-exact-formula-unresolved"
        }
    } else {
        "exact-candidate-observed-pending-conservation-and-closure-review"
    }
}

fn aggregate_effect_counts<'a>(
    counts: impl Iterator<Item = &'a EffectCountReport>,
) -> Vec<EffectCountReport> {
    let mut totals = BTreeMap::<i64, u64>::new();
    for entry in counts {
        let total = totals.entry(entry.effect_id).or_default();
        *total = total.saturating_add(entry.count);
    }
    let mut reports = totals
        .into_iter()
        .map(|(effect_id, count)| EffectCountReport { effect_id, count })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.effect_id.cmp(&right.effect_id))
    });
    reports.truncate(DEFAULT_EXAMPLE_LIMIT);
    reports
}

fn main() {
    if let Err(error) = run() {
        eprintln!("damage multiplier proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let mut sessions = Vec::new();
    let mut groups = BTreeMap::<PairGroupKey, PairGroupAccumulator>::new();
    let mut formulas = candidate_formula_names(args.selected_effect_bonus_raw)
        .into_iter()
        .map(|name| (name, CandidateFormulaAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut hit_flag_groups = BTreeMap::<HitFlagPairGroupKey, HitFlagPairGroupAccumulator>::new();
    let mut hit_flag_formulas = hit_flag_candidate_formula_names()
        .into_iter()
        .map(|name| (name, CandidateFormulaAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut hit_flag_diagnostics = hit_flag_diagnostic_tier_names()
        .into_iter()
        .map(|name| {
            (
                name,
                BTreeMap::<HitFlagDiagnosticKey, HitFlagDiagnosticContextAccumulator>::new(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut hit_flag_observations = HitFlagObservationAccumulator::default();
    let mut selected_effect_counterfactuals = SelectedEffectCounterfactualAccumulator::default();
    let mut same_wire_damage_groups =
        BTreeMap::<SameWireDamageKey, Vec<SameWireDamageSample>>::new();

    for path in &args.rlogs {
        sessions.push(read_session(
            path,
            &args,
            &mut groups,
            &mut formulas,
            &mut hit_flag_groups,
            &mut hit_flag_formulas,
            &mut hit_flag_diagnostics,
            &mut hit_flag_observations,
            &mut same_wire_damage_groups,
            &mut selected_effect_counterfactuals,
        )?);
    }

    let pair_groups = groups
        .into_iter()
        .map(|(key, value)| PairGroupReport {
            critical: key.critical,
            lucky: key.lucky,
            crit_raw_delta: key.crit_raw_delta,
            luck_raw_delta: key.luck_raw_delta,
            effect_transition: key.effect_transition,
            first_effect_stacks: key.first_effect_stacks,
            second_effect_stacks: key.second_effect_stacks,
            count: value.count,
            examples: value.examples,
        })
        .collect();
    let candidate_formulas: Vec<CandidateFormulaReport> = formulas
        .into_iter()
        .map(|(formula, value)| CandidateFormulaReport {
            formula,
            evaluable_pairs: value.evaluable_pairs,
            exact_matches: value.exact_matches,
            within_one_matches: value.within_one_matches,
            mismatches: value.mismatches,
            mean_absolute_residual: (value.evaluable_pairs > 0)
                .then_some(value.absolute_residual_sum as f64 / value.evaluable_pairs as f64),
            maximum_absolute_residual: value.maximum_absolute_residual,
            residual_examples: value.residual_examples.into_iter().collect(),
        })
        .collect();
    let hit_flag_pair_groups = hit_flag_groups
        .into_iter()
        .map(|(key, value)| HitFlagPairGroupReport {
            first_flags: key.first_flags,
            second_flags: key.second_flags,
            crit_damage_raw: key.crit_damage_raw,
            lucky_damage_raw: key.lucky_damage_raw,
            count: value.count,
            examples: value.examples,
        })
        .collect();
    let hit_flag_candidate_formulas = formula_reports(hit_flag_formulas);
    let hit_flag_diagnostic_tiers = hit_flag_diagnostic_reports(hit_flag_diagnostics);
    let same_wire_damage_groups = same_wire_damage_group_reports(same_wire_damage_groups);
    let selected_effect_counterfactuals =
        selected_effect_counterfactual_report(selected_effect_counterfactuals);
    let selected_effect_proof_gap = selected_effect_proof_gap_report(
        &sessions,
        &candidate_formulas,
        args.selected_effect_bonus_raw,
    );
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-damage-multiplier-proof",
        policy: AuditPolicy {
            runtime_use: "offline_research_only_never_loaded_by_capture_or_live_meter",
            pair_scope: "same_session_run_source_direct_source_target_ability_hit_event_damage_source_damage_type_flags_owner_level_owner_stage_normal_hit_property_hit_part_ids_damage_weight_passive_rainbow_damage_mode_exact_skill_effect_uuid_group_component_index_component_count_and_context_attributes_within_configured_gap; protobuf damage_pos, hit-part damage_pos, and skill_effect_total_damage are retained as presentation evidence but are not calculation identity",
            status_control: if args.compare_selected_effect_stacks {
                "selected_effect_must_change_one_exact_packet_observed_stack_count_while_exact_semantic_effect_source_stacks_level_part_and_count_fingerprints_excluding_that_effect_match_on_source_and_target; zero_active_instances_is_exactly_zero; multiple_matching_instances_are_retained_as_unresolved_and_excluded_without_summing_or_selection"
            } else {
                match (
                    args.ignore_source_status_in_hit_flag_pair_key,
                    args.ignore_target_status_in_hit_flag_pair_key,
                ) {
                    (false, false) => {
                        "selected_effect_must_change_active_state_while_exact_semantic_effect_source_stacks_level_part_and_count_fingerprints_excluding_that_effect_match_on_source_and_target; transient_instance_ids_are_excluded"
                    }
                    (true, false) => {
                        "diagnostic_selected_effect_transition_excludes_other_source_statuses_but_requires_exact_target_status_fingerprint"
                    }
                    (false, true) => {
                        "diagnostic_selected_effect_transition_requires_exact_other_source_status_fingerprint_but_excludes_target_statuses"
                    }
                    (true, true) => {
                        "diagnostic_selected_effect_transition_excludes_other_source_and_target_status_fingerprints"
                    }
                }
            },
            hit_flag_status_control: "same_exact_active_effect_source_stacks_level_part_and_count_on_source_and_target; transient_instance_ids_are_not_mechanics_and_are_excluded",
            attribute_scope: "both_exact_current_crit_and_luck_attribute_values_must_be_known_on_both_hits_and_at_least_one_must_change; context_attributes_are_pair_identity; diagnostic_attributes_are_retained_as_first_second_delta_evidence_but_never_select_pairs_or_authorize_formulas",
            amount_scope: "exact requested damage plus distinct normal/lucky wire values and packet dimensions retained by the current decoder",
            packet_normal_hit_control: if args.ignore_packet_normal_hit_in_pair_key {
                "diagnostic_mode_excludes_the_unresolved_packet_normal_boolean_from_hit_outcome_pair_identity_but_retains_all_other_packet_dimensions"
            } else {
                "strict_mode_requires_the_unresolved_packet_normal_boolean_to_match"
            },
            hit_flag_status_pair_control: match (
                args.ignore_source_status_in_hit_flag_pair_key,
                args.ignore_target_status_in_hit_flag_pair_key,
            ) {
                (false, false) => {
                    "strict_mode_requires_exact_source_and_target_status_fingerprints"
                }
                (true, false) => {
                    "diagnostic_mode_excludes_source_status_fingerprint_but_requires_exact_target_status_fingerprint"
                }
                (false, true) => {
                    "diagnostic_mode_requires_exact_source_status_fingerprint_but_excludes_target_status_fingerprint"
                }
                (true, true) => "diagnostic_mode_excludes_source_and_target_status_fingerprints",
            },
            formula_scope: "candidate_ratio_residuals_are_diagnostics_not_runtime_formula_authority",
            same_wire_scope: "same_rlog_session_run_capture_connection_and_stream_only; only raw wire messages containing both a normal_value component and a lucky_value component are reported; every component and its complete hit identity in each qualifying message are retained without assuming which components are paired",
            counterfactual_scope: "only packet outcomes observed while the selected effect has one externally sourced provider and the configured raw attribute deltas are present; critical uses factor 10000 plus AttrCritDamage, Lucky Strike uses AttrLuckDamInc directly, all integer rounding-compatible latent bases are retained, and attribution is exact only when lucky-then-critical, critical-then-lucky, and single-product stage candidates all produce the same single counterfactual amount",
            exact_accounting_scope: "for every event with one exact external provider, the accounting transfer is the reduced rational observed_damage * (current_combined_factor - provider_removed_combined_factor) / current_combined_factor; provider gain and recipient subtraction retain the same numerator and denominator, while integer game-counterfactual floor intervals remain separately visible",
            unresolved_evidence_is_hidden: false,
        },
        selected_effect_id: args.effect_id,
        crit_damage_attribute_id: args.crit_attribute_id,
        lucky_damage_attribute_id: args.luck_attribute_id,
        max_pair_gap_micros: args.max_gap_micros,
        expected_effect_active_crit_delta: args.effect_active_crit_delta,
        expected_effect_active_luck_delta: args.effect_active_luck_delta,
        selected_effect_bonus_raw: args.selected_effect_bonus_raw,
        ignored_packet_normal_hit_in_pair_key: args.ignore_packet_normal_hit_in_pair_key,
        ignored_source_status_in_hit_flag_pair_key: args.ignore_source_status_in_hit_flag_pair_key,
        ignored_target_status_in_hit_flag_pair_key: args.ignore_target_status_in_hit_flag_pair_key,
        selected_effect_scope: if args.selected_effect_on_target {
            "target"
        } else {
            "source"
        },
        selected_effect_provider_is_attacker: args.selected_effect_provider_is_attacker,
        compare_selected_effect_stacks: args.compare_selected_effect_stacks,
        ignored_status_effect_ids: args.ignored_status_effect_ids.iter().copied().collect(),
        attacker_scoped_target_effect_ids: args
            .attacker_scoped_target_effect_ids
            .iter()
            .copied()
            .collect(),
        context_attribute_ids: args.context_attribute_ids.iter().copied().collect(),
        diagnostic_attribute_ids: args.diagnostic_attribute_ids.iter().copied().collect(),
        sessions,
        pair_groups,
        candidate_formulas,
        hit_flag_pair_groups,
        hit_flag_candidate_formulas,
        hit_flag_diagnostic_tiers,
        packet_normal_hit_outcomes: packet_normal_hit_outcome_reports(&hit_flag_observations),
        packet_field_outcomes: packet_field_outcome_reports(&hit_flag_observations),
        hit_flag_observations: hit_flag_observation_report(&hit_flag_observations),
        same_wire_damage_groups,
        selected_effect_counterfactuals,
        selected_effect_proof_gap,
    };
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn direct_source_owners(
    path: &Path,
) -> Result<HashMap<u32, HashMap<i64, i64>>, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut current_run_ordinal = 0_u32;
    let mut owners = HashMap::<u32, HashMap<i64, i64>>::new();
    while let Some(envelope) = reader.next_event()? {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => current_run_ordinal = current_run_ordinal.saturating_add(1),
                RunState::Started if current_run_ordinal == 0 => current_run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::Damage(damage) => {
                if let Some(direct) = damage
                    .direct_source
                    .filter(|direct| direct.entity_uuid != damage.source.entity_uuid)
                {
                    owners
                        .entry(current_run_ordinal)
                        .or_default()
                        .entry(direct.entity_uuid.0)
                        .or_insert(damage.source.entity_uuid.0);
                }
            }
            TimelineEventKind::Healing(healing) => {
                if let Some(direct) = healing
                    .direct_source
                    .filter(|direct| direct.entity_uuid != healing.source.entity_uuid)
                {
                    owners
                        .entry(current_run_ordinal)
                        .or_default()
                        .entry(direct.entity_uuid.0)
                        .or_insert(healing.source.entity_uuid.0);
                }
            }
            _ => {}
        }
    }
    Ok(owners)
}

fn read_session(
    path: &Path,
    args: &Arguments,
    groups: &mut BTreeMap<PairGroupKey, PairGroupAccumulator>,
    formulas: &mut BTreeMap<&'static str, CandidateFormulaAccumulator>,
    hit_flag_groups: &mut BTreeMap<HitFlagPairGroupKey, HitFlagPairGroupAccumulator>,
    hit_flag_formulas: &mut BTreeMap<&'static str, CandidateFormulaAccumulator>,
    hit_flag_diagnostics: &mut BTreeMap<
        &'static str,
        BTreeMap<HitFlagDiagnosticKey, HitFlagDiagnosticContextAccumulator>,
    >,
    hit_flag_observations: &mut HitFlagObservationAccumulator,
    same_wire_damage_groups: &mut BTreeMap<SameWireDamageKey, Vec<SameWireDamageSample>>,
    selected_effect_counterfactuals: &mut SelectedEffectCounterfactualAccumulator,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    let selected_effect_transition_wires = selected_effect_transition_wires(path, args.effect_id)?;
    let owner_by_run_and_direct_entity = direct_source_owners(path)?;
    let empty_owner_map = HashMap::new();
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = None::<String>;
    let mut current_run_ordinal = 0_u32;
    let mut maximum_run_ordinal = 0_u32;
    let mut damage_events = 0_u64;
    let mut relevant_damage_events = 0_u64;
    let mut damage_events_with_both_attributes_known = 0_u64;
    let mut selected_status_events = 0_u64;
    let mut effect_transition_candidates_before_status_control = 0_u64;
    let mut strict_candidate_pairs = 0_u64;
    let mut fully_attribute_stable_candidate_pairs = 0_u64;
    let mut attribute_adjusted_candidate_pairs = 0_u64;
    let mut confounded_effect_transition_pairs = 0_u64;
    let mut source_status_confounders = BTreeMap::<i64, u64>::new();
    let mut target_status_confounders = BTreeMap::<i64, u64>::new();
    let mut confounded_effect_transition_examples = Vec::new();
    let mut confounded_expected_delta_pairs = 0_u64;
    let mut confounded_expected_delta_examples = Vec::<ConfoundedEffectTransitionExample>::new();
    let mut hit_flag_eligible_damage_events = 0_u64;
    let mut strict_hit_flag_pairs = 0_u64;
    let mut attributes = HashMap::<(u32, i64, i32), i64>::new();
    let mut statuses = HashMap::<(u32, i64), StatusTracker>::new();
    let mut excluded_pair_status_effect_ids = args.ignored_status_effect_ids.clone();
    excluded_pair_status_effect_ids.insert(args.effect_id);
    let mut recent = BTreeMap::<DamageKey, VecDeque<DamageSample>>::new();
    let mut recent_hit_flags = BTreeMap::<HitFlagContextKey, VecDeque<HitFlagSample>>::new();

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
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    clear_actor_attribute_snapshot(
                        &mut attributes,
                        current_run_ordinal,
                        event.actor.entity_uuid.0,
                    );
                }
                for attribute in &event.attributes {
                    if attribute.attribute_id != args.crit_attribute_id
                        && attribute.attribute_id != args.luck_attribute_id
                        && !args.context_attribute_ids.contains(&attribute.attribute_id)
                        && !args
                            .diagnostic_attribute_ids
                            .contains(&attribute.attribute_id)
                    {
                        continue;
                    }
                    if let Some(value) = decode_attribute(attribute) {
                        attributes.insert(
                            (
                                current_run_ordinal,
                                event.actor.entity_uuid.0,
                                attribute.attribute_id,
                            ),
                            value,
                        );
                    }
                }
            }
            TimelineEventKind::Status(status) => {
                if status.effect.0 == args.effect_id {
                    selected_status_events = selected_status_events.saturating_add(1);
                }
                statuses
                    .entry((current_run_ordinal, status.target.entity_uuid.0))
                    .or_default()
                    .observe(
                        StatusKey {
                            effect_id: status.effect.0,
                            instance_id: status.instance_id.map(|value| value.0),
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
                let event_wire = wire_message_key(&envelope.provenance.source);
                let selected_effect_transition_wire = event_wire
                    .map(|wire| {
                        selected_effect_transition_wires.contains(&(current_run_ordinal, wire))
                    })
                    .unwrap_or(false);
                let critical = damage.flags.critical == Some(true);
                let lucky = damage.flags.lucky == Some(true);
                let source_uuid = damage.source.entity_uuid.0;
                let owner_by_direct_entity = owner_by_run_and_direct_entity
                    .get(&current_run_ordinal)
                    .unwrap_or(&empty_owner_map);
                let Some(crit_damage_raw) = attributes
                    .get(&(current_run_ordinal, source_uuid, args.crit_attribute_id))
                    .copied()
                else {
                    continue;
                };
                let Some(lucky_damage_raw) = attributes
                    .get(&(current_run_ordinal, source_uuid, args.luck_attribute_id))
                    .copied()
                else {
                    continue;
                };
                damage_events_with_both_attributes_known =
                    damage_events_with_both_attributes_known.saturating_add(1);
                let source_tracker = statuses
                    .get(&(current_run_ordinal, source_uuid))
                    .unwrap_or(&EMPTY_STATUS_TRACKER);
                let target_tracker = statuses
                    .get(&(current_run_ordinal, damage.target.entity_uuid.0))
                    .unwrap_or(&EMPTY_STATUS_TRACKER);
                if !args.selected_effect_on_target {
                    observe_selected_effect_counterfactual(
                        selected_effect_counterfactuals,
                        args,
                        path,
                        &envelope.session_id,
                        current_run_ordinal,
                        envelope.sequence,
                        envelope.time.observed_micros,
                        damage,
                        source_tracker,
                        crit_damage_raw,
                        lucky_damage_raw,
                        selected_effect_transition_wire,
                    );
                }
                hit_flag_eligible_damage_events = hit_flag_eligible_damage_events.saturating_add(1);
                let skill_effect_component = SkillEffectComponentIdentity {
                    skill_effect_uuid: damage.packet.skill_effect_uuid,
                    skill_effect_group_index: damage.packet.skill_effect_group_index,
                    skill_effect_component_index: damage.packet.skill_effect_component_index,
                    skill_effect_component_count: damage.packet.skill_effect_component_count,
                };
                let hit_flag_context = HitFlagContextKey {
                    run_ordinal: current_run_ordinal,
                    source_entity_uuid: source_uuid,
                    direct_source_entity_uuid: damage
                        .direct_source
                        .map(|value| value.entity_uuid.0),
                    raw_attacker_uuid: damage.packet.attacker_uuid,
                    raw_top_summoner_uuid: damage.packet.top_summoner_uuid,
                    raw_owner_id: damage.packet.owner_id,
                    target_entity_uuid: damage.target.entity_uuid.0,
                    ability_id: damage.ability.map(|value| value.0),
                    hit_event_id: damage.hit_event_id,
                    damage_source: damage.damage_source,
                    damage_type: damage.damage_type,
                    blocked: damage.flags.blocked == Some(true),
                    periodic: damage.flags.periodic == Some(true),
                    crit_damage_raw,
                    lucky_damage_raw,
                    source_status_fingerprint: if args.ignore_source_status_in_hit_flag_pair_key {
                        0
                    } else {
                        source_tracker.semantic_fingerprint()
                    },
                    target_status_fingerprint: if args.ignore_target_status_in_hit_flag_pair_key {
                        0
                    } else {
                        target_tracker.semantic_fingerprint()
                    },
                    owner_level: damage.packet.owner_level,
                    owner_stage: damage.packet.owner_stage,
                    normal_hit: if args.ignore_packet_normal_hit_in_pair_key {
                        None
                    } else {
                        damage.packet.normal_hit
                    },
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
                    skill_effect_component,
                };
                let hit_flag_sample = HitFlagSample {
                    rlog: path.display().to_string(),
                    session_id: envelope.session_id.clone(),
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    amount: damage.amount,
                    actual_amount: damage.actual_amount,
                    critical,
                    lucky,
                    normal_value: damage.packet.normal_value,
                    lucky_value: damage.packet.lucky_value,
                    normal_hit: damage.packet.normal_hit,
                };
                if let Some(wire) = event_wire {
                    let same_wire_key = SameWireDamageKey {
                        rlog: path.display().to_string(),
                        session_id: envelope.session_id.clone(),
                        run_ordinal: current_run_ordinal,
                        wire,
                    };
                    same_wire_damage_groups
                        .entry(same_wire_key)
                        .or_default()
                        .push(SameWireDamageSample {
                            sequence: envelope.sequence,
                            observed_micros: envelope.time.observed_micros,
                            source_entity_uuid: source_uuid,
                            direct_source_entity_uuid: damage
                                .direct_source
                                .map(|value| value.entity_uuid.0),
                            target_entity_uuid: damage.target.entity_uuid.0,
                            ability_id: damage.ability.map(|value| value.0),
                            hit_event_id: damage.hit_event_id,
                            damage_source: damage.damage_source,
                            damage_type: damage.damage_type,
                            blocked: damage.flags.blocked == Some(true),
                            periodic: damage.flags.periodic == Some(true),
                            owner_level: damage.packet.owner_level,
                            owner_stage: damage.packet.owner_stage,
                            property: damage.packet.property,
                            hit_part_ids: damage
                                .packet
                                .hit_parts
                                .iter()
                                .map(|part| part.part_id)
                                .collect(),
                            damage_weight_bits: damage.packet.damage_weight.map(|weight| {
                                (weight.x.map(f32::to_bits), weight.y.map(f32::to_bits))
                            }),
                            passive_uuid: damage.packet.passive_uuid,
                            rainbow: damage.packet.rainbow,
                            damage_mode: damage.packet.damage_mode,
                            skill_effect_component,
                            skill_effect_total_damage: damage.packet.skill_effect_total_damage,
                            amount: damage.amount,
                            actual_amount: damage.actual_amount,
                            critical,
                            lucky,
                            reported_critical: damage.packet.reported_critical,
                            type_flags: damage.packet.type_flags,
                            normal_value: damage.packet.normal_value,
                            lucky_value: damage.packet.lucky_value,
                            normal_hit: damage.packet.normal_hit,
                            crit_damage_raw,
                            lucky_damage_raw,
                            source_status_fingerprint: format!(
                                "{:016x}",
                                source_tracker.semantic_fingerprint()
                            ),
                            target_status_fingerprint: format!(
                                "{:016x}",
                                target_tracker.semantic_fingerprint()
                            ),
                        });
                }
                observe_packet_fields(damage, critical, lucky, hit_flag_observations);
                observe_hit_flag_sample(&hit_flag_sample, hit_flag_observations);
                observe_hit_flag_diagnostics(
                    &envelope.session_id,
                    &hit_flag_context,
                    critical,
                    lucky,
                    hit_flag_diagnostics,
                );
                let hit_flag_samples = recent_hit_flags
                    .entry(hit_flag_context.clone())
                    .or_default();
                let mut compared_flags = BTreeSet::new();
                for previous in hit_flag_samples.iter().rev() {
                    let gap = hit_flag_sample
                        .observed_micros
                        .saturating_sub(previous.observed_micros);
                    if gap > args.max_gap_micros {
                        break;
                    }
                    if (previous.critical, previous.lucky) == (critical, lucky)
                        || !compared_flags.insert((previous.critical, previous.lucky))
                    {
                        continue;
                    }
                    strict_hit_flag_pairs = strict_hit_flag_pairs.saturating_add(1);
                    observe_hit_flag_pair(
                        args,
                        &hit_flag_context,
                        previous,
                        &hit_flag_sample,
                        hit_flag_groups,
                        hit_flag_formulas,
                    );
                }
                hit_flag_samples.push_back(hit_flag_sample);
                while hit_flag_samples.len() > RECENT_PER_KEY {
                    hit_flag_samples.pop_front();
                }

                if !args.selected_effect_on_target && !critical && !lucky {
                    continue;
                }
                relevant_damage_events = relevant_damage_events.saturating_add(1);
                let selected_effect_tracker = if args.selected_effect_on_target {
                    target_tracker
                } else {
                    source_tracker
                };
                let selected_effect_stacks = selected_effect_tracker.exact_effect_stacks(
                    args.effect_id,
                    args.selected_effect_provider_is_attacker
                        .then_some(source_uuid),
                    owner_by_direct_entity,
                );
                if args.compare_selected_effect_stacks && selected_effect_stacks.is_none() {
                    continue;
                }
                let sample = DamageSample {
                    rlog: path.display().to_string(),
                    session_id: envelope.session_id.clone(),
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    amount: damage.amount,
                    actual_amount: damage.actual_amount,
                    hp_loss: damage.hp_loss,
                    shield_loss: damage.shield_loss,
                    normal_value: damage.packet.normal_value,
                    lucky_value: damage.packet.lucky_value,
                    reported_critical: damage.packet.reported_critical,
                    type_flags: damage.packet.type_flags,
                    normal_hit: damage.packet.normal_hit,
                    property: damage.packet.property,
                    passive_uuid: damage.packet.passive_uuid,
                    rainbow: damage.packet.rainbow,
                    damage_mode: damage.packet.damage_mode,
                    skill_effect_total_damage: damage.packet.skill_effect_total_damage,
                    position_bits: damage.packet.position.map(|position| {
                        (
                            position.x.map(f32::to_bits),
                            position.y.map(f32::to_bits),
                            position.z.map(f32::to_bits),
                        )
                    }),
                    hit_part_context: damage
                        .packet
                        .hit_parts
                        .iter()
                        .map(|part| {
                            (
                                part.part_id,
                                part.position.map(|position| {
                                    (
                                        position.x.map(f32::to_bits),
                                        position.y.map(f32::to_bits),
                                        position.z.map(f32::to_bits),
                                    )
                                }),
                            )
                        })
                        .collect(),
                    effect_active: if args.selected_effect_provider_is_attacker {
                        selected_effect_stacks.is_some_and(|stacks| stacks > 0)
                    } else if args.selected_effect_on_target {
                        target_tracker.contains_effect(args.effect_id)
                    } else {
                        source_tracker.contains_effect(args.effect_id)
                    },
                    effect_stacks: selected_effect_stacks,
                    crit_damage_raw,
                    lucky_damage_raw,
                    source_status_fingerprint: if args.ignore_source_status_in_hit_flag_pair_key {
                        0
                    } else {
                        source_tracker
                            .semantic_fingerprint_without(&excluded_pair_status_effect_ids)
                    },
                    target_status_fingerprint: if args.ignore_target_status_in_hit_flag_pair_key {
                        0
                    } else {
                        target_tracker.attacker_scoped_semantic_fingerprint_without(
                            &excluded_pair_status_effect_ids,
                            &args.attacker_scoped_target_effect_ids,
                            source_uuid,
                            owner_by_direct_entity,
                        )
                    },
                    source_status_snapshot: source_tracker
                        .semantic_snapshot_without(&excluded_pair_status_effect_ids),
                    target_status_snapshot: target_tracker
                        .attacker_scoped_semantic_snapshot_without(
                            &excluded_pair_status_effect_ids,
                            &args.attacker_scoped_target_effect_ids,
                            source_uuid,
                            owner_by_direct_entity,
                        ),
                    diagnostic_attribute_values: args
                        .diagnostic_attribute_ids
                        .iter()
                        .map(|attribute_id| {
                            (
                                *attribute_id,
                                attributes
                                    .get(&(current_run_ordinal, source_uuid, *attribute_id))
                                    .copied(),
                            )
                        })
                        .collect(),
                };
                let key = DamageKey {
                    run_ordinal: current_run_ordinal,
                    source_entity_uuid: source_uuid,
                    direct_source_entity_uuid: damage
                        .direct_source
                        .map(|value| value.entity_uuid.0),
                    raw_attacker_uuid: damage.packet.attacker_uuid,
                    raw_top_summoner_uuid: damage.packet.top_summoner_uuid,
                    raw_owner_id: damage.packet.owner_id,
                    target_entity_uuid: damage.target.entity_uuid.0,
                    ability_id: damage.ability.map(|value| value.0),
                    hit_event_id: damage.hit_event_id,
                    damage_source: damage.damage_source,
                    damage_type: damage.damage_type,
                    missed: damage.packet.missed,
                    reported_critical: damage.packet.reported_critical,
                    type_flags: damage.packet.type_flags,
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
                    skill_effect_component,
                    critical,
                    lucky,
                    blocked: damage.flags.blocked == Some(true),
                    periodic: damage.flags.periodic == Some(true),
                    context_attribute_values: args
                        .context_attribute_ids
                        .iter()
                        .map(|attribute_id| {
                            (
                                *attribute_id,
                                attributes
                                    .get(&(current_run_ordinal, source_uuid, *attribute_id))
                                    .copied(),
                            )
                        })
                        .collect(),
                };
                let samples = recent.entry(key.clone()).or_default();
                for previous in samples.iter().rev() {
                    let gap = sample
                        .observed_micros
                        .saturating_sub(previous.observed_micros);
                    if gap > args.max_gap_micros {
                        break;
                    }
                    let selected_effect_unchanged = if args.compare_selected_effect_stacks {
                        previous.effect_stacks == sample.effect_stacks
                    } else {
                        previous.effect_active == sample.effect_active
                    };
                    if selected_effect_unchanged
                        || (!args.selected_effect_on_target
                            && previous.crit_damage_raw == sample.crit_damage_raw
                            && previous.lucky_damage_raw == sample.lucky_damage_raw)
                    {
                        continue;
                    }
                    effect_transition_candidates_before_status_control =
                        effect_transition_candidates_before_status_control.saturating_add(1);
                    if previous.source_status_fingerprint != sample.source_status_fingerprint
                        || previous.target_status_fingerprint != sample.target_status_fingerprint
                    {
                        confounded_effect_transition_pairs =
                            confounded_effect_transition_pairs.saturating_add(1);
                        let (source_removed, source_added) = semantic_status_diff(
                            &previous.source_status_snapshot,
                            &sample.source_status_snapshot,
                        );
                        let (target_removed, target_added) = semantic_status_diff(
                            &previous.target_status_snapshot,
                            &sample.target_status_snapshot,
                        );
                        record_status_confounders(
                            &mut source_status_confounders,
                            source_removed.iter().chain(source_added.iter()),
                        );
                        record_status_confounders(
                            &mut target_status_confounders,
                            target_removed.iter().chain(target_added.iter()),
                        );
                        let crit_delta = sample
                            .crit_damage_raw
                            .saturating_sub(previous.crit_damage_raw);
                        let luck_delta = sample
                            .lucky_damage_raw
                            .saturating_sub(previous.lucky_damage_raw);
                        let direction = if args.compare_selected_effect_stacks {
                            if sample.effect_stacks.unwrap_or_default()
                                > previous.effect_stacks.unwrap_or_default()
                            {
                                1
                            } else {
                                -1
                            }
                        } else if !previous.effect_active && sample.effect_active {
                            1
                        } else {
                            -1
                        };
                        let matches_expected_delta =
                            args.effect_active_crit_delta.is_none_or(|expected| {
                                crit_delta == expected.saturating_mul(direction)
                            }) && args.effect_active_luck_delta.is_none_or(|expected| {
                                luck_delta == expected.saturating_mul(direction)
                            });
                        if confounded_effect_transition_examples.len() < args.example_limit
                            || matches_expected_delta
                        {
                            let example =
                                confounded_effect_transition_example(&key, previous, &sample);
                            if confounded_effect_transition_examples.len() < args.example_limit {
                                confounded_effect_transition_examples.push(example.clone());
                            }
                            if matches_expected_delta {
                                confounded_expected_delta_pairs =
                                    confounded_expected_delta_pairs.saturating_add(1);
                                if confounded_expected_delta_examples
                                    .iter()
                                    .filter(|existing| {
                                        existing.critical == key.critical
                                            && existing.lucky == key.lucky
                                    })
                                    .count()
                                    < args.example_limit
                                {
                                    confounded_expected_delta_examples.push(example);
                                }
                            }
                        }
                        break;
                    }
                    strict_candidate_pairs = strict_candidate_pairs.saturating_add(1);
                    if previous.crit_damage_raw == sample.crit_damage_raw
                        && previous.lucky_damage_raw == sample.lucky_damage_raw
                    {
                        fully_attribute_stable_candidate_pairs =
                            fully_attribute_stable_candidate_pairs.saturating_add(1);
                    } else {
                        attribute_adjusted_candidate_pairs =
                            attribute_adjusted_candidate_pairs.saturating_add(1);
                    }
                    observe_pair(args, &key, previous, &sample, groups, formulas);
                    break;
                }
                samples.push_back(sample);
                while samples.len() > RECENT_PER_KEY {
                    samples.pop_front();
                }
            }
            _ => {}
        }
    }

    Ok(SessionSummary {
        rlog: path.display().to_string(),
        session_id: session_id.unwrap_or_else(|| "unobserved".to_owned()),
        run_ordinals_observed: maximum_run_ordinal,
        damage_events,
        damage_events_with_relevant_multiplier_flag: relevant_damage_events,
        damage_events_with_both_attributes_known,
        selected_status_events,
        effect_transition_candidates_before_status_control,
        strict_candidate_pairs,
        fully_attribute_stable_candidate_pairs,
        attribute_adjusted_candidate_pairs,
        confounded_effect_transition_pairs,
        source_status_confounders: effect_count_reports(source_status_confounders),
        target_status_confounders: effect_count_reports(target_status_confounders),
        confounded_effect_transition_examples,
        confounded_expected_delta_pairs,
        confounded_expected_delta_examples,
        damage_events_eligible_for_hit_flag_comparison: hit_flag_eligible_damage_events,
        strict_hit_flag_pairs,
    })
}

fn clear_actor_attribute_snapshot(
    attributes: &mut HashMap<(u32, i64, i32), i64>,
    run_ordinal: u32,
    entity_uuid: i64,
) {
    attributes.retain(|(run, entity, _), _| *run != run_ordinal || *entity != entity_uuid);
}

static EMPTY_STATUS_TRACKER: StatusTracker = StatusTracker {
    active: BTreeMap::new(),
};

fn semantic_status_diff(
    first: &[SemanticStatusEntry],
    second: &[SemanticStatusEntry],
) -> (Vec<SemanticStatusCount>, Vec<SemanticStatusCount>) {
    let mut first_counts = BTreeMap::<SemanticStatusEntry, u32>::new();
    let mut second_counts = BTreeMap::<SemanticStatusEntry, u32>::new();
    for status in first {
        *first_counts.entry(status.clone()).or_default() += 1;
    }
    for status in second {
        *second_counts.entry(status.clone()).or_default() += 1;
    }
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for status in first_counts
        .keys()
        .chain(second_counts.keys())
        .collect::<BTreeSet<_>>()
    {
        let first_count = first_counts.get(status).copied().unwrap_or_default();
        let second_count = second_counts.get(status).copied().unwrap_or_default();
        if first_count > second_count {
            removed.push(SemanticStatusCount {
                status: (*status).clone(),
                count: first_count - second_count,
            });
        } else if second_count > first_count {
            added.push(SemanticStatusCount {
                status: (*status).clone(),
                count: second_count - first_count,
            });
        }
    }
    (removed, added)
}

fn record_status_confounders<'a>(
    totals: &mut BTreeMap<i64, u64>,
    changes: impl Iterator<Item = &'a SemanticStatusCount>,
) {
    for change in changes {
        let total = totals.entry(change.status.effect_id).or_default();
        *total = total.saturating_add(u64::from(change.count));
    }
}

fn effect_count_reports(totals: BTreeMap<i64, u64>) -> Vec<EffectCountReport> {
    let mut reports = totals
        .into_iter()
        .map(|(effect_id, count)| EffectCountReport { effect_id, count })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.effect_id.cmp(&right.effect_id))
    });
    reports
}

fn confounded_effect_transition_example(
    key: &DamageKey,
    first: &DamageSample,
    second: &DamageSample,
) -> ConfoundedEffectTransitionExample {
    let (source_status_removed, source_status_added) = semantic_status_diff(
        &first.source_status_snapshot,
        &second.source_status_snapshot,
    );
    let (target_status_removed, target_status_added) = semantic_status_diff(
        &first.target_status_snapshot,
        &second.target_status_snapshot,
    );
    ConfoundedEffectTransitionExample {
        first_sequence: first.sequence,
        second_sequence: second.sequence,
        gap_micros: second.observed_micros.saturating_sub(first.observed_micros),
        source_entity_uuid: key.source_entity_uuid,
        target_entity_uuid: key.target_entity_uuid,
        ability_id: key.ability_id,
        critical: key.critical,
        lucky: key.lucky,
        first_effect_active: first.effect_active,
        second_effect_active: second.effect_active,
        first_effect_stacks: first.effect_stacks,
        second_effect_stacks: second.effect_stacks,
        first_amount: first.amount,
        second_amount: second.amount,
        first_crit_damage_raw: first.crit_damage_raw,
        second_crit_damage_raw: second.crit_damage_raw,
        first_lucky_damage_raw: first.lucky_damage_raw,
        second_lucky_damage_raw: second.lucky_damage_raw,
        source_status_removed,
        source_status_added,
        target_status_removed,
        target_status_added,
    }
}

fn observe_pair(
    args: &Arguments,
    key: &DamageKey,
    first: &DamageSample,
    second: &DamageSample,
    groups: &mut BTreeMap<PairGroupKey, PairGroupAccumulator>,
    formulas: &mut BTreeMap<&'static str, CandidateFormulaAccumulator>,
) {
    let mut residuals = BTreeMap::new();
    for formula in candidate_formula_names(args.selected_effect_bonus_raw) {
        if let Some(residual) = formula_residual(
            formula,
            key,
            first,
            second,
            args.selected_effect_bonus_raw,
            args.compare_selected_effect_stacks,
        ) {
            residuals.insert(formula, residual);
            let accumulator = formulas.entry(formula).or_default();
            accumulator.evaluable_pairs = accumulator.evaluable_pairs.saturating_add(1);
            let absolute = u128::from(residual.unsigned_abs());
            accumulator.absolute_residual_sum =
                accumulator.absolute_residual_sum.saturating_add(absolute);
            accumulator.maximum_absolute_residual =
                accumulator.maximum_absolute_residual.max(absolute);
            if residual == 0 {
                accumulator.exact_matches = accumulator.exact_matches.saturating_add(1);
            } else if absolute <= 1 {
                accumulator.within_one_matches = accumulator.within_one_matches.saturating_add(1);
            } else {
                accumulator.mismatches = accumulator.mismatches.saturating_add(1);
            }
            if accumulator.residual_examples.len() < args.example_limit {
                accumulator.residual_examples.insert(residual);
            }
        }
    }
    let crit_raw_delta = second.crit_damage_raw.saturating_sub(first.crit_damage_raw);
    let luck_raw_delta = second
        .lucky_damage_raw
        .saturating_sub(first.lucky_damage_raw);
    let effect_transition = if args.compare_selected_effect_stacks
        && second.effect_stacks.unwrap_or_default() > first.effect_stacks.unwrap_or_default()
    {
        "stack_increase"
    } else if args.compare_selected_effect_stacks {
        "stack_decrease"
    } else if !first.effect_active && second.effect_active {
        "inactive_to_active"
    } else {
        "active_to_inactive"
    };
    let example = PairExample {
        rlog: first.rlog.clone(),
        session_id: first.session_id.clone(),
        run_ordinal: key.run_ordinal,
        source_entity_uuid: key.source_entity_uuid,
        direct_source_entity_uuid: key.direct_source_entity_uuid,
        target_entity_uuid: key.target_entity_uuid,
        ability_id: key.ability_id,
        hit_event_id: key.hit_event_id,
        damage_source: key.damage_source,
        damage_type: key.damage_type,
        missed: key.missed,
        reported_critical: key.reported_critical,
        type_flags: key.type_flags,
        owner_level: key.owner_level,
        owner_stage: key.owner_stage,
        normal_hit: key.normal_hit,
        property: key.property,
        hit_part_ids: key.hit_part_ids.clone(),
        first_position_bits: first.position_bits,
        second_position_bits: second.position_bits,
        first_hit_part_context: first.hit_part_context.clone(),
        second_hit_part_context: second.hit_part_context.clone(),
        damage_weight_bits: key.damage_weight_bits,
        passive_uuid: key.passive_uuid,
        rainbow: key.rainbow,
        damage_mode: key.damage_mode,
        skill_effect_component: key.skill_effect_component,
        first_skill_effect_total_damage: first.skill_effect_total_damage,
        second_skill_effect_total_damage: second.skill_effect_total_damage,
        context_attribute_values: key.context_attribute_values.clone(),
        diagnostic_attribute_transitions: diagnostic_attribute_transitions(
            &first.diagnostic_attribute_values,
            &second.diagnostic_attribute_values,
        ),
        critical: key.critical,
        lucky: key.lucky,
        first_sequence: first.sequence,
        second_sequence: second.sequence,
        gap_micros: second.observed_micros.saturating_sub(first.observed_micros),
        first_effect_active: first.effect_active,
        second_effect_active: second.effect_active,
        first_effect_stacks: first.effect_stacks,
        second_effect_stacks: second.effect_stacks,
        first_amount: first.amount,
        second_amount: second.amount,
        first_actual_amount: first.actual_amount,
        second_actual_amount: second.actual_amount,
        first_hp_loss: first.hp_loss,
        second_hp_loss: second.hp_loss,
        first_shield_loss: first.shield_loss,
        second_shield_loss: second.shield_loss,
        first_normal_value: first.normal_value,
        second_normal_value: second.normal_value,
        first_lucky_value: first.lucky_value,
        second_lucky_value: second.lucky_value,
        first_reported_critical: first.reported_critical,
        second_reported_critical: second.reported_critical,
        first_type_flags: first.type_flags,
        second_type_flags: second.type_flags,
        first_normal_hit: first.normal_hit,
        second_normal_hit: second.normal_hit,
        first_property: first.property,
        second_property: second.property,
        first_passive_uuid: first.passive_uuid,
        second_passive_uuid: second.passive_uuid,
        first_rainbow: first.rainbow,
        second_rainbow: second.rainbow,
        first_damage_mode: first.damage_mode,
        second_damage_mode: second.damage_mode,
        first_crit_damage_raw: first.crit_damage_raw,
        second_crit_damage_raw: second.crit_damage_raw,
        first_lucky_damage_raw: first.lucky_damage_raw,
        second_lucky_damage_raw: second.lucky_damage_raw,
        crit_raw_delta,
        luck_raw_delta,
        source_status_fingerprint: format!("{:016x}", first.source_status_fingerprint),
        target_status_fingerprint: format!("{:016x}", first.target_status_fingerprint),
        source_statuses: first.source_status_snapshot.clone(),
        target_statuses: first.target_status_snapshot.clone(),
        formula_residuals: residuals,
    };
    let accumulator = groups
        .entry(PairGroupKey {
            critical: key.critical,
            lucky: key.lucky,
            crit_raw_delta,
            luck_raw_delta,
            effect_transition,
            first_effect_stacks: first.effect_stacks,
            second_effect_stacks: second.effect_stacks,
        })
        .or_default();
    accumulator.count = accumulator.count.saturating_add(1);
    if accumulator.examples.len() < args.example_limit {
        accumulator.examples.push(example);
    }
}

fn diagnostic_attribute_transitions(
    first: &[(i32, Option<i64>)],
    second: &[(i32, Option<i64>)],
) -> Vec<AttributeTransitionReport> {
    let second_by_id = second.iter().copied().collect::<BTreeMap<_, _>>();
    first
        .iter()
        .map(|(attribute_id, first_value)| {
            let second_value = second_by_id.get(attribute_id).copied().flatten();
            AttributeTransitionReport {
                attribute_id: *attribute_id,
                first_value: *first_value,
                second_value,
                delta: (*first_value)
                    .zip(second_value)
                    .map(|(first, second)| second.saturating_sub(first)),
            }
        })
        .collect()
}

fn observe_hit_flag_pair(
    args: &Arguments,
    key: &HitFlagContextKey,
    first: &HitFlagSample,
    second: &HitFlagSample,
    groups: &mut BTreeMap<HitFlagPairGroupKey, HitFlagPairGroupAccumulator>,
    formulas: &mut BTreeMap<&'static str, CandidateFormulaAccumulator>,
) {
    let mut residuals = BTreeMap::new();
    for formula in hit_flag_candidate_formula_names() {
        if let Some(residual) = hit_flag_formula_residual(formula, key, first, second) {
            residuals.insert(formula, residual);
            observe_formula_residual(formulas.entry(formula).or_default(), residual, args);
        }
    }
    let first_flags = hit_flags_name(first.critical, first.lucky);
    let second_flags = hit_flags_name(second.critical, second.lucky);
    let example = HitFlagPairExample {
        rlog: first.rlog.clone(),
        session_id: first.session_id.clone(),
        run_ordinal: key.run_ordinal,
        source_entity_uuid: key.source_entity_uuid,
        direct_source_entity_uuid: key.direct_source_entity_uuid,
        target_entity_uuid: key.target_entity_uuid,
        ability_id: key.ability_id,
        hit_event_id: key.hit_event_id,
        damage_source: key.damage_source,
        damage_type: key.damage_type,
        first_sequence: first.sequence,
        second_sequence: second.sequence,
        gap_micros: second.observed_micros.saturating_sub(first.observed_micros),
        first_flags,
        second_flags,
        first_amount: first.amount,
        second_amount: second.amount,
        first_actual_amount: first.actual_amount,
        second_actual_amount: second.actual_amount,
        first_normal_value: first.normal_value,
        second_normal_value: second.normal_value,
        first_lucky_value: first.lucky_value,
        second_lucky_value: second.lucky_value,
        crit_damage_raw: key.crit_damage_raw,
        lucky_damage_raw: key.lucky_damage_raw,
        formula_residuals: residuals,
    };
    let accumulator = groups
        .entry(HitFlagPairGroupKey {
            first_flags,
            second_flags,
            crit_damage_raw: key.crit_damage_raw,
            lucky_damage_raw: key.lucky_damage_raw,
        })
        .or_default();
    accumulator.count = accumulator.count.saturating_add(1);
    if accumulator.examples.len() < args.example_limit {
        accumulator.examples.push(example);
    }
}

fn observe_formula_residual(
    accumulator: &mut CandidateFormulaAccumulator,
    residual: i64,
    args: &Arguments,
) {
    accumulator.evaluable_pairs = accumulator.evaluable_pairs.saturating_add(1);
    let absolute = u128::from(residual.unsigned_abs());
    accumulator.absolute_residual_sum = accumulator.absolute_residual_sum.saturating_add(absolute);
    accumulator.maximum_absolute_residual = accumulator.maximum_absolute_residual.max(absolute);
    if residual == 0 {
        accumulator.exact_matches = accumulator.exact_matches.saturating_add(1);
    } else if absolute <= 1 {
        accumulator.within_one_matches = accumulator.within_one_matches.saturating_add(1);
    } else {
        accumulator.mismatches = accumulator.mismatches.saturating_add(1);
    }
    if accumulator.residual_examples.len() < args.example_limit {
        accumulator.residual_examples.insert(residual);
    }
}

fn formula_reports(
    formulas: BTreeMap<&'static str, CandidateFormulaAccumulator>,
) -> Vec<CandidateFormulaReport> {
    formulas
        .into_iter()
        .map(|(formula, value)| CandidateFormulaReport {
            formula,
            evaluable_pairs: value.evaluable_pairs,
            exact_matches: value.exact_matches,
            within_one_matches: value.within_one_matches,
            mismatches: value.mismatches,
            mean_absolute_residual: (value.evaluable_pairs > 0)
                .then_some(value.absolute_residual_sum as f64 / value.evaluable_pairs as f64),
            maximum_absolute_residual: value.maximum_absolute_residual,
            residual_examples: value.residual_examples.into_iter().collect(),
        })
        .collect()
}

fn hit_flags_name(critical: bool, lucky: bool) -> &'static str {
    match (critical, lucky) {
        (false, false) => "normal",
        (true, false) => "critical",
        (false, true) => "lucky",
        (true, true) => "critical_lucky",
    }
}

fn observe_hit_flag_sample(
    sample: &HitFlagSample,
    observations: &mut HitFlagObservationAccumulator,
) {
    let index = hit_flag_index(sample.critical, sample.lucky);
    observations.flag_counts[index] = observations.flag_counts[index].saturating_add(1);
    match sample.normal_hit {
        Some(true) => {
            observations.normal_hit_true_by_flag[index] =
                observations.normal_hit_true_by_flag[index].saturating_add(1);
        }
        Some(false) => {
            observations.normal_hit_false_by_flag[index] =
                observations.normal_hit_false_by_flag[index].saturating_add(1);
        }
        None => {
            observations.normal_hit_absent_by_flag[index] =
                observations.normal_hit_absent_by_flag[index].saturating_add(1);
        }
    }
    if sample.normal_value.is_some() {
        observations.normal_value_present = observations.normal_value_present.saturating_add(1);
    }
    if sample.lucky_value.is_some() {
        observations.lucky_value_present = observations.lucky_value_present.saturating_add(1);
    }
    match (sample.normal_value, sample.lucky_value) {
        (Some(normal), Some(lucky)) => {
            observations.both_packet_values_present =
                observations.both_packet_values_present.saturating_add(1);
            if sample.amount == normal {
                observations.amount_matches_normal_value =
                    observations.amount_matches_normal_value.saturating_add(1);
            }
            if sample.amount == lucky {
                observations.amount_matches_lucky_value =
                    observations.amount_matches_lucky_value.saturating_add(1);
            }
            if sample.amount != normal && sample.amount != lucky {
                observations.amount_matches_neither_packet_value = observations
                    .amount_matches_neither_packet_value
                    .saturating_add(1);
            }
        }
        (Some(normal), None) => {
            if sample.amount == normal {
                observations.amount_matches_normal_value =
                    observations.amount_matches_normal_value.saturating_add(1);
            } else {
                observations.amount_matches_neither_packet_value = observations
                    .amount_matches_neither_packet_value
                    .saturating_add(1);
            }
        }
        (None, Some(lucky)) => {
            if sample.amount == lucky {
                observations.amount_matches_lucky_value =
                    observations.amount_matches_lucky_value.saturating_add(1);
            } else {
                observations.amount_matches_neither_packet_value = observations
                    .amount_matches_neither_packet_value
                    .saturating_add(1);
            }
        }
        (None, None) => {
            observations.neither_packet_value_present =
                observations.neither_packet_value_present.saturating_add(1);
            observations.amount_matches_neither_packet_value = observations
                .amount_matches_neither_packet_value
                .saturating_add(1);
        }
    }
}

fn hit_flag_observation_report(
    observations: &HitFlagObservationAccumulator,
) -> HitFlagObservationReport {
    HitFlagObservationReport {
        normal_events: observations.flag_counts[hit_flag_index(false, false)],
        lucky_events: observations.flag_counts[hit_flag_index(false, true)],
        critical_events: observations.flag_counts[hit_flag_index(true, false)],
        critical_lucky_events: observations.flag_counts[hit_flag_index(true, true)],
        normal_value_present: observations.normal_value_present,
        lucky_value_present: observations.lucky_value_present,
        both_packet_values_present: observations.both_packet_values_present,
        neither_packet_value_present: observations.neither_packet_value_present,
        amount_matches_normal_value: observations.amount_matches_normal_value,
        amount_matches_lucky_value: observations.amount_matches_lucky_value,
        amount_matches_neither_packet_value: observations.amount_matches_neither_packet_value,
    }
}

fn packet_normal_hit_outcome_reports(
    observations: &HitFlagObservationAccumulator,
) -> Vec<PacketNormalHitOutcomeReport> {
    [(false, false), (false, true), (true, false), (true, true)]
        .into_iter()
        .map(|(critical, lucky)| {
            let index = hit_flag_index(critical, lucky);
            PacketNormalHitOutcomeReport {
                flags: hit_flags_name(critical, lucky),
                packet_normal_hit_true: observations.normal_hit_true_by_flag[index],
                packet_normal_hit_false: observations.normal_hit_false_by_flag[index],
                packet_normal_hit_absent: observations.normal_hit_absent_by_flag[index],
            }
        })
        .collect()
}

fn observe_packet_fields(
    damage: &rlogs_events::DamageEvent,
    critical: bool,
    lucky: bool,
    observations: &mut HitFlagObservationAccumulator,
) {
    let index = hit_flag_index(critical, lucky);
    let packet = &damage.packet;
    let hit_part_ids = packet
        .hit_parts
        .iter()
        .map(|part| part.part_id)
        .collect::<Vec<_>>();
    let damage_weight_bits = packet
        .damage_weight
        .map(|weight| (weight.x.map(f32::to_bits), weight.y.map(f32::to_bits)));
    for (field, value) in [
        (
            "reported_critical",
            format!("{:?}", packet.reported_critical),
        ),
        ("type_flags", format!("{:?}", packet.type_flags)),
        ("owner_level", format!("{:?}", packet.owner_level)),
        ("owner_stage", format!("{:?}", packet.owner_stage)),
        ("normal_hit", format!("{:?}", packet.normal_hit)),
        ("property", format!("{:?}", packet.property)),
        ("hit_part_ids", format!("{hit_part_ids:?}")),
        ("damage_weight_bits", format!("{damage_weight_bits:?}")),
        ("passive_uuid", format!("{:?}", packet.passive_uuid)),
        ("rainbow", format!("{:?}", packet.rainbow)),
        ("damage_mode", format!("{:?}", packet.damage_mode)),
        (
            "skill_effect_uuid",
            format!("{:?}", packet.skill_effect_uuid),
        ),
        (
            "skill_effect_group_index",
            format!("{:?}", packet.skill_effect_group_index),
        ),
        (
            "skill_effect_component_index",
            format!("{:?}", packet.skill_effect_component_index),
        ),
        (
            "skill_effect_component_count",
            format!("{:?}", packet.skill_effect_component_count),
        ),
        (
            "skill_effect_total_damage",
            format!("{:?}", packet.skill_effect_total_damage),
        ),
    ] {
        let counts = observations
            .packet_field_counts
            .entry((field, value))
            .or_default();
        counts[index] = counts[index].saturating_add(1);
    }
}

fn packet_field_outcome_reports(
    observations: &HitFlagObservationAccumulator,
) -> Vec<PacketFieldOutcomeReport> {
    observations
        .packet_field_counts
        .iter()
        .map(|((field, value), counts)| PacketFieldOutcomeReport {
            field,
            value: value.clone(),
            normal: counts[hit_flag_index(false, false)],
            lucky: counts[hit_flag_index(false, true)],
            critical: counts[hit_flag_index(true, false)],
            critical_lucky: counts[hit_flag_index(true, true)],
        })
        .collect()
}

fn same_wire_damage_group_reports(
    groups: BTreeMap<SameWireDamageKey, Vec<SameWireDamageSample>>,
) -> Vec<SameWireDamageGroupReport> {
    groups
        .into_iter()
        .filter(|(_, samples)| {
            samples.iter().any(|sample| sample.normal_value.is_some())
                && samples.iter().any(|sample| sample.lucky_value.is_some())
        })
        .map(|(key, samples)| SameWireDamageGroupReport {
            rlog: key.rlog,
            session_id: key.session_id,
            run_ordinal: key.run_ordinal,
            capture_sequence: key.wire.capture_sequence,
            connection_id: key.wire.connection_id,
            stream_id: key.wire.stream_id,
            samples,
        })
        .collect()
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

fn selected_effect_transition_wires(
    path: &Path,
    effect_id: i64,
) -> Result<BTreeSet<(u32, WireMessageKey)>, Box<dyn std::error::Error>> {
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
            TimelineEventKind::Status(status) if status.effect.0 == effect_id => {
                if let Some(wire) = wire_message_key(&envelope.provenance.source) {
                    wires.insert((run_ordinal, wire));
                }
            }
            _ => {}
        }
    }
    Ok(wires)
}

fn hit_flag_diagnostic_tier_names() -> [&'static str; 13] {
    [
        "actor_ability_without_attributes",
        "actor_ability_with_attributes",
        "actor_target_ability",
        "actor_target_ability_direct_source",
        "actor_target_ability_hit_event",
        "actor_target_ability_damage_source",
        "actor_target_ability_damage_type",
        "actor_target_ability_blocked_periodic",
        "mechanic_identity_without_packet_or_statuses",
        "packet_identity_without_normal_hit_or_statuses",
        "packet_identity_without_statuses",
        "packet_identity_plus_source_statuses",
        "packet_identity_plus_source_and_target_statuses",
    ]
}

fn observe_hit_flag_diagnostics(
    session_id: &str,
    context: &HitFlagContextKey,
    critical: bool,
    lucky: bool,
    diagnostics: &mut BTreeMap<
        &'static str,
        BTreeMap<HitFlagDiagnosticKey, HitFlagDiagnosticContextAccumulator>,
    >,
) {
    for tier in hit_flag_diagnostic_tier_names() {
        let mut diagnostic_context = context.clone();
        match tier {
            "actor_ability_without_attributes" => {
                clear_mechanic_identity(&mut diagnostic_context);
                diagnostic_context.target_entity_uuid = 0;
                diagnostic_context.crit_damage_raw = 0;
                diagnostic_context.lucky_damage_raw = 0;
            }
            "actor_ability_with_attributes" => {
                clear_mechanic_identity(&mut diagnostic_context);
                diagnostic_context.target_entity_uuid = 0;
            }
            "actor_target_ability" => {
                clear_mechanic_identity(&mut diagnostic_context);
            }
            "actor_target_ability_direct_source" => {
                let direct_source = diagnostic_context.direct_source_entity_uuid;
                clear_mechanic_identity(&mut diagnostic_context);
                diagnostic_context.direct_source_entity_uuid = direct_source;
            }
            "actor_target_ability_hit_event" => {
                let hit_event_id = diagnostic_context.hit_event_id;
                clear_mechanic_identity(&mut diagnostic_context);
                diagnostic_context.hit_event_id = hit_event_id;
            }
            "actor_target_ability_damage_source" => {
                let damage_source = diagnostic_context.damage_source;
                clear_mechanic_identity(&mut diagnostic_context);
                diagnostic_context.damage_source = damage_source;
            }
            "actor_target_ability_damage_type" => {
                let damage_type = diagnostic_context.damage_type;
                clear_mechanic_identity(&mut diagnostic_context);
                diagnostic_context.damage_type = damage_type;
            }
            "actor_target_ability_blocked_periodic" => {
                let blocked = diagnostic_context.blocked;
                let periodic = diagnostic_context.periodic;
                clear_mechanic_identity(&mut diagnostic_context);
                diagnostic_context.blocked = blocked;
                diagnostic_context.periodic = periodic;
            }
            "mechanic_identity_without_packet_or_statuses" => {
                clear_packet_identity(&mut diagnostic_context);
                diagnostic_context.source_status_fingerprint = 0;
                diagnostic_context.target_status_fingerprint = 0;
            }
            "packet_identity_without_normal_hit_or_statuses" => {
                diagnostic_context.normal_hit = None;
                diagnostic_context.source_status_fingerprint = 0;
                diagnostic_context.target_status_fingerprint = 0;
            }
            "packet_identity_without_statuses" => {
                diagnostic_context.source_status_fingerprint = 0;
                diagnostic_context.target_status_fingerprint = 0;
            }
            "packet_identity_plus_source_statuses" => {
                diagnostic_context.target_status_fingerprint = 0;
            }
            "packet_identity_plus_source_and_target_statuses" => {}
            _ => unreachable!("diagnostic tier is declared above"),
        }
        let entry = diagnostics
            .get_mut(tier)
            .expect("diagnostic tier accumulator exists")
            .entry(HitFlagDiagnosticKey {
                session_id: session_id.to_owned(),
                context: diagnostic_context,
            })
            .or_default();
        entry.flag_counts[hit_flag_index(critical, lucky)] =
            entry.flag_counts[hit_flag_index(critical, lucky)].saturating_add(1);
    }
}

fn clear_mechanic_identity(context: &mut HitFlagContextKey) {
    context.direct_source_entity_uuid = None;
    context.hit_event_id = None;
    context.damage_source = None;
    context.damage_type = None;
    context.blocked = false;
    context.periodic = false;
    context.source_status_fingerprint = 0;
    context.target_status_fingerprint = 0;
    clear_packet_identity(context);
}

fn clear_packet_identity(context: &mut HitFlagContextKey) {
    context.owner_level = None;
    context.owner_stage = None;
    context.normal_hit = None;
    context.property = None;
    context.hit_part_ids.clear();
    context.damage_weight_bits = None;
    context.passive_uuid = None;
    context.rainbow = None;
    context.damage_mode = None;
}

fn hit_flag_index(critical: bool, lucky: bool) -> usize {
    (usize::from(critical) << 1) | usize::from(lucky)
}

fn hit_flag_diagnostic_reports(
    diagnostics: BTreeMap<
        &'static str,
        BTreeMap<HitFlagDiagnosticKey, HitFlagDiagnosticContextAccumulator>,
    >,
) -> Vec<HitFlagDiagnosticTierReport> {
    diagnostics
        .into_iter()
        .map(|(tier, contexts)| {
            let mut eligible_events = 0_u64;
            let mut contexts_with_multiple_flag_states = 0_u64;
            let mut cross_flag_pairs = 0_u64;
            let mut outcome_pair_counts = [[0_u64; 4]; 4];
            for context in contexts.values() {
                eligible_events = eligible_events
                    .saturating_add(context.flag_counts.iter().copied().sum::<u64>());
                if context
                    .flag_counts
                    .iter()
                    .filter(|count| **count > 0)
                    .count()
                    > 1
                {
                    contexts_with_multiple_flag_states =
                        contexts_with_multiple_flag_states.saturating_add(1);
                }
                for left in 0..context.flag_counts.len() {
                    for right in (left + 1)..context.flag_counts.len() {
                        cross_flag_pairs = cross_flag_pairs.saturating_add(
                            context.flag_counts[left].saturating_mul(context.flag_counts[right]),
                        );
                        outcome_pair_counts[left][right] = outcome_pair_counts[left][right]
                            .saturating_add(
                                context.flag_counts[left]
                                    .saturating_mul(context.flag_counts[right]),
                            );
                    }
                }
            }
            HitFlagDiagnosticTierReport {
                tier,
                eligible_events,
                distinct_contexts: u64::try_from(contexts.len()).unwrap_or(u64::MAX),
                contexts_with_multiple_flag_states,
                cross_flag_pairs,
                normal_lucky_pairs: outcome_pair_counts[hit_flag_index(false, false)]
                    [hit_flag_index(false, true)],
                normal_critical_pairs: outcome_pair_counts[hit_flag_index(false, false)]
                    [hit_flag_index(true, false)],
                normal_critical_lucky_pairs: outcome_pair_counts[hit_flag_index(false, false)]
                    [hit_flag_index(true, true)],
                lucky_critical_pairs: outcome_pair_counts[hit_flag_index(false, true)]
                    [hit_flag_index(true, false)],
                lucky_critical_lucky_pairs: outcome_pair_counts[hit_flag_index(false, true)]
                    [hit_flag_index(true, true)],
                critical_critical_lucky_pairs: outcome_pair_counts[hit_flag_index(true, false)]
                    [hit_flag_index(true, true)],
            }
        })
        .collect()
}

fn hit_flag_candidate_formula_names() -> [&'static str; 6] {
    [
        "product_crit_raw_luck_raw",
        "product_crit_raw_luck_plus_10000",
        "product_crit_plus_10000_luck_raw",
        "product_crit_plus_10000_luck_plus_10000",
        "additive_bonus_raw",
        "additive_total_raw",
    ]
}

fn hit_flag_formula_residual(
    formula: &'static str,
    key: &HitFlagContextKey,
    first: &HitFlagSample,
    second: &HitFlagSample,
) -> Option<i64> {
    let first_factor = hit_flag_factor(
        formula,
        key.crit_damage_raw,
        key.lucky_damage_raw,
        first.critical,
        first.lucky,
    )?;
    let second_factor = hit_flag_factor(
        formula,
        key.crit_damage_raw,
        key.lucky_damage_raw,
        second.critical,
        second.lucky,
    )?;
    if first_factor == 0 {
        return None;
    }
    let predicted_second = predict_ratio_amount(first.amount, second_factor, first_factor)?;
    i64::try_from(i128::from(second.amount).checked_sub(predicted_second)?).ok()
}

fn hit_flag_factor(
    formula: &'static str,
    crit_raw: i64,
    lucky_raw: i64,
    critical: bool,
    lucky: bool,
) -> Option<i128> {
    let crit_raw = i128::from(crit_raw);
    let lucky_raw = i128::from(lucky_raw);
    match formula {
        "product_crit_raw_luck_raw" => {
            hit_flag_product_factor(crit_raw, lucky_raw, critical, lucky, 0, 0)
        }
        "product_crit_raw_luck_plus_10000" => {
            hit_flag_product_factor(crit_raw, lucky_raw, critical, lucky, 0, PERCENT_SCALE)
        }
        "product_crit_plus_10000_luck_raw" => {
            hit_flag_product_factor(crit_raw, lucky_raw, critical, lucky, PERCENT_SCALE, 0)
        }
        "product_crit_plus_10000_luck_plus_10000" => hit_flag_product_factor(
            crit_raw,
            lucky_raw,
            critical,
            lucky,
            PERCENT_SCALE,
            PERCENT_SCALE,
        ),
        "additive_bonus_raw" => fixed_point_bonus_factor(
            if critical { crit_raw } else { 0 } + if lucky { lucky_raw } else { 0 },
        ),
        "additive_total_raw" => Some(
            PERCENT_SCALE
                + if critical {
                    crit_raw - PERCENT_SCALE
                } else {
                    0
                }
                + if lucky { lucky_raw - PERCENT_SCALE } else { 0 },
        ),
        _ => None,
    }
}

fn hit_flag_product_factor(
    crit_raw: i128,
    lucky_raw: i128,
    critical: bool,
    lucky: bool,
    crit_offset: i128,
    luck_offset: i128,
) -> Option<i128> {
    let crit = if critical {
        crit_raw.checked_add(crit_offset)?
    } else {
        PERCENT_SCALE
    };
    let luck = if lucky {
        lucky_raw.checked_add(luck_offset)?
    } else {
        PERCENT_SCALE
    };
    crit.checked_mul(luck)
}

fn candidate_formula_names(selected_effect_bonus_raw: Option<i64>) -> Vec<&'static str> {
    let mut formulas = vec![
        "crit_raw_x_luck_raw",
        "crit_raw_x_luck_plus_10000",
        "crit_plus_10000_x_luck_raw",
        "crit_plus_10000_x_luck_plus_10000",
    ];
    if selected_effect_bonus_raw.is_some() {
        formulas.extend([
            "crit_raw_x_luck_raw_x_selected_effect_final",
            "crit_raw_x_luck_plus_10000_x_selected_effect_final",
            "crit_plus_10000_x_luck_raw_x_selected_effect_final",
            "crit_plus_10000_x_luck_plus_10000_x_selected_effect_final",
        ]);
    }
    formulas
}

fn formula_residual(
    formula: &'static str,
    key: &DamageKey,
    first: &DamageSample,
    second: &DamageSample,
    selected_effect_bonus_raw: Option<i64>,
    compare_selected_effect_stacks: bool,
) -> Option<i64> {
    let selected_effect_final = formula.ends_with("_x_selected_effect_final");
    let base_formula = formula
        .strip_suffix("_x_selected_effect_final")
        .unwrap_or(formula);
    let (crit_offset, luck_offset) = match base_formula {
        "crit_raw_x_luck_raw" => (0_i128, 0_i128),
        "crit_raw_x_luck_plus_10000" => (0, PERCENT_SCALE),
        "crit_plus_10000_x_luck_raw" => (PERCENT_SCALE, 0),
        "crit_plus_10000_x_luck_plus_10000" => (PERCENT_SCALE, PERCENT_SCALE),
        _ => return None,
    };
    let first_factor = multiplier_factor(key, first, crit_offset, luck_offset)?;
    let second_factor = multiplier_factor(key, second, crit_offset, luck_offset)?;
    let (first_effect_factor, second_effect_factor) = if selected_effect_final {
        let bonus_raw = i128::from(selected_effect_bonus_raw?);
        (
            selected_effect_factor(
                first.effect_active,
                first.effect_stacks,
                bonus_raw,
                compare_selected_effect_stacks,
            )?,
            selected_effect_factor(
                second.effect_active,
                second.effect_stacks,
                bonus_raw,
                compare_selected_effect_stacks,
            )?,
        )
    } else {
        (PERCENT_SCALE, PERCENT_SCALE)
    };
    let numerator = second_factor.checked_mul(second_effect_factor)?;
    let denominator = first_factor.checked_mul(first_effect_factor)?;
    if denominator == 0 {
        return None;
    }
    let predicted_second = predict_ratio_amount(first.amount, numerator, denominator)?;
    let residual = i128::from(second.amount).checked_sub(predicted_second)?;
    i64::try_from(residual).ok()
}

fn selected_effect_factor(
    effect_active: bool,
    effect_stacks: Option<u32>,
    bonus_raw: i128,
    compare_selected_effect_stacks: bool,
) -> Option<i128> {
    let applications = if compare_selected_effect_stacks {
        i128::from(effect_stacks.unwrap_or_default())
    } else if effect_active {
        1
    } else {
        0
    };
    PERCENT_SCALE.checked_add(bonus_raw.checked_mul(applications)?)
}

fn multiplier_factor(
    key: &DamageKey,
    sample: &DamageSample,
    crit_offset: i128,
    luck_offset: i128,
) -> Option<i128> {
    let crit = if key.critical {
        i128::from(sample.crit_damage_raw).checked_add(crit_offset)?
    } else {
        PERCENT_SCALE
    };
    let luck = if key.lucky {
        i128::from(sample.lucky_damage_raw).checked_add(luck_offset)?
    } else {
        PERCENT_SCALE
    };
    crit.checked_mul(luck)
}

fn fixed_point_bonus_factor(bonus_raw: i128) -> Option<i128> {
    PERCENT_SCALE.checked_add(bonus_raw)
}

fn predict_ratio_amount(amount: i64, numerator: i128, denominator: i128) -> Option<i128> {
    (denominator != 0).then(|| {
        i128::from(amount)
            .checked_mul(numerator)?
            .checked_div(denominator)
    })?
}

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
    let decoded = attribute.decoded.clone().or_else(|| {
        decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value)
    });
    match decoded {
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

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let effect_id = parse_i64(take_value(&mut values, "--effect")?, "--effect")?;
    let crit_attribute_id = parse_i32(
        take_value(&mut values, "--crit-attribute")?,
        "--crit-attribute",
    )?;
    let luck_attribute_id = parse_i32(
        take_value(&mut values, "--luck-attribute")?,
        "--luck-attribute",
    )?;
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let max_gap_micros = take_optional_value(&mut values, "--max-gap-micros")
        .map(|value| parse_u64(value, "--max-gap-micros"))
        .transpose()?
        .unwrap_or(DEFAULT_MAX_GAP_MICROS);
    let example_limit = take_optional_value(&mut values, "--example-limit")
        .map(|value| parse_usize(value, "--example-limit"))
        .transpose()?
        .unwrap_or(DEFAULT_EXAMPLE_LIMIT);
    let effect_active_crit_delta = take_optional_value(&mut values, "--effect-active-crit-delta")
        .map(|value| parse_i64(value, "--effect-active-crit-delta"))
        .transpose()?;
    let effect_active_luck_delta = take_optional_value(&mut values, "--effect-active-luck-delta")
        .map(|value| parse_i64(value, "--effect-active-luck-delta"))
        .transpose()?;
    let selected_effect_bonus_raw = take_optional_value(&mut values, "--selected-effect-bonus-raw")
        .map(|value| parse_i64(value, "--selected-effect-bonus-raw"))
        .transpose()?;
    let ignore_packet_normal_hit_in_pair_key =
        take_switch(&mut values, "--ignore-packet-normal-hit-in-pair-key");
    let ignore_source_status_in_hit_flag_pair_key =
        take_switch(&mut values, "--ignore-source-status-in-hit-flag-pair-key");
    let ignore_target_status_in_hit_flag_pair_key =
        take_switch(&mut values, "--ignore-target-status-in-hit-flag-pair-key");
    let selected_effect_on_target = take_switch(&mut values, "--selected-effect-on-target");
    let selected_effect_provider_is_attacker =
        take_switch(&mut values, "--selected-effect-provider-is-attacker");
    let compare_selected_effect_stacks =
        take_switch(&mut values, "--compare-selected-effect-stacks");
    let mut ignored_status_effect_ids = BTreeSet::new();
    while let Some(value) = take_optional_value(&mut values, "--ignore-status-effect") {
        ignored_status_effect_ids.insert(parse_i64(value, "--ignore-status-effect")?);
    }
    let mut attacker_scoped_target_effect_ids = BTreeSet::new();
    while let Some(value) = take_optional_value(&mut values, "--attacker-scoped-target-effect") {
        attacker_scoped_target_effect_ids
            .insert(parse_i64(value, "--attacker-scoped-target-effect")?);
    }
    let mut context_attribute_ids = BTreeSet::new();
    while let Some(value) = take_optional_value(&mut values, "--context-attribute") {
        context_attribute_ids.insert(parse_i32(value, "--context-attribute")?);
    }
    let mut diagnostic_attribute_ids = BTreeSet::new();
    while let Some(value) = take_optional_value(&mut values, "--diagnostic-attribute") {
        diagnostic_attribute_ids.insert(parse_i32(value, "--diagnostic-attribute")?);
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
        effect_id,
        crit_attribute_id,
        luck_attribute_id,
        rlogs,
        output,
        max_gap_micros,
        example_limit,
        effect_active_crit_delta,
        effect_active_luck_delta,
        selected_effect_bonus_raw,
        ignore_packet_normal_hit_in_pair_key,
        ignore_source_status_in_hit_flag_pair_key,
        ignore_target_status_in_hit_flag_pair_key,
        selected_effect_on_target,
        selected_effect_provider_is_attacker,
        compare_selected_effect_stacks,
        ignored_status_effect_ids,
        attacker_scoped_target_effect_ids,
        context_attribute_ids,
        diagnostic_attribute_ids,
    })
}

fn parse_i64(value: OsString, flag: &str) -> Result<i64, String> {
    value
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| format!("{flag} requires an integer"))
}

fn parse_i32(value: OsString, flag: &str) -> Result<i32, String> {
    value
        .to_string_lossy()
        .parse::<i32>()
        .map_err(|_| format!("{flag} requires an integer"))
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

fn take_switch(values: &mut Vec<OsString>, flag: &str) -> bool {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return false;
    };
    values.remove(position);
    true
}

fn usage() -> String {
    "usage: rlogs-bpsr-damage-multiplier-proof --effect <status-id> --crit-attribute <id> --luck-attribute <id> --rlog <current-decoder.rlog> [--rlog <current-decoder.rlog> ...] --output <audit.json> [--max-gap-micros <micros>] [--example-limit <count>] [--effect-active-crit-delta <raw>] [--effect-active-luck-delta <raw>] [--selected-effect-bonus-raw <10000-scale>] [--selected-effect-on-target] [--selected-effect-provider-is-attacker] [--compare-selected-effect-stacks] [--ignore-status-effect <proven-non-damage-id>]... [--attacker-scoped-target-effect <self-only-effect-id>]... [--context-attribute <source-attribute-id>]... [--diagnostic-attribute <source-attribute-id>]... [--ignore-packet-normal-hit-in-pair-key] [--ignore-source-status-in-hit-flag-pair-key] [--ignore-target-status-in-hit-flag-pair-key]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        AttributeTransitionReport, EffectCountReport, ExactFractionBucketAccumulator,
        ExactFractionBucketKey, PERCENT_SCALE, SkillEffectComponentIdentity, StatusKey,
        StatusTracker, StatusValue, aggregate_effect_counts, clear_actor_attribute_snapshot,
        diagnostic_attribute_transitions, exact_fraction_bucket_report, exact_observed_share,
        fixed_point_bonus_factor, formula_placement_status, inverse_floor_interval,
        predict_ratio_amount, selected_effect_factor, single_counterfactual_candidate,
    };
    use rlogs_events::StatusState;
    use std::collections::HashMap;

    #[test]
    fn attribute_snapshot_clears_only_the_matching_run_actor_state() {
        let mut attributes = HashMap::from([
            ((2, 100, 12550), 600),
            ((2, 100, 12510), 11_558),
            ((2, 200, 12550), 900),
            ((1, 100, 12550), 300),
        ]);

        clear_actor_attribute_snapshot(&mut attributes, 2, 100);

        assert_eq!(attributes.len(), 2);
        assert_eq!(attributes.get(&(2, 200, 12550)), Some(&900));
        assert_eq!(attributes.get(&(1, 100, 12550)), Some(&300));
    }

    #[test]
    fn diagnostic_attributes_report_transitions_without_becoming_pair_identity() {
        assert_eq!(
            diagnostic_attribute_transitions(
                &[(12511, Some(20_626)), (12512, None)],
                &[(12511, Some(20_401)), (12512, Some(20_401))],
            ),
            vec![
                AttributeTransitionReport {
                    attribute_id: 12511,
                    first_value: Some(20_626),
                    second_value: Some(20_401),
                    delta: Some(-225),
                },
                AttributeTransitionReport {
                    attribute_id: 12512,
                    first_value: None,
                    second_value: Some(20_401),
                    delta: None,
                },
            ]
        );
    }

    #[test]
    fn skill_effect_component_identity_rejects_different_component_shapes() {
        let two_component_hit = SkillEffectComponentIdentity {
            skill_effect_uuid: Some(9_001),
            skill_effect_group_index: Some(22),
            skill_effect_component_index: Some(0),
            skill_effect_component_count: Some(2),
        };
        let one_component_hit = SkillEffectComponentIdentity {
            skill_effect_uuid: Some(9_001),
            skill_effect_group_index: Some(18),
            skill_effect_component_index: Some(0),
            skill_effect_component_count: Some(1),
        };

        assert_ne!(two_component_hit, one_component_hit);
    }

    #[test]
    fn explicit_empty_attribute_payload_is_zero() {
        assert_eq!(super::decode_varint(&[]), Some(0));
    }

    #[test]
    fn formula_placement_status_preserves_each_proof_boundary() {
        assert_eq!(
            formula_placement_status(0, 0, false),
            "status-controlled-effect-toggle-pair-unavailable"
        );
        assert_eq!(
            formula_placement_status(1, 0, false),
            "status-controlled-toggle-observed-with-attribute-change-but-exact-formula-unresolved"
        );
        assert_eq!(
            formula_placement_status(1, 1, false),
            "fully-controlled-toggle-observed-but-exact-formula-unresolved"
        );
        assert_eq!(
            formula_placement_status(1, 0, true),
            "exact-candidate-observed-pending-conservation-and-closure-review"
        );
    }

    #[test]
    fn selected_effect_factor_uses_boolean_or_exact_stack_count() {
        assert_eq!(
            selected_effect_factor(false, None, 1_000, false),
            Some(10_000)
        );
        assert_eq!(
            selected_effect_factor(true, None, 1_000, false),
            Some(11_000)
        );
        assert_eq!(
            selected_effect_factor(true, Some(3), 500, true),
            Some(11_500)
        );
    }

    #[test]
    fn effect_confounders_are_aggregated_and_ranked_deterministically() {
        let first = [
            EffectCountReport {
                effect_id: 30,
                count: 2,
            },
            EffectCountReport {
                effect_id: 10,
                count: 4,
            },
        ];
        let second = [
            EffectCountReport {
                effect_id: 30,
                count: 3,
            },
            EffectCountReport {
                effect_id: 20,
                count: 5,
            },
        ];

        let report = aggregate_effect_counts(first.iter().chain(second.iter()));

        assert_eq!(report.len(), 3);
        assert_eq!((report[0].effect_id, report[0].count), (20, 5));
        assert_eq!((report[1].effect_id, report[1].count), (30, 5));
        assert_eq!((report[2].effect_id, report[2].count), (10, 4));
    }

    #[test]
    fn consumed_status_only_closes_when_remaining_stack_count_is_zero() {
        let key = StatusKey {
            effect_id: 2_300_621,
            instance_id: Some(17),
            source_entity_uuid: Some(216_009_015_936),
        };
        let mut tracker = StatusTracker::default();
        tracker.observe(
            key.clone(),
            StatusValue {
                stacks: Some(4),
                level: Some(1),
                part_id: None,
                count: None,
            },
            StatusState::Consumed,
        );
        assert!(tracker.contains_effect(key.effect_id));

        tracker.observe(
            key.clone(),
            StatusValue {
                stacks: Some(0),
                level: Some(1),
                part_id: None,
                count: None,
            },
            StatusState::Consumed,
        );
        assert!(!tracker.contains_effect(key.effect_id));
    }

    #[test]
    fn exact_effect_stacks_filters_by_resolved_provider_without_inventing_a_sum() {
        let effect_id = 2_300_621;
        let direct_provider = 42;
        let resolved_provider = 7;
        let other_provider = 9;
        let owner_by_direct_entity = HashMap::from([(direct_provider, resolved_provider)]);
        let mut tracker = StatusTracker::default();
        for (instance_id, source_entity_uuid, stacks) in
            [(1, direct_provider, 3), (2, other_provider, 4)]
        {
            tracker.observe(
                StatusKey {
                    effect_id,
                    instance_id: Some(instance_id),
                    source_entity_uuid: Some(source_entity_uuid),
                },
                StatusValue {
                    stacks: Some(stacks),
                    level: Some(1),
                    part_id: None,
                    count: None,
                },
                StatusState::Applied,
            );
        }

        assert_eq!(
            tracker.exact_effect_stacks(
                effect_id,
                Some(resolved_provider),
                &owner_by_direct_entity,
            ),
            Some(3)
        );
        assert_eq!(
            tracker.exact_effect_stacks(effect_id, Some(other_provider), &owner_by_direct_entity,),
            Some(4)
        );
        assert_eq!(
            tracker.exact_effect_stacks(effect_id, None, &owner_by_direct_entity),
            None,
            "two packet-observed providers are not an authoritatively summable stack"
        );
        assert_eq!(
            tracker.exact_effect_stacks(effect_id, Some(99), &owner_by_direct_entity),
            Some(0)
        );
    }

    #[test]
    fn exact_effect_stacks_rejects_multiple_instances_from_the_same_provider() {
        let effect_id = 2_300_621;
        let provider = 7;
        let mut tracker = StatusTracker::default();
        for instance_id in [1, 2] {
            tracker.observe(
                StatusKey {
                    effect_id,
                    instance_id: Some(instance_id),
                    source_entity_uuid: Some(provider),
                },
                StatusValue {
                    stacks: Some(2),
                    level: Some(1),
                    part_id: None,
                    count: None,
                },
                StatusState::Applied,
            );
        }

        assert_eq!(
            tracker.exact_effect_stacks(effect_id, Some(provider), &HashMap::new()),
            None
        );
    }

    #[test]
    fn raw_bonus_is_added_to_the_one_point_zero_base() {
        assert_eq!(fixed_point_bonus_factor(520), Some(10_520));
        assert_eq!(fixed_point_bonus_factor(340), Some(10_340));
        assert_eq!(fixed_point_bonus_factor(310), Some(10_310));
        assert_eq!(fixed_point_bonus_factor(200), Some(10_200));
        assert_eq!(PERCENT_SCALE, 10_000);
    }

    #[test]
    fn critical_pair_uses_bonus_points_not_relative_stat_growth() {
        // Packet pair: a 67,428 critical hit and 30,568 normal hit at raw 12,058.
        // Reversing the multiplier floors exactly to the observed normal amount.
        assert_eq!(
            predict_ratio_amount(67_428, 10_000, 10_000 + 12_058),
            Some(30_568)
        );

        // A relative-stat interpretation would first multiply 12,058 by 1.052,
        // producing 12,685 instead of the packet-observed +520-point change.
        assert_ne!(12_058_i128 * 10_520 / 10_000, 12_058 + 520);
    }

    #[test]
    fn inverse_floor_interval_retains_every_integer_preimage() {
        assert_eq!(
            inverse_floor_interval(1_052, 10_520, 10_000),
            Some((1_000, 1_000))
        );
        assert_eq!(inverse_floor_interval(1, 4_555, 10_000), Some((3, 4)));
        assert_eq!(inverse_floor_interval(-1, 10_520, 10_000), None);
        assert_eq!(inverse_floor_interval(1, 0, 10_000), None);
    }

    #[test]
    fn exact_counterfactual_requires_a_single_output_across_the_floor_interval() {
        let candidate = single_counterfactual_candidate("test", 1_052, 10_520, 10_000)
            .expect("valid positive factors");
        assert_eq!(
            (candidate.latent_base_min, candidate.latent_base_max),
            (1_000, 1_000)
        );
        assert_eq!(
            (candidate.counterfactual_min, candidate.counterfactual_max),
            (1_000, 1_000)
        );
    }

    #[test]
    fn exact_fraction_keeps_the_one_point_integer_interval_without_losing_conservation() {
        let share = exact_observed_share(7_671, 4_555, 4_215)
            .expect("positive packet-proven factors produce an exact fraction");
        assert_eq!(share.numerator, "521628");
        assert_eq!(share.denominator, "911");
        assert_eq!(share.floor, "572");
        assert_eq!(share.ceil, "573");
    }

    #[test]
    fn provider_and_recipient_fraction_buckets_sum_to_observed_damage_exactly() {
        let report = exact_fraction_bucket_report(
            ExactFractionBucketKey {
                current_critical_factor: 10_520,
                current_lucky_factor: 10_000,
                provider_removed_critical_factor: 10_000,
                provider_removed_lucky_factor: 10_000,
            },
            ExactFractionBucketAccumulator {
                event_count: 2,
                observed_damage_sum: 101_000,
            },
        );
        assert!(report.conservation_identity_holds);
        assert_eq!(report.provider_attribution.numerator, "1313000");
        assert_eq!(report.provider_attribution.denominator, "263");
        assert_eq!(report.recipient_retained.numerator, "25250000");
        assert_eq!(report.recipient_retained.denominator, "263");
    }
}
