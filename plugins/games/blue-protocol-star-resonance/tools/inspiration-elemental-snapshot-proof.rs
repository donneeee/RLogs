use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 14;
const ATTACK_ATTRIBUTE_ID: i32 = 11_330;
const MAGIC_ATTACK_ATTRIBUTE_ID: i32 = 11_340;
const CRITICAL_DAMAGE_ATTRIBUTE_ID: i32 = 12_510;
const LUCKY_DAMAGE_ATTRIBUTE_ID: i32 = 12_530;
const LIGHT_DAMAGE_ATTRIBUTE_ID: i32 = 13_170;
const EXTERNAL_DAMAGE_ATTRIBUTE_ID: i32 = 11_840;
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const INSPIRATION_EFFECT_ID: i64 = 2_202_041;
const STEEL_BEAK_STACK_EFFECT_ID: i64 = 2_203_521;
const MASTERY_ATTRIBUTE_ID: i32 = 11_940;
const INSPIRATION_COMPOSITE_MODELS: &[&str] = &[
    "attack_coefficient_plus_fixed",
    "attack_hit_outcome",
    "external_then_light",
    "light_then_external",
    "external_plus_light_single_bucket",
    "external_light_product_single_floor",
];
const SCOPED_INSPIRATION_MODELS: &[&str] = &[
    "attack_coefficient_plus_fixed",
    "attack_hit_outcome",
    "external_then_serialized_light",
    "serialized_light_then_external",
    "external_plus_serialized_light_single_bucket",
    "external_serialized_light_product_single_floor",
    "external_then_logical_light_from_mastery_delta",
    "logical_light_from_mastery_delta_then_external",
    "external_plus_logical_light_single_bucket",
    "external_logical_light_product_single_floor",
];
const INSPIRATION_DIRECT_ATTRIBUTE_IDS: &[i32] = &[
    11_010, 11_011, 11_012, 11_013, 11_014, 11_015, 11_020, 11_021, 11_022, 11_023, 11_024, 11_025,
    11_030, 11_031, 11_032, 11_033, 11_034, 11_035, 11_040, 11_041, 11_042, 11_043, 11_044, 11_045,
    11_330, 11_331, 11_332, 11_333, 11_334, 11_335, 11_710, 11_711, 11_712, 11_713, 11_714, 11_715,
    11_780, 11_781, 11_782, 11_783, 11_784, 11_785, 11_840, 11_930, 11_931, 11_932, 11_933, 11_934,
    11_935, 11_940, 11_941, 11_942, 11_943, 11_944, 11_945, 11_950, 11_951, 11_952, 11_953, 11_954,
    11_955,
];
const INSPIRATION_DERIVED_ATTRIBUTE_IDS: &[i32] = &[
    // Packet-observed downstream families recalculated from Inspiration's primary-stat,
    // Haste, Mastery, and Versatility inputs. These are comparison controls only: they
    // do not grant credit and cannot become formula authority without their own proof.
    11_120, 11_121, 11_122, 11_123, 11_124, 11_125, 11_320, 11_321, 11_322, 11_323, 11_324, 11_325,
    11_350, 11_351, 11_352, 11_353, 11_354, 11_355, 11_360, 11_361, 11_362, 11_363, 11_364, 11_365,
    11_720, 11_721, 11_722, 11_723, 11_724, 11_725, 11_730, 11_731, 11_732, 11_733, 11_734, 11_735,
    11_841, 11_842, 11_843, 11_844, 11_845, 11_850, 11_851, 11_852, 11_853, 11_854, 11_855, 12_510,
    12_511, 12_512, 12_513, 12_514, 12_515, 12_530, 12_531, 12_532, 12_533, 12_534, 12_535, 12_720,
    12_721, 12_722, 12_723, 12_724, 12_725, 13_170, 13_171, 13_172, 13_173, 13_174, 13_175,
];
const SOURCE_IGNORED_ATTRIBUTE_IDS: &[i32] = &[
    CURRENT_HP_ATTRIBUTE_ID,
    LIGHT_DAMAGE_ATTRIBUTE_ID,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 1,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 2,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 3,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 4,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 5,
];
const OCCURRENCE_CONTROL_SOURCE_IGNORED_ATTRIBUTE_IDS: &[i32] = &[
    CURRENT_HP_ATTRIBUTE_ID,
    11_120,
    11_121,
    11_122,
    11_123,
    11_124,
    11_125,
    11_320,
    11_321,
    11_322,
    11_323,
    11_324,
    11_325,
    11_350,
    11_351,
    11_352,
    11_353,
    11_354,
    11_355,
    11_360,
    11_361,
    11_362,
    11_363,
    11_364,
    11_365,
    11_720,
    11_721,
    11_722,
    11_723,
    11_724,
    11_725,
    11_730,
    11_731,
    11_732,
    11_733,
    11_734,
    11_735,
    11_840,
    11_841,
    11_842,
    11_843,
    11_844,
    11_845,
    11_850,
    11_851,
    11_852,
    11_853,
    11_854,
    11_855,
    12_510,
    12_511,
    12_512,
    12_513,
    12_514,
    12_515,
    12_530,
    12_531,
    12_532,
    12_533,
    12_534,
    12_535,
    12_720,
    12_721,
    12_722,
    12_723,
    12_724,
    12_725,
    LIGHT_DAMAGE_ATTRIBUTE_ID,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 1,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 2,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 3,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 4,
    LIGHT_DAMAGE_ATTRIBUTE_ID + 5,
];

#[derive(Debug)]
struct Arguments {
    cohort: PathBuf,
    gap_proof: PathBuf,
    damage_surface: PathBuf,
    output: PathBuf,
    example_limit: usize,
}

#[derive(Debug, Deserialize)]
struct FormulaCohort {
    attribute_states: Vec<Vec<AttributeEntry>>,
    status_states: Vec<Vec<StatusEntry>>,
    samples: Vec<FormulaSample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct AttributeEntry {
    attribute_id: i32,
    value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct StatusEntry {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct FormulaSample {
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: i64,
    passive_uuid: Option<i64>,
    hit_event_id: Option<i32>,
    amount: i64,
    normal_value: Option<i64>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    packet: PacketFields,
    source_attribute_state_id: usize,
    target_attribute_state_id: usize,
    source_status_state_id: usize,
    target_status_state_id: usize,
}

#[derive(Debug, Deserialize)]
struct PacketFields {
    #[serde(default)]
    attacker_uuid: Option<i64>,
    #[serde(default)]
    top_summoner_uuid: Option<i64>,
    #[serde(default)]
    owner_id: Option<i32>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    #[serde(default)]
    damage_weight: Option<PacketPosition>,
    #[serde(default)]
    hit_parts: Vec<PacketHitPart>,
    skill_effect_uuid: Option<i64>,
    skill_effect_total_damage: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct PacketPosition {
    x: Option<f32>,
    y: Option<f32>,
    z: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
struct PacketHitPart {
    part_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct GapProof {
    sessions: Vec<GapSession>,
}

#[derive(Debug, Deserialize)]
struct GapSession {
    session_id: String,
    #[serde(default)]
    transition_boundaries: BTreeMap<u64, u64>,
    #[serde(default)]
    exact_examples: Vec<GapTransition>,
    gap_light_damage_examples: Vec<GapSample>,
}

#[derive(Debug, Clone, Deserialize)]
struct GapTransition {
    mastery_sequence: u64,
    light_sequence: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct GapSample {
    sequence: u64,
    mastery_sequence: u64,
    mastery_before: i64,
    mastery_after: i64,
    serialized_light: Option<i64>,
    logical_light: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
struct CalculationIdentity {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: i64,
    passive_uuid: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    packet_passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    damage_weight_bits: Option<(Option<u32>, Option<u32>, Option<u32>)>,
    hit_part_ids: Vec<Option<i32>>,
}

#[derive(Debug, Default, Serialize)]
struct Counters {
    gap_examples: u64,
    gap_examples_present_in_cohort: u64,
    gap_examples_with_logical_light: u64,
    stable_controls_with_same_calculation_identity: u64,
    stable_controls_with_logical_light: u64,
    controls_with_source_attributes_equal_except_light_and_hp: u64,
    controls_with_target_attributes_equal_except_hp: u64,
    controls_with_both_attribute_states_equal: u64,
    controls_rejected_only_by_source_status: u64,
    controls_rejected_only_by_target_status: u64,
    controls_rejected_by_both_status_states: u64,
    strict_state_control_pairs: u64,
    strict_state_equal_damage_pairs: u64,
    strict_state_divergent_damage_pairs: u64,
    strict_state_gap_examples_with_any_pair: u64,
}

#[derive(Debug, Default, Serialize)]
struct AbilityCounters {
    gap_examples_present_in_cohort: u64,
    strict_state_control_pairs: u64,
    strict_state_equal_damage_pairs: u64,
    strict_state_divergent_damage_pairs: u64,
}

#[derive(Debug, Serialize)]
struct PairExample {
    session_id: String,
    run_ordinal: u32,
    ability_id: i64,
    hit_event_id: Option<i32>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    gap_sequence: u64,
    stable_sequence: u64,
    gap_mastery_sequence: u64,
    gap_mastery_before: i64,
    gap_mastery_after: i64,
    serialized_light: i64,
    logical_light: i64,
    stable_light: i64,
    gap_amount: i64,
    stable_amount: i64,
    gap_normal_value: Option<i64>,
    stable_normal_value: Option<i64>,
    equal_damage: bool,
    gap_to_stable_micros: i64,
}

#[derive(Debug, Serialize)]
struct Audit {
    schema_version: u16,
    generated_by: &'static str,
    policy: Policy,
    inputs: Inputs,
    counters: Counters,
    by_ability: BTreeMap<i64, AbilityCounters>,
    equal_examples: Vec<PairExample>,
    divergent_examples: Vec<PairExample>,
    nearest_examples: Vec<NearestExample>,
    occurrence_control_diagnostic: OccurrenceControlDiagnostic,
    status_state_isolation_diagnostic: StatusStateIsolationDiagnostic,
    target_status_stack_isolation_diagnostic: TargetStatusStackIsolationDiagnostic,
    current_hp_isolation_diagnostic: CurrentHpIsolationDiagnostic,
    serialized_input_outcome_multiplicity_diagnostic: SerializedInputOutcomeMultiplicityDiagnostic,
    attribute_stage_isolation_diagnostic: AttributeStageIsolationDiagnostic,
    inspiration_composite_transition_diagnostic: InspirationCompositeTransitionDiagnostic,
    action_snapshot_lag_diagnostic: ActionSnapshotLagDiagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SerializedInputOutcomeMultiplicityKey {
    identity: CalculationIdentity,
    source_attribute_state_id: usize,
    target_attribute_state_id: usize,
    source_status_state_id: usize,
    target_status_state_id: usize,
}

#[derive(Debug, Default)]
struct SerializedInputOutcomeMultiplicityBucket {
    sequences: BTreeSet<u64>,
    outcomes: BTreeMap<StatusOutcome, u64>,
}

#[derive(Debug, Default, Serialize)]
struct SerializedInputOutcomeMultiplicityCounters {
    exact_serialized_input_groups: u64,
    groups_with_multiple_occurrences: u64,
    groups_with_multiple_distinct_outcomes: u64,
    repeated_occurrences: u64,
    repeated_occurrences_in_multi_outcome_groups: u64,
}

#[derive(Debug, Default, Serialize)]
struct SerializedInputOutcomeMultiplicityAbilityCounters {
    groups_with_multiple_occurrences: u64,
    groups_with_multiple_distinct_outcomes: u64,
    repeated_occurrences: u64,
}

#[derive(Debug, Serialize)]
struct SerializedInputOutcomeMultiplicityExample {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    source_attribute_state_id: usize,
    target_attribute_state_id: usize,
    source_status_state_id: usize,
    target_status_state_id: usize,
    sequences: Vec<u64>,
    outcomes: BTreeMap<StatusOutcome, u64>,
}

#[derive(Debug, Serialize)]
struct SerializedInputOutcomeMultiplicityDiagnostic {
    runtime_authority: bool,
    exact_input_control: &'static str,
    interpretation_boundary: &'static str,
    counters: SerializedInputOutcomeMultiplicityCounters,
    by_ability_hit: BTreeMap<String, SerializedInputOutcomeMultiplicityAbilityCounters>,
    multi_outcome_examples: Vec<SerializedInputOutcomeMultiplicityExample>,
}

#[derive(Debug, Deserialize)]
struct DamageCorrelationSurface {
    observed_keys: Vec<DamageCorrelationKey>,
}

#[derive(Debug, Deserialize)]
struct DamageCorrelationKey {
    ability_id: i64,
    hit_event_id: i32,
    match_status: String,
    unique_row: Option<DamageCorrelationRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct DamageCorrelationRow {
    damage_id: String,
    damage_script: String,
    pve_damage_ratio: Vec<i64>,
    pve_fixed_parameter: Vec<i64>,
}

#[derive(Debug, Default, Serialize)]
struct InspirationCompositeCounters {
    eligible_light_property_samples: u64,
    eligible_samples_with_inspiration_status: u64,
    eligible_samples_without_inspiration_status: u64,
    samples_with_unique_standard_damage_row: u64,
    samples_with_exact_stage_coefficient_and_fixed_parameter: u64,
    controlled_groups_with_multiple_vector_states: u64,
    controlled_groups_with_only_inactive_states: u64,
    controlled_groups_with_only_active_states: u64,
    controlled_groups_with_active_and_inactive_states: u64,
    active_inactive_vector_state_pairs: u64,
    deterministic_active_inactive_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
}

#[derive(Debug, Default, Serialize)]
struct InspirationCompositeModelCounters {
    evaluated_pairs: u64,
    compatible_pairs: u64,
    rejected_pairs: u64,
}

#[derive(Debug, Serialize)]
struct InspirationCompositeTransitionDiagnostic {
    policy: InspirationCompositePolicy,
    counters: InspirationCompositeCounters,
    models: BTreeMap<String, InspirationCompositeModelCounters>,
    compatible_examples: Vec<InspirationCompositeExample>,
    rejected_examples: Vec<InspirationCompositeExample>,
    nearest_active_inactive_mismatches: InspirationNearestMismatchDiagnostic,
}

#[derive(Debug, Serialize)]
struct InspirationCompositePolicy {
    runtime_authority: bool,
    calculation_control: &'static str,
    source_attribute_control: &'static str,
    target_attribute_control: &'static str,
    status_control: &'static str,
    damage_surface_control: &'static str,
    tested_stage_models: Vec<&'static str>,
    interpretation_boundary: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InspirationCompositeState {
    selected_statuses: Vec<StatusEntry>,
    vector_attributes: Vec<AttributeEntry>,
    coefficient: i64,
    fixed_parameter: i64,
    damage_id: String,
    damage_script: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InspirationCompositeControlKey {
    identity: CalculationIdentity,
    source_attributes_without_inspiration: Vec<AttributeEntry>,
    target_attributes_without_current_hp: Vec<AttributeEntry>,
    source_statuses_without_inspiration: Vec<StatusEntry>,
    target_statuses: Vec<StatusEntry>,
}

#[derive(Debug, Default)]
struct InspirationCompositeBucket {
    occurrences: u64,
    outcomes: BTreeMap<StatusOutcome, u64>,
    sequences: BTreeSet<u64>,
    source_current_hp: BTreeSet<Option<i64>>,
    target_current_hp: BTreeSet<Option<i64>>,
}

#[derive(Debug, Clone, Serialize)]
struct InspirationCompositeExample {
    model: String,
    calculation_identity: CalculationIdentity,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    inactive_sequences: Vec<u64>,
    active_sequences: Vec<u64>,
    damage_id: String,
    damage_script: String,
    coefficient: i64,
    fixed_parameter: i64,
    inactive_vector_attributes: Vec<AttributeEntry>,
    active_vector_attributes: Vec<AttributeEntry>,
    inactive_outcome: StatusOutcome,
    active_outcome: StatusOutcome,
    inactive_body: Option<i64>,
    active_body: Option<i64>,
    inactive_later_factor_minimum: Option<i64>,
    inactive_later_factor_maximum: Option<i64>,
    active_later_factor_minimum: Option<i64>,
    active_later_factor_maximum: Option<i64>,
    compatible_later_factor_minimum: Option<i64>,
    compatible_later_factor_maximum: Option<i64>,
    inactive_source_current_hp: Vec<Option<i64>>,
    active_source_current_hp: Vec<Option<i64>>,
    inactive_target_current_hp: Vec<Option<i64>>,
    active_target_current_hp: Vec<Option<i64>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct InspirationLooseState {
    selected_statuses: Vec<StatusEntry>,
    vector_attributes: Vec<AttributeEntry>,
    source_attributes_without_inspiration: Vec<AttributeEntry>,
    target_attributes_without_current_hp: Vec<AttributeEntry>,
    source_statuses_without_inspiration: Vec<StatusEntry>,
    target_statuses: Vec<StatusEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InspirationLooseControlKey {
    identity: CalculationIdentity,
    coefficient: i64,
    fixed_parameter: i64,
    damage_id: String,
    damage_script: String,
}

#[derive(Debug, Default)]
struct InspirationLooseBucket {
    occurrences: u64,
    outcomes: BTreeMap<StatusOutcome, u64>,
    sequences: BTreeSet<u64>,
    source_current_hp: BTreeSet<Option<i64>>,
    target_current_hp: BTreeSet<Option<i64>>,
}

#[derive(Debug, Default, Serialize)]
struct InspirationNearestMismatchCounters {
    loose_control_groups: u64,
    loose_control_groups_with_active_and_inactive_states: u64,
    nearest_pairs: u64,
    nearest_pairs_without_unexpected_differences: u64,
    nearest_pairs_with_scoped_target_stack_invariance: u64,
    scoped_pairs_with_deterministic_outcomes: u64,
    scoped_pairs_with_source_current_hp_difference: u64,
    scoped_pairs_with_target_current_hp_difference: u64,
}

#[derive(Debug, Serialize)]
struct InspirationNearestMismatchDiagnostic {
    runtime_authority: bool,
    comparison_control: &'static str,
    interpretation_boundary: &'static str,
    counters: InspirationNearestMismatchCounters,
    mismatch_score_histogram: BTreeMap<usize, u64>,
    source_attribute_mismatches: Vec<InspirationAttributeMismatchCount>,
    target_attribute_mismatches: Vec<InspirationAttributeMismatchCount>,
    source_status_mismatches: Vec<InspirationStatusMismatchCount>,
    target_status_mismatches: Vec<InspirationStatusMismatchCount>,
    scoped_target_stack_models: BTreeMap<String, InspirationCompositeModelCounters>,
    scoped_compatible_examples: Vec<InspirationCompositeExample>,
    scoped_rejected_examples: Vec<InspirationCompositeExample>,
    examples: Vec<InspirationNearestMismatchExample>,
}

#[derive(Debug, Serialize)]
struct InspirationAttributeMismatchCount {
    attribute_id: i32,
    nearest_group_count: u64,
}

#[derive(Debug, Serialize)]
struct InspirationStatusMismatchCount {
    direction: &'static str,
    status: StatusEntry,
    nearest_group_count: u64,
}

#[derive(Debug, Serialize)]
struct InspirationAttributeChange {
    attribute_id: i32,
    inactive_value: Option<i64>,
    active_value: Option<i64>,
}

#[derive(Debug, Serialize)]
struct InspirationNearestMismatchExample {
    calculation_identity: CalculationIdentity,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    damage_id: String,
    damage_script: String,
    coefficient: i64,
    fixed_parameter: i64,
    mismatch_score: usize,
    inactive_sequences: Vec<u64>,
    active_sequences: Vec<u64>,
    inactive_vector_attributes: Vec<AttributeEntry>,
    active_vector_attributes: Vec<AttributeEntry>,
    inactive_control_source_attributes: Vec<AttributeEntry>,
    active_control_source_attributes: Vec<AttributeEntry>,
    inactive_control_target_attributes: Vec<AttributeEntry>,
    active_control_target_attributes: Vec<AttributeEntry>,
    inactive_occurrences: u64,
    active_occurrences: u64,
    inactive_outcomes: Vec<StatusOutcome>,
    active_outcomes: Vec<StatusOutcome>,
    inactive_source_current_hp: Vec<Option<i64>>,
    active_source_current_hp: Vec<Option<i64>>,
    inactive_target_current_hp: Vec<Option<i64>>,
    active_target_current_hp: Vec<Option<i64>>,
    scoped_target_stack_invariance: bool,
    source_current_hp_differs: bool,
    target_current_hp_differs: bool,
    logical_active_light_from_mastery_delta: Option<i64>,
    source_attribute_changes: Vec<InspirationAttributeChange>,
    target_attribute_changes: Vec<InspirationAttributeChange>,
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
}

#[derive(Debug, Default, Serialize)]
struct AttributeStageIsolationCounters {
    controlled_groups: u64,
    distinct_axis_state_pairs: u64,
    deterministic_state_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    exact_independent_final_stage_pairs: u64,
    rejected_independent_final_stage_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
}

#[derive(Debug, Serialize)]
struct AttributeStageIsolationAxis {
    current_attribute_id: i32,
    family_attribute_ids: Vec<i32>,
    counters: AttributeStageIsolationCounters,
    exact_examples: Vec<AttributeStageIsolationExample>,
    rejected_examples: Vec<AttributeStageIsolationExample>,
}

#[derive(Debug, Serialize)]
struct AttributeStageIsolationDiagnostic {
    policy: AttributeStageIsolationPolicy,
    axes: BTreeMap<String, AttributeStageIsolationAxis>,
}

#[derive(Debug, Serialize)]
struct AttributeStageIsolationPolicy {
    runtime_authority: bool,
    calculation_control: &'static str,
    state_control: &'static str,
    exact_model_tested: &'static str,
    rejection_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct AttributeStageIsolationExample {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    left_axis_state: Vec<AttributeEntry>,
    right_axis_state: Vec<AttributeEntry>,
    left_source_current_hp: Vec<Option<i64>>,
    right_source_current_hp: Vec<Option<i64>>,
    left_target_current_hp: Vec<Option<i64>>,
    right_target_current_hp: Vec<Option<i64>>,
    left_outcomes: Vec<StatusOutcome>,
    right_outcomes: Vec<StatusOutcome>,
    compatible_pre_factor_minimum: Option<String>,
    compatible_pre_factor_maximum: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct StatusStateIsolationCounters {
    current_hp_relaxed_attribute_groups: u64,
    status_state_pairs: u64,
    single_effect_difference_pairs: u64,
    deterministic_equal_output_pairs: u64,
    deterministic_divergent_output_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
    exact_scoped_transition_candidates: u64,
    exact_scoped_neutral_transition_candidates: u64,
}

#[derive(Debug, Default, Serialize)]
struct StatusStateIsolationEffectCounters {
    status_state_pairs: u64,
    candidate_occurrence_pairs: u64,
    deterministic_equal_output_pairs: u64,
    deterministic_divergent_output_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
    abilities: BTreeSet<i64>,
}

#[derive(Debug, Serialize)]
struct StatusStateIsolationExample {
    effect_id: i64,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    left_source_status_state_id: usize,
    left_target_status_state_id: usize,
    right_source_status_state_id: usize,
    right_target_status_state_id: usize,
    left_source_current_hp: Vec<Option<i64>>,
    right_source_current_hp: Vec<Option<i64>>,
    left_target_current_hp: Vec<Option<i64>>,
    right_target_current_hp: Vec<Option<i64>>,
    left_occurrences: u64,
    right_occurrences: u64,
    left_outcomes: Vec<StatusOutcome>,
    right_outcomes: Vec<StatusOutcome>,
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct StatusOutcome {
    amount: i64,
    normal_value: Option<i64>,
}

#[derive(Debug, Default)]
struct StatusOutcomeBucket {
    occurrences: u64,
    outcomes: BTreeMap<StatusOutcome, u64>,
    source_current_hp: BTreeSet<Option<i64>>,
    target_current_hp: BTreeSet<Option<i64>>,
}

#[derive(Debug, Serialize)]
struct StatusStateIsolationDiagnostic {
    policy: StatusStateIsolationPolicy,
    counters: StatusStateIsolationCounters,
    by_effect: BTreeMap<i64, StatusStateIsolationEffectCounters>,
    exact_scoped_transition_evidence: Vec<ScopedStatusTransitionEvidence>,
    deterministic_equal_examples: Vec<StatusStateIsolationExample>,
    deterministic_divergent_examples: Vec<StatusStateIsolationExample>,
}

struct StatusStateIsolationAnalysis {
    diagnostic: StatusStateIsolationDiagnostic,
    scoped_neutral_keys: BTreeSet<ScopedStatusTransitionKey>,
}

#[derive(Debug, Serialize)]
struct StatusStateIsolationPolicy {
    runtime_authority: bool,
    attribute_control: &'static str,
    status_control: &'static str,
    deterministic_equal_interpretation: &'static str,
    promotion_boundary: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ScopedStatusTransitionKey {
    calculation_identity: CalculationIdentity,
    effect_id: i64,
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
}

#[derive(Debug, Default)]
struct ScopedStatusTransitionAccumulator {
    status_state_pairs: u64,
    candidate_occurrence_pairs: u64,
    deterministic_equal_output_pairs: u64,
    deterministic_divergent_output_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
}

#[derive(Debug, Serialize)]
struct ScopedStatusTransitionEvidence {
    calculation_identity: CalculationIdentity,
    effect_id: i64,
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
    status_state_pairs: u64,
    candidate_occurrence_pairs: u64,
    deterministic_equal_output_pairs: u64,
    deterministic_divergent_output_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
    scoped_neutral_control_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TargetStatusStackControlKey {
    identity: CalculationIdentity,
    source_attributes_without_current_hp: Vec<AttributeEntry>,
    target_attributes_without_current_hp: Vec<AttributeEntry>,
    source_statuses: Vec<StatusEntry>,
    target_statuses_without_selected_effect: Vec<StatusEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TargetStatusStackState {
    selected_target_statuses: Vec<StatusEntry>,
}

#[derive(Debug, Default)]
struct TargetStatusStackBucket {
    occurrences: u64,
    outcomes: BTreeMap<StatusOutcome, u64>,
    sequences: BTreeSet<u64>,
    source_current_hp: BTreeSet<Option<i64>>,
    target_current_hp: BTreeSet<Option<i64>>,
}

#[derive(Debug, Default, Serialize)]
struct TargetStatusStackIsolationCounters {
    selected_effect_id: i64,
    samples: u64,
    controlled_groups: u64,
    controlled_groups_with_multiple_stack_states: u64,
    stack_state_pairs: u64,
    deterministic_equal_output_pairs: u64,
    deterministic_divergent_output_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
}

#[derive(Debug, Default, Serialize)]
struct TargetStatusStackAbilityCounters {
    stack_state_pairs: u64,
    deterministic_equal_output_pairs: u64,
    deterministic_divergent_output_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
}

#[derive(Debug, Serialize)]
struct TargetStatusStackIsolationExample {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    left_selected_target_statuses: Vec<StatusEntry>,
    right_selected_target_statuses: Vec<StatusEntry>,
    left_sequences: Vec<u64>,
    right_sequences: Vec<u64>,
    left_source_current_hp: Vec<Option<i64>>,
    right_source_current_hp: Vec<Option<i64>>,
    left_target_current_hp: Vec<Option<i64>>,
    right_target_current_hp: Vec<Option<i64>>,
    left_occurrences: u64,
    right_occurrences: u64,
    left_outcomes: Vec<StatusOutcome>,
    right_outcomes: Vec<StatusOutcome>,
}

#[derive(Debug, Serialize)]
struct TargetStatusStackIsolationDiagnostic {
    policy: TargetStatusStackIsolationPolicy,
    counters: TargetStatusStackIsolationCounters,
    by_ability_hit: BTreeMap<String, TargetStatusStackAbilityCounters>,
    deterministic_equal_examples: Vec<TargetStatusStackIsolationExample>,
    deterministic_divergent_examples: Vec<TargetStatusStackIsolationExample>,
}

#[derive(Debug, Serialize)]
struct TargetStatusStackIsolationPolicy {
    runtime_authority: bool,
    selected_effect_id: i64,
    calculation_control: &'static str,
    attribute_control: &'static str,
    status_control: &'static str,
    interpretation_boundary: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CurrentHpIsolationControlKey {
    axis: &'static str,
    identity: CalculationIdentity,
    source_attributes_without_current_hp: Vec<AttributeEntry>,
    target_attributes_without_current_hp: Vec<AttributeEntry>,
    source_statuses: Vec<StatusEntry>,
    target_statuses: Vec<StatusEntry>,
    opposite_current_hp: Option<i64>,
}

#[derive(Debug, Default)]
struct CurrentHpIsolationBucket {
    occurrences: u64,
    outcomes: BTreeMap<StatusOutcome, u64>,
    sequences: BTreeSet<u64>,
}

#[derive(Debug, Default, Serialize)]
struct CurrentHpIsolationCounters {
    controlled_groups: u64,
    controlled_groups_with_multiple_hp_states: u64,
    hp_state_pairs: u64,
    deterministic_equal_output_pairs: u64,
    deterministic_divergent_output_pairs: u64,
    nondeterministic_or_partially_overlapping_pairs: u64,
}

#[derive(Debug, Default, Serialize)]
struct CurrentHpIsolationAbilityCounters {
    source_hp_state_pairs: u64,
    source_hp_deterministic_equal_output_pairs: u64,
    source_hp_deterministic_divergent_output_pairs: u64,
    source_hp_nondeterministic_or_partially_overlapping_pairs: u64,
    target_hp_state_pairs: u64,
    target_hp_deterministic_equal_output_pairs: u64,
    target_hp_deterministic_divergent_output_pairs: u64,
    target_hp_nondeterministic_or_partially_overlapping_pairs: u64,
}

#[derive(Debug, Serialize)]
struct CurrentHpIsolationExample {
    axis: &'static str,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    left_current_hp: Option<i64>,
    right_current_hp: Option<i64>,
    opposite_current_hp: Option<i64>,
    left_sequences: Vec<u64>,
    right_sequences: Vec<u64>,
    left_occurrences: u64,
    right_occurrences: u64,
    left_outcomes: Vec<StatusOutcome>,
    right_outcomes: Vec<StatusOutcome>,
}

#[derive(Debug, Serialize)]
struct CurrentHpIsolationDiagnostic {
    policy: CurrentHpIsolationPolicy,
    counters: CurrentHpIsolationCounters,
    by_ability_hit: BTreeMap<String, CurrentHpIsolationAbilityCounters>,
    deterministic_equal_examples: Vec<CurrentHpIsolationExample>,
    deterministic_divergent_examples: Vec<CurrentHpIsolationExample>,
}

#[derive(Debug, Serialize)]
struct CurrentHpIsolationPolicy {
    runtime_authority: bool,
    calculation_control: &'static str,
    state_control: &'static str,
    deterministic_equal_interpretation: &'static str,
    promotion_boundary: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct ActionSnapshotLagCounters {
    gap_events: u64,
    gap_events_with_transition_boundary: u64,
    gap_events_with_prior_old_light_observation: u64,
    gap_events_with_next_logical_light_observation: u64,
    gap_events_with_both_boundary_observations: u64,
    prior_observation_amount_equal: u64,
    next_observation_amount_equal: u64,
    both_observations_amount_equal: u64,
    only_prior_observation_amount_equal: u64,
    only_next_observation_amount_equal: u64,
    neither_observation_amount_equal: u64,
    prior_observation_state_controlled: u64,
    next_observation_state_controlled: u64,
    both_observations_state_controlled: u64,
    controlled_both_match_prior_only: u64,
    controlled_both_match_next_only: u64,
    controlled_both_match_both: u64,
    controlled_both_match_neither: u64,
}

#[derive(Debug, Default, Serialize)]
struct ActionSnapshotLagAbilityCounters {
    gap_events: u64,
    gap_events_with_both_boundary_observations: u64,
    both_observations_amount_equal: u64,
    only_prior_observation_amount_equal: u64,
    only_next_observation_amount_equal: u64,
    neither_observation_amount_equal: u64,
    both_observations_state_controlled: u64,
}

#[derive(Debug, Serialize)]
struct ActionSnapshotLagDiagnostic {
    policy: ActionSnapshotLagPolicy,
    counters: ActionSnapshotLagCounters,
    by_ability: BTreeMap<i64, ActionSnapshotLagAbilityCounters>,
    examples: Vec<ActionSnapshotLagExample>,
}

#[derive(Debug, Serialize)]
struct ActionSnapshotLagPolicy {
    runtime_authority: bool,
    prior_boundary: &'static str,
    next_boundary: &'static str,
    state_control: &'static str,
    inspiration_direct_attribute_ids: Vec<i32>,
    packet_observed_downstream_attribute_ids: Vec<i32>,
    component_identity: &'static str,
    interpretation_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ActionSnapshotLagExample {
    session_id: String,
    run_ordinal: u32,
    ability_id: i64,
    hit_event_id: Option<i32>,
    gap_sequence: u64,
    mastery_sequence: u64,
    light_sequence: u64,
    serialized_light: i64,
    logical_light: i64,
    gap_amount: i64,
    skill_effect_uuid: Option<i64>,
    skill_effect_total_damage: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    prior: Option<ActionSnapshotBoundaryObservation>,
    next: Option<ActionSnapshotBoundaryObservation>,
}

#[derive(Debug, Serialize)]
struct ActionSnapshotBoundaryObservation {
    sequence: u64,
    observed_micros_delta: i64,
    light: Option<i64>,
    amount: i64,
    normal_value: Option<i64>,
    amount_equal_to_gap: bool,
    state_controlled: bool,
    source_attribute_differences: Vec<AttributeDifference>,
    target_attribute_differences: Vec<AttributeDifference>,
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
}

#[derive(Debug, Default, Serialize)]
struct OccurrenceControlCounters {
    candidate_pairs: u64,
    complete_attribute_control_pairs: u64,
    complete_source_status_control_pairs: u64,
    complete_target_status_control_pairs: u64,
    complete_status_control_pairs: u64,
    scoped_neutral_status_control_pairs: u64,
    status_mismatch_equal_amount_pairs: u64,
    status_mismatch_divergent_amount_pairs: u64,
    status_mismatch_pairs_with_both_normal_values: u64,
    status_mismatch_equal_normal_value_pairs: u64,
    status_mismatch_divergent_normal_value_pairs: u64,
    status_mismatch_pairs_missing_any_normal_value: u64,
    gap_examples_with_equal_amount_witness: u64,
    equal_amount_pairs_by_light_transition: BTreeMap<String, u64>,
    divergent_amount_pairs_by_light_transition: BTreeMap<String, u64>,
    unsupported_combined_critical_lucky_pairs: u64,
    missing_multiplier_attribute_pairs: u64,
    rejected_shortcut_interval_overlap_pairs: u64,
    rejected_shortcut_interval_disjoint_pairs: u64,
    gap_examples_with_any_rejected_shortcut_pair: u64,
}

#[derive(Debug, Default, Serialize)]
struct OccurrenceControlAbilityCounters {
    rejected_shortcut_control_pairs: u64,
    rejected_shortcut_interval_overlap_pairs: u64,
    rejected_shortcut_interval_disjoint_pairs: u64,
    scoped_neutral_status_control_pairs: u64,
    status_mismatch_equal_amount_pairs: u64,
    status_mismatch_divergent_amount_pairs: u64,
    status_mismatch_pairs_with_both_normal_values: u64,
    status_mismatch_equal_normal_value_pairs: u64,
    status_mismatch_divergent_normal_value_pairs: u64,
    status_mismatch_pairs_missing_any_normal_value: u64,
    equal_amount_pairs_by_light_transition: BTreeMap<String, u64>,
    divergent_amount_pairs_by_light_transition: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize)]
struct OccurrenceControlDiagnostic {
    policy: OccurrenceControlPolicy,
    counters: OccurrenceControlCounters,
    by_ability: BTreeMap<i64, OccurrenceControlAbilityCounters>,
    rejected_shortcut_overlap_examples: Vec<RejectedShortcutPairExample>,
    rejected_shortcut_disjoint_examples: Vec<RejectedShortcutPairExample>,
    nearest_status_mismatch_examples: Vec<StatusMismatchExample>,
    status_difference_aggregates: Vec<StatusDifferenceAggregate>,
}

#[derive(Debug, Serialize)]
struct OccurrenceControlPolicy {
    runtime_authority: bool,
    source_attribute_control: &'static str,
    scoped_status_control: &'static str,
    rejected_shortcut: &'static str,
    retained_value: &'static str,
    interpretation_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct RejectedShortcutPairExample {
    session_id: String,
    run_ordinal: u32,
    ability_id: i64,
    gap_sequence: u64,
    stable_sequence: u64,
    serialized_light: i64,
    logical_light: i64,
    gap_amount: i64,
    stable_amount: i64,
    critical: bool,
    lucky: bool,
    gap_multiplier_raw: i64,
    stable_multiplier_raw: i64,
    gap_base_min: i64,
    gap_base_max: i64,
    stable_base_min: i64,
    stable_base_max: i64,
    overlap_min: Option<i64>,
    overlap_max: Option<i64>,
}

#[derive(Debug, Serialize)]
struct StatusMismatchExample {
    session_id: String,
    run_ordinal: u32,
    ability_id: i64,
    gap_sequence: u64,
    stable_sequence: u64,
    gap_wire_capture_sequence: Option<u64>,
    stable_wire_capture_sequence: Option<u64>,
    serialized_light: i64,
    logical_light: i64,
    gap_amount: i64,
    stable_amount: i64,
    gap_normal_value: Option<i64>,
    stable_normal_value: Option<i64>,
    critical: Option<bool>,
    lucky: Option<bool>,
    gap_to_stable_micros: i64,
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StatusDifferenceKey {
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
}

#[derive(Debug, Default)]
struct StatusDifferenceAccumulator {
    equal_amount_pairs: u64,
    divergent_amount_pairs: u64,
    pairs_with_both_normal_values: u64,
    equal_normal_value_pairs: u64,
    divergent_normal_value_pairs: u64,
    pairs_missing_any_normal_value: u64,
    gap_examples: BTreeSet<(String, u32, u64)>,
    by_ability: BTreeMap<i64, PairOutcomeCounters>,
    by_light_transition: BTreeMap<String, PairOutcomeCounters>,
}

#[derive(Debug, Default, Serialize)]
struct PairOutcomeCounters {
    equal_amount_pairs: u64,
    divergent_amount_pairs: u64,
    pairs_with_both_normal_values: u64,
    equal_normal_value_pairs: u64,
    divergent_normal_value_pairs: u64,
    pairs_missing_any_normal_value: u64,
}

#[derive(Debug, Serialize)]
struct StatusDifferenceAggregate {
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
    equal_amount_pairs: u64,
    divergent_amount_pairs: u64,
    pairs_with_both_normal_values: u64,
    equal_normal_value_pairs: u64,
    divergent_normal_value_pairs: u64,
    pairs_missing_any_normal_value: u64,
    distinct_gap_examples: usize,
    by_ability: BTreeMap<i64, PairOutcomeCounters>,
    by_light_transition: BTreeMap<String, PairOutcomeCounters>,
}

#[derive(Debug, Serialize)]
struct NearestExample {
    session_id: String,
    run_ordinal: u32,
    ability_id: i64,
    gap_sequence: u64,
    stable_sequence: u64,
    source_attribute_differences: Vec<AttributeDifference>,
    target_attribute_differences: Vec<AttributeDifference>,
    source_status_removed: Vec<i64>,
    source_status_added: Vec<i64>,
    target_status_removed: Vec<i64>,
    target_status_added: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct AttributeDifference {
    attribute_id: i32,
    gap_value: Option<i64>,
    stable_value: Option<i64>,
}

#[derive(Debug, Serialize)]
struct Policy {
    runtime_authority: bool,
    unresolved_evidence_is_hidden: bool,
    source_attribute_control: &'static str,
    target_attribute_control: &'static str,
    status_control: &'static str,
    stable_light_requirement: &'static str,
    interpretation_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct Inputs {
    cohort: String,
    gap_proof: String,
    damage_surface: String,
    light_damage_attribute_id: i32,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Inspiration elemental snapshot proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let cohort: FormulaCohort = serde_json::from_reader(BufReader::new(File::open(&args.cohort)?))?;
    let gap: GapProof = serde_json::from_reader(BufReader::new(File::open(&args.gap_proof)?))?;
    let damage_surface: DamageCorrelationSurface =
        serde_json::from_reader(BufReader::new(File::open(&args.damage_surface)?))?;

    let mut gap_by_event = HashMap::new();
    let mut light_sequence_by_mastery = HashMap::new();
    let mut counters = Counters::default();
    for session in gap.sessions {
        for (mastery_sequence, light_sequence) in session.transition_boundaries {
            light_sequence_by_mastery.insert(
                (session.session_id.clone(), mastery_sequence),
                light_sequence,
            );
        }
        for transition in session.exact_examples {
            light_sequence_by_mastery.insert(
                (session.session_id.clone(), transition.mastery_sequence),
                transition.light_sequence,
            );
        }
        for sample in session.gap_light_damage_examples {
            counters.gap_examples = counters.gap_examples.saturating_add(1);
            gap_by_event.insert((session.session_id.clone(), sample.sequence), sample);
        }
    }

    let mut indices_by_identity: HashMap<CalculationIdentity, Vec<usize>> = HashMap::new();
    for (index, sample) in cohort.samples.iter().enumerate() {
        indices_by_identity
            .entry(calculation_identity(sample))
            .or_default()
            .push(index);
    }

    let mut by_ability: BTreeMap<i64, AbilityCounters> = BTreeMap::new();
    let mut equal_examples = Vec::new();
    let mut divergent_examples = Vec::new();
    let mut nearest_examples = Vec::new();
    for gap_sample_index in 0..cohort.samples.len() {
        let sample = &cohort.samples[gap_sample_index];
        let Some(gap_sample) = gap_by_event.get(&(sample.session_id.clone(), sample.sequence))
        else {
            continue;
        };
        counters.gap_examples_present_in_cohort =
            counters.gap_examples_present_in_cohort.saturating_add(1);
        let ability = by_ability.entry(sample.ability_id).or_default();
        ability.gap_examples_present_in_cohort =
            ability.gap_examples_present_in_cohort.saturating_add(1);
        let (Some(serialized_light), Some(logical_light)) =
            (gap_sample.serialized_light, gap_sample.logical_light)
        else {
            continue;
        };
        counters.gap_examples_with_logical_light =
            counters.gap_examples_with_logical_light.saturating_add(1);
        let identity = calculation_identity(sample);
        let Some(candidate_indices) = indices_by_identity.get(&identity) else {
            continue;
        };
        let mut gap_has_pair = false;
        for &candidate_index in candidate_indices {
            if candidate_index == gap_sample_index {
                continue;
            }
            let stable = &cohort.samples[candidate_index];
            counters.stable_controls_with_same_calculation_identity = counters
                .stable_controls_with_same_calculation_identity
                .saturating_add(1);
            if attribute_value(
                &cohort.attribute_states[stable.source_attribute_state_id],
                LIGHT_DAMAGE_ATTRIBUTE_ID,
            ) != Some(logical_light)
            {
                continue;
            }
            counters.stable_controls_with_logical_light = counters
                .stable_controls_with_logical_light
                .saturating_add(1);
            let source_attributes_equal = same_attributes_except(
                &cohort.attribute_states[sample.source_attribute_state_id],
                &cohort.attribute_states[stable.source_attribute_state_id],
                SOURCE_IGNORED_ATTRIBUTE_IDS,
            );
            let target_attributes_equal = same_attributes_except(
                &cohort.attribute_states[sample.target_attribute_state_id],
                &cohort.attribute_states[stable.target_attribute_state_id],
                &[CURRENT_HP_ATTRIBUTE_ID],
            );
            if source_attributes_equal {
                counters.controls_with_source_attributes_equal_except_light_and_hp = counters
                    .controls_with_source_attributes_equal_except_light_and_hp
                    .saturating_add(1);
            }
            if target_attributes_equal {
                counters.controls_with_target_attributes_equal_except_hp = counters
                    .controls_with_target_attributes_equal_except_hp
                    .saturating_add(1);
            }
            if !source_attributes_equal || !target_attributes_equal {
                if nearest_examples.len() < args.example_limit {
                    nearest_examples.push(NearestExample {
                        session_id: sample.session_id.clone(),
                        run_ordinal: sample.run_ordinal,
                        ability_id: sample.ability_id,
                        gap_sequence: sample.sequence,
                        stable_sequence: stable.sequence,
                        source_attribute_differences: attribute_difference_ids(
                            &cohort.attribute_states[sample.source_attribute_state_id],
                            &cohort.attribute_states[stable.source_attribute_state_id],
                            SOURCE_IGNORED_ATTRIBUTE_IDS,
                        ),
                        target_attribute_differences: attribute_difference_ids(
                            &cohort.attribute_states[sample.target_attribute_state_id],
                            &cohort.attribute_states[stable.target_attribute_state_id],
                            &[CURRENT_HP_ATTRIBUTE_ID],
                        ),
                        source_status_removed: Vec::new(),
                        source_status_added: Vec::new(),
                        target_status_removed: Vec::new(),
                        target_status_added: Vec::new(),
                    });
                }
                continue;
            }
            counters.controls_with_both_attribute_states_equal = counters
                .controls_with_both_attribute_states_equal
                .saturating_add(1);
            let source_status_equal = cohort.status_states[sample.source_status_state_id]
                == cohort.status_states[stable.source_status_state_id];
            let target_status_equal = cohort.status_states[sample.target_status_state_id]
                == cohort.status_states[stable.target_status_state_id];
            if !source_status_equal || !target_status_equal {
                match (source_status_equal, target_status_equal) {
                    (false, true) => {
                        counters.controls_rejected_only_by_source_status = counters
                            .controls_rejected_only_by_source_status
                            .saturating_add(1)
                    }
                    (true, false) => {
                        counters.controls_rejected_only_by_target_status = counters
                            .controls_rejected_only_by_target_status
                            .saturating_add(1)
                    }
                    (false, false) => {
                        counters.controls_rejected_by_both_status_states = counters
                            .controls_rejected_by_both_status_states
                            .saturating_add(1)
                    }
                    (true, true) => unreachable!(),
                }
                if nearest_examples.len() < args.example_limit {
                    let (source_status_removed, source_status_added) = status_difference_ids(
                        &cohort.status_states[sample.source_status_state_id],
                        &cohort.status_states[stable.source_status_state_id],
                    );
                    let (target_status_removed, target_status_added) = status_difference_ids(
                        &cohort.status_states[sample.target_status_state_id],
                        &cohort.status_states[stable.target_status_state_id],
                    );
                    nearest_examples.push(NearestExample {
                        session_id: sample.session_id.clone(),
                        run_ordinal: sample.run_ordinal,
                        ability_id: sample.ability_id,
                        gap_sequence: sample.sequence,
                        stable_sequence: stable.sequence,
                        source_attribute_differences: Vec::new(),
                        target_attribute_differences: Vec::new(),
                        source_status_removed,
                        source_status_added,
                        target_status_removed,
                        target_status_added,
                    });
                }
                continue;
            }
            gap_has_pair = true;
            counters.strict_state_control_pairs =
                counters.strict_state_control_pairs.saturating_add(1);
            ability.strict_state_control_pairs =
                ability.strict_state_control_pairs.saturating_add(1);
            let equal_damage =
                sample.amount == stable.amount && sample.normal_value == stable.normal_value;
            if equal_damage {
                counters.strict_state_equal_damage_pairs =
                    counters.strict_state_equal_damage_pairs.saturating_add(1);
                ability.strict_state_equal_damage_pairs =
                    ability.strict_state_equal_damage_pairs.saturating_add(1);
            } else {
                counters.strict_state_divergent_damage_pairs = counters
                    .strict_state_divergent_damage_pairs
                    .saturating_add(1);
                ability.strict_state_divergent_damage_pairs = ability
                    .strict_state_divergent_damage_pairs
                    .saturating_add(1);
            }
            let stable_light = attribute_value(
                &cohort.attribute_states[stable.source_attribute_state_id],
                LIGHT_DAMAGE_ATTRIBUTE_ID,
            )
            .expect("stable Light checked above");
            let example = PairExample {
                session_id: sample.session_id.clone(),
                run_ordinal: sample.run_ordinal,
                ability_id: sample.ability_id,
                hit_event_id: sample.hit_event_id,
                source_entity_uuid: sample.source_entity_uuid,
                direct_source_entity_uuid: sample.direct_source_entity_uuid,
                target_entity_uuid: sample.target_entity_uuid,
                gap_sequence: sample.sequence,
                stable_sequence: stable.sequence,
                gap_mastery_sequence: gap_sample.mastery_sequence,
                gap_mastery_before: gap_sample.mastery_before,
                gap_mastery_after: gap_sample.mastery_after,
                serialized_light,
                logical_light,
                stable_light,
                gap_amount: sample.amount,
                stable_amount: stable.amount,
                gap_normal_value: sample.normal_value,
                stable_normal_value: stable.normal_value,
                equal_damage,
                gap_to_stable_micros: i64::try_from(stable.observed_micros)
                    .unwrap_or(i64::MAX)
                    .saturating_sub(i64::try_from(sample.observed_micros).unwrap_or(i64::MAX)),
            };
            let examples = if equal_damage {
                &mut equal_examples
            } else {
                &mut divergent_examples
            };
            if examples.len() < args.example_limit {
                examples.push(example);
            }
        }
        if gap_has_pair {
            counters.strict_state_gap_examples_with_any_pair = counters
                .strict_state_gap_examples_with_any_pair
                .saturating_add(1);
        }
    }

    // The loop above records rejection-class counters. Re-rank diagnostics per
    // gap event here so the retained examples are the genuinely nearest
    // controls rather than whichever candidate happened to be interned first.
    let nearest_examples = best_nearest_examples(
        &cohort,
        &gap_by_event,
        &indices_by_identity,
        args.example_limit,
    );
    let status_state_isolation_analysis =
        status_state_isolation_diagnostic(&cohort, args.example_limit);
    let occurrence_control_diagnostic = occurrence_control_diagnostic(
        &cohort,
        &gap_by_event,
        &indices_by_identity,
        &status_state_isolation_analysis.scoped_neutral_keys,
        args.example_limit,
    );
    let status_state_isolation_diagnostic = status_state_isolation_analysis.diagnostic;
    let target_status_stack_isolation_diagnostic = target_status_stack_isolation_diagnostic(
        &cohort,
        STEEL_BEAK_STACK_EFFECT_ID,
        args.example_limit,
    );
    let current_hp_isolation_diagnostic =
        current_hp_isolation_diagnostic(&cohort, args.example_limit);
    let serialized_input_outcome_multiplicity_diagnostic =
        serialized_input_outcome_multiplicity_diagnostic(&cohort, args.example_limit);
    let attribute_stage_isolation_diagnostic =
        attribute_stage_isolation_diagnostic(&cohort, args.example_limit);
    let inspiration_composite_transition_diagnostic = inspiration_composite_transition_diagnostic(
        &cohort,
        &damage_surface,
        &target_status_stack_isolation_diagnostic,
        args.example_limit,
    );
    let action_snapshot_lag_diagnostic = action_snapshot_lag_diagnostic(
        &cohort,
        &gap_by_event,
        &light_sequence_by_mastery,
        &indices_by_identity,
        args.example_limit,
    );

    let audit = Audit {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-inspiration-elemental-snapshot-proof",
        policy: Policy {
            runtime_authority: false,
            unresolved_evidence_is_hidden: false,
            source_attribute_control: "all packet attributes equal except the complete delayed Light Bonus family 13170-13175 and volatile CurrentHP 11310",
            target_attribute_control: "all packet attributes equal except volatile CurrentHP 11310",
            status_control: "complete semantic source and target status states equal",
            stable_light_requirement: "control hit source Light Bonus equals the Mastery-derived logical Light expected for the gap hit",
            interpretation_boundary: "equal controlled outputs can prove that delayed serialization did not change effective damage state; divergent pairs and absent controls remain explicit and no formula stage is promoted automatically",
        },
        inputs: Inputs {
            cohort: args.cohort.display().to_string(),
            gap_proof: args.gap_proof.display().to_string(),
            damage_surface: args.damage_surface.display().to_string(),
            light_damage_attribute_id: LIGHT_DAMAGE_ATTRIBUTE_ID,
        },
        counters,
        by_ability,
        equal_examples,
        divergent_examples,
        nearest_examples,
        occurrence_control_diagnostic,
        status_state_isolation_diagnostic,
        target_status_stack_isolation_diagnostic,
        current_hp_isolation_diagnostic,
        serialized_input_outcome_multiplicity_diagnostic,
        attribute_stage_isolation_diagnostic,
        inspiration_composite_transition_diagnostic,
        action_snapshot_lag_diagnostic,
    };
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &audit)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn best_nearest_examples(
    cohort: &FormulaCohort,
    gap_by_event: &HashMap<(String, u64), GapSample>,
    indices_by_identity: &HashMap<CalculationIdentity, Vec<usize>>,
    limit: usize,
) -> Vec<NearestExample> {
    let mut ranked = Vec::new();
    for (gap_sample_index, sample) in cohort.samples.iter().enumerate() {
        let Some(gap_sample) = gap_by_event.get(&(sample.session_id.clone(), sample.sequence))
        else {
            continue;
        };
        let Some(logical_light) = gap_sample.logical_light else {
            continue;
        };
        let Some(candidate_indices) = indices_by_identity.get(&calculation_identity(sample)) else {
            continue;
        };
        let mut best = None;
        for &candidate_index in candidate_indices {
            if candidate_index == gap_sample_index {
                continue;
            }
            let stable = &cohort.samples[candidate_index];
            if attribute_value(
                &cohort.attribute_states[stable.source_attribute_state_id],
                LIGHT_DAMAGE_ATTRIBUTE_ID,
            ) != Some(logical_light)
            {
                continue;
            }
            let source_attribute_differences = attribute_difference_ids(
                &cohort.attribute_states[sample.source_attribute_state_id],
                &cohort.attribute_states[stable.source_attribute_state_id],
                SOURCE_IGNORED_ATTRIBUTE_IDS,
            );
            let target_attribute_differences = attribute_difference_ids(
                &cohort.attribute_states[sample.target_attribute_state_id],
                &cohort.attribute_states[stable.target_attribute_state_id],
                &[CURRENT_HP_ATTRIBUTE_ID],
            );
            let (source_status_removed, source_status_added) = status_difference_ids(
                &cohort.status_states[sample.source_status_state_id],
                &cohort.status_states[stable.source_status_state_id],
            );
            let (target_status_removed, target_status_added) = status_difference_ids(
                &cohort.status_states[sample.target_status_state_id],
                &cohort.status_states[stable.target_status_state_id],
            );
            let difference_score = source_attribute_differences.len()
                + target_attribute_differences.len()
                + source_status_removed.len()
                + source_status_added.len()
                + target_status_removed.len()
                + target_status_added.len();
            let time_distance = sample.observed_micros.abs_diff(stable.observed_micros);
            let diagnostic = NearestExample {
                session_id: sample.session_id.clone(),
                run_ordinal: sample.run_ordinal,
                ability_id: sample.ability_id,
                gap_sequence: sample.sequence,
                stable_sequence: stable.sequence,
                source_attribute_differences,
                target_attribute_differences,
                source_status_removed,
                source_status_added,
                target_status_removed,
                target_status_added,
            };
            if best.as_ref().is_none_or(|(score, distance, _)| {
                (difference_score, time_distance) < (*score, *distance)
            }) {
                best = Some((difference_score, time_distance, diagnostic));
            }
        }
        if let Some(best) = best {
            ranked.push(best);
        }
    }
    ranked.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.session_id.cmp(&right.2.session_id))
            .then_with(|| left.2.run_ordinal.cmp(&right.2.run_ordinal))
            .then_with(|| left.2.gap_sequence.cmp(&right.2.gap_sequence))
            .then_with(|| left.2.stable_sequence.cmp(&right.2.stable_sequence))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, _, example)| example)
        .collect()
}

fn action_snapshot_lag_diagnostic(
    cohort: &FormulaCohort,
    gap_by_event: &HashMap<(String, u64), GapSample>,
    light_sequence_by_mastery: &HashMap<(String, u64), u64>,
    indices_by_identity: &HashMap<CalculationIdentity, Vec<usize>>,
    example_limit: usize,
) -> ActionSnapshotLagDiagnostic {
    let mut counters = ActionSnapshotLagCounters::default();
    let mut by_ability: BTreeMap<i64, ActionSnapshotLagAbilityCounters> = BTreeMap::new();
    let mut ranked_examples = Vec::new();
    let mut prior_ignored_attributes = vec![CURRENT_HP_ATTRIBUTE_ID];
    prior_ignored_attributes.extend_from_slice(INSPIRATION_DIRECT_ATTRIBUTE_IDS);
    prior_ignored_attributes.extend_from_slice(INSPIRATION_DERIVED_ATTRIBUTE_IDS);
    let mut next_ignored_attributes = vec![CURRENT_HP_ATTRIBUTE_ID];
    next_ignored_attributes.extend_from_slice(INSPIRATION_DERIVED_ATTRIBUTE_IDS);

    for (gap_index, sample) in cohort.samples.iter().enumerate() {
        let Some(gap) = gap_by_event.get(&(sample.session_id.clone(), sample.sequence)) else {
            continue;
        };
        counters.gap_events = counters.gap_events.saturating_add(1);
        let ability = by_ability.entry(sample.ability_id).or_default();
        ability.gap_events = ability.gap_events.saturating_add(1);
        let (Some(serialized_light), Some(logical_light)) =
            (gap.serialized_light, gap.logical_light)
        else {
            continue;
        };
        let Some(&light_sequence) =
            light_sequence_by_mastery.get(&(sample.session_id.clone(), gap.mastery_sequence))
        else {
            continue;
        };
        counters.gap_events_with_transition_boundary = counters
            .gap_events_with_transition_boundary
            .saturating_add(1);
        let Some(candidate_indices) = indices_by_identity.get(&calculation_identity(sample)) else {
            continue;
        };

        let mut prior_best = None;
        let mut next_best = None;
        for &candidate_index in candidate_indices {
            if candidate_index == gap_index {
                continue;
            }
            let candidate = &cohort.samples[candidate_index];
            let candidate_light = attribute_value(
                &cohort.attribute_states[candidate.source_attribute_state_id],
                LIGHT_DAMAGE_ATTRIBUTE_ID,
            );
            if candidate.sequence < gap.mastery_sequence
                && candidate_light == Some(serialized_light)
            {
                let observation = boundary_observation(
                    cohort,
                    sample,
                    candidate,
                    &prior_ignored_attributes,
                    &[INSPIRATION_EFFECT_ID],
                );
                let rank = (
                    boundary_difference_score(&observation),
                    sample.observed_micros.abs_diff(candidate.observed_micros),
                    candidate_index,
                );
                if prior_best.as_ref().is_none_or(|current| rank < *current) {
                    prior_best = Some(rank);
                }
            }
            if candidate.sequence > light_sequence && candidate_light == Some(logical_light) {
                let observation =
                    boundary_observation(cohort, sample, candidate, &next_ignored_attributes, &[]);
                let rank = (
                    boundary_difference_score(&observation),
                    sample.observed_micros.abs_diff(candidate.observed_micros),
                    candidate_index,
                );
                if next_best.as_ref().is_none_or(|current| rank < *current) {
                    next_best = Some(rank);
                }
            }
        }

        let prior = prior_best.map(|(_, _, index)| {
            boundary_observation(
                cohort,
                sample,
                &cohort.samples[index],
                &prior_ignored_attributes,
                &[INSPIRATION_EFFECT_ID],
            )
        });
        let next = next_best.map(|(_, _, index)| {
            boundary_observation(
                cohort,
                sample,
                &cohort.samples[index],
                &next_ignored_attributes,
                &[],
            )
        });
        if prior.is_some() {
            counters.gap_events_with_prior_old_light_observation = counters
                .gap_events_with_prior_old_light_observation
                .saturating_add(1);
        }
        if next.is_some() {
            counters.gap_events_with_next_logical_light_observation = counters
                .gap_events_with_next_logical_light_observation
                .saturating_add(1);
        }
        if prior
            .as_ref()
            .is_some_and(|value| value.amount_equal_to_gap)
        {
            counters.prior_observation_amount_equal =
                counters.prior_observation_amount_equal.saturating_add(1);
        }
        if next.as_ref().is_some_and(|value| value.amount_equal_to_gap) {
            counters.next_observation_amount_equal =
                counters.next_observation_amount_equal.saturating_add(1);
        }
        if prior.as_ref().is_some_and(|value| value.state_controlled) {
            counters.prior_observation_state_controlled = counters
                .prior_observation_state_controlled
                .saturating_add(1);
        }
        if next.as_ref().is_some_and(|value| value.state_controlled) {
            counters.next_observation_state_controlled =
                counters.next_observation_state_controlled.saturating_add(1);
        }

        if let (Some(prior), Some(next)) = (&prior, &next) {
            counters.gap_events_with_both_boundary_observations = counters
                .gap_events_with_both_boundary_observations
                .saturating_add(1);
            ability.gap_events_with_both_boundary_observations = ability
                .gap_events_with_both_boundary_observations
                .saturating_add(1);
            match (prior.amount_equal_to_gap, next.amount_equal_to_gap) {
                (true, true) => {
                    counters.both_observations_amount_equal =
                        counters.both_observations_amount_equal.saturating_add(1);
                    ability.both_observations_amount_equal =
                        ability.both_observations_amount_equal.saturating_add(1);
                }
                (true, false) => {
                    counters.only_prior_observation_amount_equal = counters
                        .only_prior_observation_amount_equal
                        .saturating_add(1);
                    ability.only_prior_observation_amount_equal = ability
                        .only_prior_observation_amount_equal
                        .saturating_add(1);
                }
                (false, true) => {
                    counters.only_next_observation_amount_equal = counters
                        .only_next_observation_amount_equal
                        .saturating_add(1);
                    ability.only_next_observation_amount_equal =
                        ability.only_next_observation_amount_equal.saturating_add(1);
                }
                (false, false) => {
                    counters.neither_observation_amount_equal =
                        counters.neither_observation_amount_equal.saturating_add(1);
                    ability.neither_observation_amount_equal =
                        ability.neither_observation_amount_equal.saturating_add(1);
                }
            }
            if prior.state_controlled && next.state_controlled {
                counters.both_observations_state_controlled = counters
                    .both_observations_state_controlled
                    .saturating_add(1);
                ability.both_observations_state_controlled =
                    ability.both_observations_state_controlled.saturating_add(1);
                match (prior.amount_equal_to_gap, next.amount_equal_to_gap) {
                    (true, false) => {
                        counters.controlled_both_match_prior_only =
                            counters.controlled_both_match_prior_only.saturating_add(1)
                    }
                    (false, true) => {
                        counters.controlled_both_match_next_only =
                            counters.controlled_both_match_next_only.saturating_add(1)
                    }
                    (true, true) => {
                        counters.controlled_both_match_both =
                            counters.controlled_both_match_both.saturating_add(1)
                    }
                    (false, false) => {
                        counters.controlled_both_match_neither =
                            counters.controlled_both_match_neither.saturating_add(1)
                    }
                }
            }
        }

        let controlled_rank = u8::from(
            !prior.as_ref().is_some_and(|value| value.state_controlled)
                || !next.as_ref().is_some_and(|value| value.state_controlled),
        );
        let boundary_rank = u8::from(prior.is_none() || next.is_none());
        let distance = prior
            .as_ref()
            .map_or(u64::MAX / 2, |value| {
                value.observed_micros_delta.unsigned_abs()
            })
            .saturating_add(next.as_ref().map_or(u64::MAX / 2, |value| {
                value.observed_micros_delta.unsigned_abs()
            }));
        ranked_examples.push((
            controlled_rank,
            boundary_rank,
            distance,
            ActionSnapshotLagExample {
                session_id: sample.session_id.clone(),
                run_ordinal: sample.run_ordinal,
                ability_id: sample.ability_id,
                hit_event_id: sample.hit_event_id,
                gap_sequence: sample.sequence,
                mastery_sequence: gap.mastery_sequence,
                light_sequence,
                serialized_light,
                logical_light,
                gap_amount: sample.amount,
                skill_effect_uuid: sample.packet.skill_effect_uuid,
                skill_effect_total_damage: sample.packet.skill_effect_total_damage,
                skill_effect_group_index: sample.packet.skill_effect_group_index,
                skill_effect_component_index: sample.packet.skill_effect_component_index,
                skill_effect_component_count: sample.packet.skill_effect_component_count,
                prior,
                next,
            },
        ));
    }

    ranked_examples.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.session_id.cmp(&right.3.session_id))
            .then_with(|| left.3.gap_sequence.cmp(&right.3.gap_sequence))
    });

    ActionSnapshotLagDiagnostic {
        policy: ActionSnapshotLagPolicy {
            runtime_authority: false,
            prior_boundary: "best-state earlier observation with the exact calculation identity, sequence before the Mastery update, and source Light equal to the serialized pre-transition value; candidates minimize non-Inspiration state differences before time distance",
            next_boundary: "best-state later observation with the exact calculation identity, sequence after the paired Light update, and source Light equal to the logical post-transition value; candidates minimize non-derived state differences before time distance",
            state_control: "prior comparison ignores CurrentHP, the packet-proven Inspiration direct attribute vector, the explicitly listed packet-observed downstream comparison families, and effect 2202041 itself; next comparison ignores CurrentHP and only those downstream comparison families. All remaining source attributes, target attributes except CurrentHP, and semantic statuses must match exactly",
            inspiration_direct_attribute_ids: INSPIRATION_DIRECT_ATTRIBUTE_IDS.to_vec(),
            packet_observed_downstream_attribute_ids: INSPIRATION_DERIVED_ATTRIBUTE_IDS.to_vec(),
            component_identity: "server skill-effect UUID plus component index and component count are calculation identity when present; group index is retained only as occurrence evidence and is never compared across casts",
            interpretation_boundary: "amount equality is only an observation, not proof of a server snapshot stage; only two-sided state-controlled witnesses can distinguish old versus logical state, and every missing or conflicting witness remains reported",
        },
        counters,
        by_ability,
        examples: ranked_examples
            .into_iter()
            .take(example_limit)
            .map(|(_, _, _, example)| example)
            .collect(),
    }
}

fn boundary_observation(
    cohort: &FormulaCohort,
    gap: &FormulaSample,
    boundary: &FormulaSample,
    ignored_source_attributes: &[i32],
    ignored_status_ids: &[i64],
) -> ActionSnapshotBoundaryObservation {
    let source_attribute_differences = attribute_difference_ids(
        &cohort.attribute_states[gap.source_attribute_state_id],
        &cohort.attribute_states[boundary.source_attribute_state_id],
        ignored_source_attributes,
    );
    let target_attribute_differences = attribute_difference_ids(
        &cohort.attribute_states[gap.target_attribute_state_id],
        &cohort.attribute_states[boundary.target_attribute_state_id],
        &[CURRENT_HP_ATTRIBUTE_ID],
    );
    let gap_source_statuses = cohort.status_states[gap.source_status_state_id]
        .iter()
        .filter(|status| !ignored_status_ids.contains(&status.effect_id))
        .cloned()
        .collect::<Vec<_>>();
    let boundary_source_statuses = cohort.status_states[boundary.source_status_state_id]
        .iter()
        .filter(|status| !ignored_status_ids.contains(&status.effect_id))
        .cloned()
        .collect::<Vec<_>>();
    let (source_status_removed, source_status_added) =
        status_entry_differences(&gap_source_statuses, &boundary_source_statuses);
    let gap_target_statuses = cohort.status_states[gap.target_status_state_id]
        .iter()
        .filter(|status| !ignored_status_ids.contains(&status.effect_id))
        .cloned()
        .collect::<Vec<_>>();
    let boundary_target_statuses = cohort.status_states[boundary.target_status_state_id]
        .iter()
        .filter(|status| !ignored_status_ids.contains(&status.effect_id))
        .cloned()
        .collect::<Vec<_>>();
    let (target_status_removed, target_status_added) =
        status_entry_differences(&gap_target_statuses, &boundary_target_statuses);
    let state_controlled = source_attribute_differences.is_empty()
        && target_attribute_differences.is_empty()
        && source_status_removed.is_empty()
        && source_status_added.is_empty()
        && target_status_removed.is_empty()
        && target_status_added.is_empty();
    ActionSnapshotBoundaryObservation {
        sequence: boundary.sequence,
        observed_micros_delta: i64::try_from(boundary.observed_micros)
            .unwrap_or(i64::MAX)
            .saturating_sub(i64::try_from(gap.observed_micros).unwrap_or(i64::MAX)),
        light: attribute_value(
            &cohort.attribute_states[boundary.source_attribute_state_id],
            LIGHT_DAMAGE_ATTRIBUTE_ID,
        ),
        amount: boundary.amount,
        normal_value: boundary.normal_value,
        amount_equal_to_gap: boundary.amount == gap.amount,
        state_controlled,
        source_attribute_differences,
        target_attribute_differences,
        source_status_removed,
        source_status_added,
        target_status_removed,
        target_status_added,
    }
}

fn boundary_difference_score(observation: &ActionSnapshotBoundaryObservation) -> usize {
    observation
        .source_attribute_differences
        .len()
        .saturating_add(observation.target_attribute_differences.len())
        .saturating_add(observation.source_status_removed.len())
        .saturating_add(observation.source_status_added.len())
        .saturating_add(observation.target_status_removed.len())
        .saturating_add(observation.target_status_added.len())
}

fn canonical_scoped_status_transition_key(
    calculation_identity: CalculationIdentity,
    effect_id: i64,
    source_status_removed: Vec<StatusEntry>,
    source_status_added: Vec<StatusEntry>,
    target_status_removed: Vec<StatusEntry>,
    target_status_added: Vec<StatusEntry>,
) -> ScopedStatusTransitionKey {
    let forward = ScopedStatusTransitionKey {
        calculation_identity: calculation_identity.clone(),
        effect_id,
        source_status_removed: source_status_removed.clone(),
        source_status_added: source_status_added.clone(),
        target_status_removed: target_status_removed.clone(),
        target_status_added: target_status_added.clone(),
    };
    let reverse = ScopedStatusTransitionKey {
        calculation_identity,
        effect_id,
        source_status_removed: source_status_added,
        source_status_added: source_status_removed,
        target_status_removed: target_status_added,
        target_status_added: target_status_removed,
    };
    forward.min(reverse)
}

fn status_state_isolation_diagnostic(
    cohort: &FormulaCohort,
    example_limit: usize,
) -> StatusStateIsolationAnalysis {
    type AttributeCalculationKey = (
        CalculationIdentity,
        Vec<AttributeEntry>,
        Vec<AttributeEntry>,
    );
    type StatusStateKey = (usize, usize);

    let mut groups: HashMap<
        AttributeCalculationKey,
        BTreeMap<StatusStateKey, StatusOutcomeBucket>,
    > = HashMap::new();
    for sample in &cohort.samples {
        let bucket = groups
            .entry((
                calculation_identity(sample),
                attributes_without_current_hp(
                    &cohort.attribute_states[sample.source_attribute_state_id],
                ),
                attributes_without_current_hp(
                    &cohort.attribute_states[sample.target_attribute_state_id],
                ),
            ))
            .or_default()
            .entry((sample.source_status_state_id, sample.target_status_state_id))
            .or_default();
        bucket.occurrences = bucket.occurrences.saturating_add(1);
        bucket.source_current_hp.insert(attribute_value(
            &cohort.attribute_states[sample.source_attribute_state_id],
            CURRENT_HP_ATTRIBUTE_ID,
        ));
        bucket.target_current_hp.insert(attribute_value(
            &cohort.attribute_states[sample.target_attribute_state_id],
            CURRENT_HP_ATTRIBUTE_ID,
        ));
        *bucket
            .outcomes
            .entry(StatusOutcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
    }

    let mut counters = StatusStateIsolationCounters::default();
    let mut by_effect: BTreeMap<i64, StatusStateIsolationEffectCounters> = BTreeMap::new();
    let mut exact_scoped_transitions: BTreeMap<
        ScopedStatusTransitionKey,
        ScopedStatusTransitionAccumulator,
    > = BTreeMap::new();
    let mut deterministic_equal_examples = Vec::new();
    let mut deterministic_divergent_examples = Vec::new();
    for ((identity, _, _), states) in groups {
        if states.len() < 2 {
            continue;
        }
        counters.current_hp_relaxed_attribute_groups = counters
            .current_hp_relaxed_attribute_groups
            .saturating_add(1);
        let states = states.into_iter().collect::<Vec<_>>();
        for left_index in 0..states.len() {
            for right_index in left_index.saturating_add(1)..states.len() {
                counters.status_state_pairs = counters.status_state_pairs.saturating_add(1);
                let ((left_source_id, left_target_id), left) = &states[left_index];
                let ((right_source_id, right_target_id), right) = &states[right_index];
                let (source_status_removed, source_status_added) = status_entry_differences(
                    &cohort.status_states[*left_source_id],
                    &cohort.status_states[*right_source_id],
                );
                let (target_status_removed, target_status_added) = status_entry_differences(
                    &cohort.status_states[*left_target_id],
                    &cohort.status_states[*right_target_id],
                );
                let changed_effects = source_status_removed
                    .iter()
                    .chain(&source_status_added)
                    .chain(&target_status_removed)
                    .chain(&target_status_added)
                    .map(|status| status.effect_id)
                    .collect::<BTreeSet<_>>();
                let [effect_id] = changed_effects.iter().copied().collect::<Vec<_>>()[..] else {
                    continue;
                };
                counters.single_effect_difference_pairs =
                    counters.single_effect_difference_pairs.saturating_add(1);
                let effect = by_effect.entry(effect_id).or_default();
                effect.status_state_pairs = effect.status_state_pairs.saturating_add(1);
                effect.candidate_occurrence_pairs = effect
                    .candidate_occurrence_pairs
                    .saturating_add(left.occurrences.saturating_mul(right.occurrences));
                effect.abilities.insert(identity.ability_id);

                let scoped = exact_scoped_transitions
                    .entry(canonical_scoped_status_transition_key(
                        identity.clone(),
                        effect_id,
                        source_status_removed.clone(),
                        source_status_added.clone(),
                        target_status_removed.clone(),
                        target_status_added.clone(),
                    ))
                    .or_default();
                scoped.status_state_pairs = scoped.status_state_pairs.saturating_add(1);
                scoped.candidate_occurrence_pairs = scoped
                    .candidate_occurrence_pairs
                    .saturating_add(left.occurrences.saturating_mul(right.occurrences));

                let left_outcomes = left.outcomes.keys().cloned().collect::<Vec<_>>();
                let right_outcomes = right.outcomes.keys().cloned().collect::<Vec<_>>();
                let deterministic = left_outcomes.len() == 1 && right_outcomes.len() == 1;
                let example = || StatusStateIsolationExample {
                    effect_id,
                    session_id: identity.session_id.clone(),
                    run_ordinal: identity.run_ordinal,
                    source_entity_uuid: identity.source_entity_uuid,
                    target_entity_uuid: identity.target_entity_uuid,
                    ability_id: identity.ability_id,
                    left_source_status_state_id: *left_source_id,
                    left_target_status_state_id: *left_target_id,
                    right_source_status_state_id: *right_source_id,
                    right_target_status_state_id: *right_target_id,
                    left_source_current_hp: left.source_current_hp.iter().copied().collect(),
                    right_source_current_hp: right.source_current_hp.iter().copied().collect(),
                    left_target_current_hp: left.target_current_hp.iter().copied().collect(),
                    right_target_current_hp: right.target_current_hp.iter().copied().collect(),
                    left_occurrences: left.occurrences,
                    right_occurrences: right.occurrences,
                    left_outcomes: left_outcomes.clone(),
                    right_outcomes: right_outcomes.clone(),
                    source_status_removed: source_status_removed.clone(),
                    source_status_added: source_status_added.clone(),
                    target_status_removed: target_status_removed.clone(),
                    target_status_added: target_status_added.clone(),
                };
                if deterministic && left_outcomes == right_outcomes {
                    counters.deterministic_equal_output_pairs =
                        counters.deterministic_equal_output_pairs.saturating_add(1);
                    effect.deterministic_equal_output_pairs =
                        effect.deterministic_equal_output_pairs.saturating_add(1);
                    scoped.deterministic_equal_output_pairs =
                        scoped.deterministic_equal_output_pairs.saturating_add(1);
                    if deterministic_equal_examples.len() < example_limit {
                        deterministic_equal_examples.push(example());
                    }
                } else if deterministic {
                    counters.deterministic_divergent_output_pairs = counters
                        .deterministic_divergent_output_pairs
                        .saturating_add(1);
                    effect.deterministic_divergent_output_pairs = effect
                        .deterministic_divergent_output_pairs
                        .saturating_add(1);
                    scoped.deterministic_divergent_output_pairs = scoped
                        .deterministic_divergent_output_pairs
                        .saturating_add(1);
                    if deterministic_divergent_examples.len() < example_limit {
                        deterministic_divergent_examples.push(example());
                    }
                } else {
                    counters.nondeterministic_or_partially_overlapping_pairs = counters
                        .nondeterministic_or_partially_overlapping_pairs
                        .saturating_add(1);
                    effect.nondeterministic_or_partially_overlapping_pairs = effect
                        .nondeterministic_or_partially_overlapping_pairs
                        .saturating_add(1);
                    scoped.nondeterministic_or_partially_overlapping_pairs = scoped
                        .nondeterministic_or_partially_overlapping_pairs
                        .saturating_add(1);
                }
            }
        }
    }

    let exact_scoped_transition_evidence = exact_scoped_transitions
        .into_iter()
        .map(|(key, evidence)| {
            let scoped_neutral_control_eligible = evidence.candidate_occurrence_pairs >= 2
                && evidence.deterministic_equal_output_pairs > 0
                && evidence.deterministic_divergent_output_pairs == 0
                && evidence.nondeterministic_or_partially_overlapping_pairs == 0;
            ScopedStatusTransitionEvidence {
                calculation_identity: key.calculation_identity,
                effect_id: key.effect_id,
                source_status_removed: key.source_status_removed,
                source_status_added: key.source_status_added,
                target_status_removed: key.target_status_removed,
                target_status_added: key.target_status_added,
                status_state_pairs: evidence.status_state_pairs,
                candidate_occurrence_pairs: evidence.candidate_occurrence_pairs,
                deterministic_equal_output_pairs: evidence.deterministic_equal_output_pairs,
                deterministic_divergent_output_pairs: evidence.deterministic_divergent_output_pairs,
                nondeterministic_or_partially_overlapping_pairs: evidence
                    .nondeterministic_or_partially_overlapping_pairs,
                scoped_neutral_control_eligible,
            }
        })
        .collect::<Vec<_>>();
    let scoped_neutral_keys = exact_scoped_transition_evidence
        .iter()
        .filter(|evidence| evidence.scoped_neutral_control_eligible)
        .map(|evidence| ScopedStatusTransitionKey {
            calculation_identity: evidence.calculation_identity.clone(),
            effect_id: evidence.effect_id,
            source_status_removed: evidence.source_status_removed.clone(),
            source_status_added: evidence.source_status_added.clone(),
            target_status_removed: evidence.target_status_removed.clone(),
            target_status_added: evidence.target_status_added.clone(),
        })
        .collect::<BTreeSet<_>>();
    counters.exact_scoped_transition_candidates =
        u64::try_from(exact_scoped_transition_evidence.len()).unwrap_or(u64::MAX);
    counters.exact_scoped_neutral_transition_candidates = u64::try_from(
        exact_scoped_transition_evidence
            .iter()
            .filter(|evidence| evidence.scoped_neutral_control_eligible)
            .count(),
    )
    .unwrap_or(u64::MAX);

    StatusStateIsolationAnalysis {
        diagnostic: StatusStateIsolationDiagnostic {
            policy: StatusStateIsolationPolicy {
                runtime_authority: false,
                attribute_control: "complete source and target packet attribute states are exact except volatile CurrentHP 11310; each side's CurrentHP values are retained in every witness and HP-dependent abilities remain ineligible without their separate formula proof",
                status_control: "the compared source-plus-target status states differ by exactly one effect ID; stack, level, provider, and origin changes remain explicit",
                deterministic_equal_interpretation: "both exposed states produced exactly one identical (amount, normal_value) outcome for the same complete calculation identity; this is an exact output-invariance witness, not a global permission to ignore the effect",
                promotion_boundary: "an effect remains formula-relevant until repeated isolated lifecycle evidence proves its exact source/target transition for the complete calculation identity; scoped-neutral eligibility requires at least two candidate event pairings, at least one deterministic equal-output state pair, and zero divergent or nondeterministic pairs. No eligibility is global and every counterexample remains retained",
            },
            counters,
            by_effect,
            exact_scoped_transition_evidence,
            deterministic_equal_examples,
            deterministic_divergent_examples,
        },
        scoped_neutral_keys,
    }
}

fn target_status_stack_isolation_diagnostic(
    cohort: &FormulaCohort,
    selected_effect_id: i64,
    example_limit: usize,
) -> TargetStatusStackIsolationDiagnostic {
    let mut groups: HashMap<
        TargetStatusStackControlKey,
        BTreeMap<TargetStatusStackState, TargetStatusStackBucket>,
    > = HashMap::new();
    let mut counters = TargetStatusStackIsolationCounters {
        selected_effect_id,
        ..TargetStatusStackIsolationCounters::default()
    };

    for sample in &cohort.samples {
        let source_attributes = &cohort.attribute_states[sample.source_attribute_state_id];
        let target_attributes = &cohort.attribute_states[sample.target_attribute_state_id];
        let source_statuses = &cohort.status_states[sample.source_status_state_id];
        let target_statuses = &cohort.status_states[sample.target_status_state_id];
        let selected_target_statuses = target_statuses
            .iter()
            .filter(|status| status.effect_id == selected_effect_id)
            .cloned()
            .collect::<Vec<_>>();
        if selected_target_statuses.is_empty() {
            continue;
        }
        counters.samples = counters.samples.saturating_add(1);
        let key = TargetStatusStackControlKey {
            identity: calculation_identity(sample),
            source_attributes_without_current_hp: attributes_without_current_hp(source_attributes),
            target_attributes_without_current_hp: attributes_without_current_hp(target_attributes),
            source_statuses: source_statuses.clone(),
            target_statuses_without_selected_effect: target_statuses
                .iter()
                .filter(|status| status.effect_id != selected_effect_id)
                .cloned()
                .collect(),
        };
        let state = TargetStatusStackState {
            selected_target_statuses,
        };
        let bucket = groups.entry(key).or_default().entry(state).or_default();
        bucket.occurrences = bucket.occurrences.saturating_add(1);
        *bucket
            .outcomes
            .entry(StatusOutcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
        bucket.sequences.insert(sample.sequence);
        bucket
            .source_current_hp
            .insert(attribute_value(source_attributes, CURRENT_HP_ATTRIBUTE_ID));
        bucket
            .target_current_hp
            .insert(attribute_value(target_attributes, CURRENT_HP_ATTRIBUTE_ID));
    }

    let mut by_ability_hit = BTreeMap::<String, TargetStatusStackAbilityCounters>::new();
    let mut deterministic_equal_examples = Vec::new();
    let mut deterministic_divergent_examples = Vec::new();
    for (key, states) in groups {
        counters.controlled_groups = counters.controlled_groups.saturating_add(1);
        if states.len() < 2 {
            continue;
        }
        counters.controlled_groups_with_multiple_stack_states = counters
            .controlled_groups_with_multiple_stack_states
            .saturating_add(1);
        let states = states.into_iter().collect::<Vec<_>>();
        for left_index in 0..states.len() {
            for right_index in left_index.saturating_add(1)..states.len() {
                counters.stack_state_pairs = counters.stack_state_pairs.saturating_add(1);
                let (left_state, left_bucket) = &states[left_index];
                let (right_state, right_bucket) = &states[right_index];
                let ability_key = format!(
                    "{}:{}",
                    key.identity.ability_id,
                    key.identity.hit_event_id.unwrap_or_default()
                );
                let ability = by_ability_hit.entry(ability_key).or_default();
                ability.stack_state_pairs = ability.stack_state_pairs.saturating_add(1);
                let left_outcomes = left_bucket.outcomes.keys().cloned().collect::<Vec<_>>();
                let right_outcomes = right_bucket.outcomes.keys().cloned().collect::<Vec<_>>();
                let deterministic = left_outcomes.len() == 1 && right_outcomes.len() == 1;
                let example = TargetStatusStackIsolationExample {
                    session_id: key.identity.session_id.clone(),
                    run_ordinal: key.identity.run_ordinal,
                    source_entity_uuid: key.identity.source_entity_uuid,
                    target_entity_uuid: key.identity.target_entity_uuid,
                    ability_id: key.identity.ability_id,
                    hit_event_id: key.identity.hit_event_id,
                    left_selected_target_statuses: left_state.selected_target_statuses.clone(),
                    right_selected_target_statuses: right_state.selected_target_statuses.clone(),
                    left_sequences: left_bucket.sequences.iter().copied().collect(),
                    right_sequences: right_bucket.sequences.iter().copied().collect(),
                    left_source_current_hp: left_bucket.source_current_hp.iter().copied().collect(),
                    right_source_current_hp: right_bucket
                        .source_current_hp
                        .iter()
                        .copied()
                        .collect(),
                    left_target_current_hp: left_bucket.target_current_hp.iter().copied().collect(),
                    right_target_current_hp: right_bucket
                        .target_current_hp
                        .iter()
                        .copied()
                        .collect(),
                    left_occurrences: left_bucket.occurrences,
                    right_occurrences: right_bucket.occurrences,
                    left_outcomes: left_outcomes.clone(),
                    right_outcomes: right_outcomes.clone(),
                };
                if deterministic && left_outcomes == right_outcomes {
                    counters.deterministic_equal_output_pairs =
                        counters.deterministic_equal_output_pairs.saturating_add(1);
                    ability.deterministic_equal_output_pairs =
                        ability.deterministic_equal_output_pairs.saturating_add(1);
                    deterministic_equal_examples.push(example);
                } else if deterministic {
                    counters.deterministic_divergent_output_pairs = counters
                        .deterministic_divergent_output_pairs
                        .saturating_add(1);
                    ability.deterministic_divergent_output_pairs = ability
                        .deterministic_divergent_output_pairs
                        .saturating_add(1);
                    deterministic_divergent_examples.push(example);
                } else {
                    counters.nondeterministic_or_partially_overlapping_pairs = counters
                        .nondeterministic_or_partially_overlapping_pairs
                        .saturating_add(1);
                    ability.nondeterministic_or_partially_overlapping_pairs = ability
                        .nondeterministic_or_partially_overlapping_pairs
                        .saturating_add(1);
                }
            }
        }
    }

    let example_rank = |example: &TargetStatusStackIsolationExample| {
        (
            example.ability_id != 55_240 || example.hit_event_id != Some(3),
            example.ability_id,
            example.hit_event_id,
            example
                .left_sequences
                .first()
                .copied()
                .unwrap_or(u64::MAX)
                .abs_diff(example.right_sequences.first().copied().unwrap_or(u64::MAX)),
        )
    };
    deterministic_equal_examples.sort_by_key(example_rank);
    deterministic_divergent_examples.sort_by_key(example_rank);
    deterministic_equal_examples.truncate(example_limit);
    deterministic_divergent_examples.truncate(example_limit);

    TargetStatusStackIsolationDiagnostic {
        policy: TargetStatusStackIsolationPolicy {
            runtime_authority: false,
            selected_effect_id,
            calculation_control: "same complete packet calculation identity, including ability, hit, flags, property, stage, and skill-effect component identity",
            attribute_control: "complete source and target packet attribute states are exact except volatile CurrentHP 11310, whose values remain visible in every witness",
            status_control: "all source statuses and all non-selected target statuses are exact; only the selected target effect's complete entries may differ, preserving provider, origin, level, and stack count",
            interpretation_boundary: "equal output is scoped evidence that the selected stack transition did not continuously scale this exact ability/hit calculation; divergent and nondeterministic pairs remain visible and separately emitted proc damage is never discarded",
        },
        counters,
        by_ability_hit,
        deterministic_equal_examples,
        deterministic_divergent_examples,
    }
}

fn current_hp_isolation_diagnostic(
    cohort: &FormulaCohort,
    example_limit: usize,
) -> CurrentHpIsolationDiagnostic {
    let mut groups = HashMap::<
        CurrentHpIsolationControlKey,
        BTreeMap<Option<i64>, CurrentHpIsolationBucket>,
    >::new();
    for sample in &cohort.samples {
        let source_attributes = &cohort.attribute_states[sample.source_attribute_state_id];
        let target_attributes = &cohort.attribute_states[sample.target_attribute_state_id];
        let source_current_hp = attribute_value(source_attributes, CURRENT_HP_ATTRIBUTE_ID);
        let target_current_hp = attribute_value(target_attributes, CURRENT_HP_ATTRIBUTE_ID);
        let source_attributes_without_current_hp = attributes_without_current_hp(source_attributes);
        let target_attributes_without_current_hp = attributes_without_current_hp(target_attributes);
        let source_statuses = cohort.status_states[sample.source_status_state_id].clone();
        let target_statuses = cohort.status_states[sample.target_status_state_id].clone();
        for (axis, selected_current_hp, opposite_current_hp) in [
            ("source_current_hp", source_current_hp, target_current_hp),
            ("target_current_hp", target_current_hp, source_current_hp),
        ] {
            let key = CurrentHpIsolationControlKey {
                axis,
                identity: calculation_identity(sample),
                source_attributes_without_current_hp: source_attributes_without_current_hp.clone(),
                target_attributes_without_current_hp: target_attributes_without_current_hp.clone(),
                source_statuses: source_statuses.clone(),
                target_statuses: target_statuses.clone(),
                opposite_current_hp,
            };
            let bucket = groups
                .entry(key)
                .or_default()
                .entry(selected_current_hp)
                .or_default();
            bucket.occurrences = bucket.occurrences.saturating_add(1);
            *bucket
                .outcomes
                .entry(StatusOutcome {
                    amount: sample.amount,
                    normal_value: sample.normal_value,
                })
                .or_default() += 1;
            bucket.sequences.insert(sample.sequence);
        }
    }

    let mut counters = CurrentHpIsolationCounters::default();
    let mut by_ability_hit = BTreeMap::<String, CurrentHpIsolationAbilityCounters>::new();
    let mut deterministic_equal_examples = Vec::new();
    let mut deterministic_divergent_examples = Vec::new();
    for (key, states) in groups {
        counters.controlled_groups = counters.controlled_groups.saturating_add(1);
        if states.len() < 2 {
            continue;
        }
        counters.controlled_groups_with_multiple_hp_states = counters
            .controlled_groups_with_multiple_hp_states
            .saturating_add(1);
        let states = states.into_iter().collect::<Vec<_>>();
        for left_index in 0..states.len() {
            for right_index in left_index.saturating_add(1)..states.len() {
                counters.hp_state_pairs = counters.hp_state_pairs.saturating_add(1);
                let (left_hp, left_bucket) = &states[left_index];
                let (right_hp, right_bucket) = &states[right_index];
                let left_outcomes = left_bucket.outcomes.keys().cloned().collect::<Vec<_>>();
                let right_outcomes = right_bucket.outcomes.keys().cloned().collect::<Vec<_>>();
                let deterministic = left_outcomes.len() == 1 && right_outcomes.len() == 1;
                let ability_key = format!(
                    "{}:{}",
                    key.identity.ability_id,
                    key.identity.hit_event_id.unwrap_or_default()
                );
                let ability = by_ability_hit.entry(ability_key).or_default();
                if key.axis == "source_current_hp" {
                    ability.source_hp_state_pairs = ability.source_hp_state_pairs.saturating_add(1);
                } else {
                    ability.target_hp_state_pairs = ability.target_hp_state_pairs.saturating_add(1);
                }
                let example = CurrentHpIsolationExample {
                    axis: key.axis,
                    session_id: key.identity.session_id.clone(),
                    run_ordinal: key.identity.run_ordinal,
                    source_entity_uuid: key.identity.source_entity_uuid,
                    target_entity_uuid: key.identity.target_entity_uuid,
                    ability_id: key.identity.ability_id,
                    hit_event_id: key.identity.hit_event_id,
                    left_current_hp: *left_hp,
                    right_current_hp: *right_hp,
                    opposite_current_hp: key.opposite_current_hp,
                    left_sequences: left_bucket.sequences.iter().copied().collect(),
                    right_sequences: right_bucket.sequences.iter().copied().collect(),
                    left_occurrences: left_bucket.occurrences,
                    right_occurrences: right_bucket.occurrences,
                    left_outcomes: left_outcomes.clone(),
                    right_outcomes: right_outcomes.clone(),
                };
                if deterministic && left_outcomes == right_outcomes {
                    counters.deterministic_equal_output_pairs =
                        counters.deterministic_equal_output_pairs.saturating_add(1);
                    if key.axis == "source_current_hp" {
                        ability.source_hp_deterministic_equal_output_pairs = ability
                            .source_hp_deterministic_equal_output_pairs
                            .saturating_add(1);
                    } else {
                        ability.target_hp_deterministic_equal_output_pairs = ability
                            .target_hp_deterministic_equal_output_pairs
                            .saturating_add(1);
                    }
                    if deterministic_equal_examples.len() < example_limit {
                        deterministic_equal_examples.push(example);
                    }
                } else if deterministic {
                    counters.deterministic_divergent_output_pairs = counters
                        .deterministic_divergent_output_pairs
                        .saturating_add(1);
                    if key.axis == "source_current_hp" {
                        ability.source_hp_deterministic_divergent_output_pairs = ability
                            .source_hp_deterministic_divergent_output_pairs
                            .saturating_add(1);
                    } else {
                        ability.target_hp_deterministic_divergent_output_pairs = ability
                            .target_hp_deterministic_divergent_output_pairs
                            .saturating_add(1);
                    }
                    if deterministic_divergent_examples.len() < example_limit {
                        deterministic_divergent_examples.push(example);
                    }
                } else {
                    counters.nondeterministic_or_partially_overlapping_pairs = counters
                        .nondeterministic_or_partially_overlapping_pairs
                        .saturating_add(1);
                    if key.axis == "source_current_hp" {
                        ability.source_hp_nondeterministic_or_partially_overlapping_pairs = ability
                            .source_hp_nondeterministic_or_partially_overlapping_pairs
                            .saturating_add(1);
                    } else {
                        ability.target_hp_nondeterministic_or_partially_overlapping_pairs = ability
                            .target_hp_nondeterministic_or_partially_overlapping_pairs
                            .saturating_add(1);
                    }
                }
            }
        }
    }
    CurrentHpIsolationDiagnostic {
        policy: CurrentHpIsolationPolicy {
            runtime_authority: false,
            calculation_control: "same complete packet calculation identity, exact non-CurrentHP source and target attributes, and exact source and target status states",
            state_control: "source CurrentHP is varied only while target CurrentHP is exact, or target CurrentHP is varied only while source CurrentHP is exact",
            deterministic_equal_interpretation: "an equal pair is exact output-invariance evidence only for the named ability/hit and exposed state range",
            promotion_boundary: "absence of divergent evidence does not prove global HP independence; HP-scaled skills remain formula-relevant and every divergent or nondeterministic pair is retained",
        },
        counters,
        by_ability_hit,
        deterministic_equal_examples,
        deterministic_divergent_examples,
    }
}

fn serialized_input_outcome_multiplicity_diagnostic(
    cohort: &FormulaCohort,
    example_limit: usize,
) -> SerializedInputOutcomeMultiplicityDiagnostic {
    let mut groups = HashMap::<
        SerializedInputOutcomeMultiplicityKey,
        SerializedInputOutcomeMultiplicityBucket,
    >::new();
    for sample in &cohort.samples {
        let bucket = groups
            .entry(SerializedInputOutcomeMultiplicityKey {
                identity: calculation_identity(sample),
                source_attribute_state_id: sample.source_attribute_state_id,
                target_attribute_state_id: sample.target_attribute_state_id,
                source_status_state_id: sample.source_status_state_id,
                target_status_state_id: sample.target_status_state_id,
            })
            .or_default();
        bucket.sequences.insert(sample.sequence);
        *bucket
            .outcomes
            .entry(StatusOutcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
    }

    let mut counters = SerializedInputOutcomeMultiplicityCounters {
        exact_serialized_input_groups: u64::try_from(groups.len()).unwrap_or(u64::MAX),
        ..Default::default()
    };
    let mut by_ability_hit =
        BTreeMap::<String, SerializedInputOutcomeMultiplicityAbilityCounters>::new();
    let mut multi_outcome_examples = Vec::new();
    for (key, bucket) in groups {
        let occurrences = bucket.outcomes.values().copied().sum::<u64>();
        if occurrences < 2 {
            continue;
        }
        counters.groups_with_multiple_occurrences =
            counters.groups_with_multiple_occurrences.saturating_add(1);
        counters.repeated_occurrences = counters.repeated_occurrences.saturating_add(occurrences);
        let ability_key = format!(
            "{}:{}",
            key.identity.ability_id,
            key.identity.hit_event_id.unwrap_or_default()
        );
        let ability = by_ability_hit.entry(ability_key).or_default();
        ability.groups_with_multiple_occurrences =
            ability.groups_with_multiple_occurrences.saturating_add(1);
        ability.repeated_occurrences = ability.repeated_occurrences.saturating_add(occurrences);
        if bucket.outcomes.len() < 2 {
            continue;
        }
        counters.groups_with_multiple_distinct_outcomes = counters
            .groups_with_multiple_distinct_outcomes
            .saturating_add(1);
        counters.repeated_occurrences_in_multi_outcome_groups = counters
            .repeated_occurrences_in_multi_outcome_groups
            .saturating_add(occurrences);
        ability.groups_with_multiple_distinct_outcomes = ability
            .groups_with_multiple_distinct_outcomes
            .saturating_add(1);
        multi_outcome_examples.push(SerializedInputOutcomeMultiplicityExample {
            session_id: key.identity.session_id,
            run_ordinal: key.identity.run_ordinal,
            source_entity_uuid: key.identity.source_entity_uuid,
            target_entity_uuid: key.identity.target_entity_uuid,
            ability_id: key.identity.ability_id,
            hit_event_id: key.identity.hit_event_id,
            source_attribute_state_id: key.source_attribute_state_id,
            target_attribute_state_id: key.target_attribute_state_id,
            source_status_state_id: key.source_status_state_id,
            target_status_state_id: key.target_status_state_id,
            sequences: bucket.sequences.into_iter().collect(),
            outcomes: bucket.outcomes,
        });
    }
    multi_outcome_examples.sort_by(|left, right| {
        left.session_id
            .cmp(&right.session_id)
            .then(left.run_ordinal.cmp(&right.run_ordinal))
            .then(left.source_entity_uuid.cmp(&right.source_entity_uuid))
            .then(left.target_entity_uuid.cmp(&right.target_entity_uuid))
            .then(left.ability_id.cmp(&right.ability_id))
            .then(left.hit_event_id.cmp(&right.hit_event_id))
    });
    multi_outcome_examples.truncate(example_limit);

    SerializedInputOutcomeMultiplicityDiagnostic {
        runtime_authority: false,
        exact_input_control: "same complete packet calculation identity plus the same interned source/target attribute and semantic status states; CurrentHP is not relaxed",
        interpretation_boundary: "multiple final outcomes under exact serialized inputs prove that the captured formula surface is not a deterministic complete input set. The missing cause may be a server damage roll or another unserialized input; this diagnostic does not name or model it, and shared hidden-factor comparisons are invalid for affected groups",
        counters,
        by_ability_hit,
        multi_outcome_examples,
    }
}

fn inspiration_composite_transition_diagnostic(
    cohort: &FormulaCohort,
    damage_surface: &DamageCorrelationSurface,
    target_status_stack_isolation: &TargetStatusStackIsolationDiagnostic,
    example_limit: usize,
) -> InspirationCompositeTransitionDiagnostic {
    let surface = damage_surface
        .observed_keys
        .iter()
        .filter(|key| key.match_status == "unique")
        .filter_map(|key| {
            key.unique_row
                .as_ref()
                .map(|row| ((key.ability_id, key.hit_event_id), row))
        })
        .collect::<BTreeMap<_, _>>();
    let inspiration_attribute_ids = inspiration_attribute_ids();
    let mut groups: HashMap<
        InspirationCompositeControlKey,
        HashMap<InspirationCompositeState, InspirationCompositeBucket>,
    > = HashMap::new();
    let mut loose_groups: HashMap<
        InspirationLooseControlKey,
        HashMap<InspirationLooseState, InspirationLooseBucket>,
    > = HashMap::new();
    let mut counters = InspirationCompositeCounters::default();

    for sample in &cohort.samples {
        if sample.packet.property.unwrap_or_default() != 7 {
            continue;
        }
        counters.eligible_light_property_samples =
            counters.eligible_light_property_samples.saturating_add(1);
        let Some(row) = surface.get(&(sample.ability_id, sample.hit_event_id.unwrap_or_default()))
        else {
            continue;
        };
        if row.damage_script != "Attack" && row.damage_script != "MAttack" {
            continue;
        }
        counters.samples_with_unique_standard_damage_row = counters
            .samples_with_unique_standard_damage_row
            .saturating_add(1);
        let Some(coefficient) =
            select_stage_coefficient(&row.pve_damage_ratio, sample.packet.owner_stage)
        else {
            continue;
        };
        let Some(fixed_parameter) =
            select_level_fixed_parameter(&row.pve_fixed_parameter, sample.packet.owner_level)
        else {
            continue;
        };
        counters.samples_with_exact_stage_coefficient_and_fixed_parameter = counters
            .samples_with_exact_stage_coefficient_and_fixed_parameter
            .saturating_add(1);

        let source_attributes = &cohort.attribute_states[sample.source_attribute_state_id];
        let target_attributes = &cohort.attribute_states[sample.target_attribute_state_id];
        let source_statuses = &cohort.status_states[sample.source_status_state_id];
        let (selected_statuses, source_statuses_without_inspiration) =
            partition_inspiration_statuses(source_statuses);
        if selected_statuses.is_empty() {
            counters.eligible_samples_without_inspiration_status = counters
                .eligible_samples_without_inspiration_status
                .saturating_add(1);
        } else {
            counters.eligible_samples_with_inspiration_status = counters
                .eligible_samples_with_inspiration_status
                .saturating_add(1);
        }
        let key = InspirationCompositeControlKey {
            identity: calculation_identity(sample),
            source_attributes_without_inspiration: attributes_without_current_hp_and_set(
                source_attributes,
                &inspiration_attribute_ids,
            ),
            target_attributes_without_current_hp: attributes_without_current_hp(target_attributes),
            source_statuses_without_inspiration,
            target_statuses: cohort.status_states[sample.target_status_state_id].clone(),
        };
        let state = InspirationCompositeState {
            selected_statuses: selected_statuses.clone(),
            vector_attributes: source_attributes
                .iter()
                .filter(|entry| inspiration_attribute_ids.contains(&entry.attribute_id))
                .cloned()
                .collect(),
            coefficient,
            fixed_parameter,
            damage_id: row.damage_id.clone(),
            damage_script: row.damage_script.clone(),
        };
        let loose_key = InspirationLooseControlKey {
            identity: calculation_identity(sample),
            coefficient,
            fixed_parameter,
            damage_id: row.damage_id.clone(),
            damage_script: row.damage_script.clone(),
        };
        let loose_state = InspirationLooseState {
            selected_statuses,
            vector_attributes: state.vector_attributes.clone(),
            source_attributes_without_inspiration: key
                .source_attributes_without_inspiration
                .clone(),
            target_attributes_without_current_hp: key.target_attributes_without_current_hp.clone(),
            source_statuses_without_inspiration: key.source_statuses_without_inspiration.clone(),
            target_statuses: key.target_statuses.clone(),
        };
        let loose_bucket = loose_groups
            .entry(loose_key)
            .or_default()
            .entry(loose_state)
            .or_default();
        loose_bucket.occurrences = loose_bucket.occurrences.saturating_add(1);
        *loose_bucket
            .outcomes
            .entry(StatusOutcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
        loose_bucket.sequences.insert(sample.sequence);
        loose_bucket
            .source_current_hp
            .insert(attribute_value(source_attributes, CURRENT_HP_ATTRIBUTE_ID));
        loose_bucket
            .target_current_hp
            .insert(attribute_value(target_attributes, CURRENT_HP_ATTRIBUTE_ID));
        let bucket = groups.entry(key).or_default().entry(state).or_default();
        bucket.occurrences = bucket.occurrences.saturating_add(1);
        *bucket
            .outcomes
            .entry(StatusOutcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
        bucket.sequences.insert(sample.sequence);
        bucket
            .source_current_hp
            .insert(attribute_value(source_attributes, CURRENT_HP_ATTRIBUTE_ID));
        bucket
            .target_current_hp
            .insert(attribute_value(target_attributes, CURRENT_HP_ATTRIBUTE_ID));
    }

    let mut models = INSPIRATION_COMPOSITE_MODELS
        .iter()
        .map(|model| {
            (
                (*model).to_owned(),
                InspirationCompositeModelCounters::default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut compatible_examples = Vec::new();
    let mut rejected_examples = Vec::new();
    for (key, states) in groups {
        if states.len() < 2 {
            continue;
        }
        counters.controlled_groups_with_multiple_vector_states = counters
            .controlled_groups_with_multiple_vector_states
            .saturating_add(1);
        let inactive = states
            .iter()
            .filter(|(state, _)| state.selected_statuses.is_empty())
            .collect::<Vec<_>>();
        let active = states
            .iter()
            .filter(|(state, _)| !state.selected_statuses.is_empty())
            .collect::<Vec<_>>();
        match (inactive.is_empty(), active.is_empty()) {
            (false, true) => {
                counters.controlled_groups_with_only_inactive_states = counters
                    .controlled_groups_with_only_inactive_states
                    .saturating_add(1)
            }
            (true, false) => {
                counters.controlled_groups_with_only_active_states = counters
                    .controlled_groups_with_only_active_states
                    .saturating_add(1)
            }
            (false, false) => {
                counters.controlled_groups_with_active_and_inactive_states = counters
                    .controlled_groups_with_active_and_inactive_states
                    .saturating_add(1)
            }
            (true, true) => {}
        }
        for (inactive_state, inactive_bucket) in &inactive {
            for (active_state, active_bucket) in &active {
                counters.active_inactive_vector_state_pairs = counters
                    .active_inactive_vector_state_pairs
                    .saturating_add(1);
                if inactive_bucket.outcomes.len() != 1 || active_bucket.outcomes.len() != 1 {
                    counters.nondeterministic_or_partially_overlapping_pairs = counters
                        .nondeterministic_or_partially_overlapping_pairs
                        .saturating_add(1);
                    continue;
                }
                counters.deterministic_active_inactive_pairs = counters
                    .deterministic_active_inactive_pairs
                    .saturating_add(1);
                let inactive_outcome = inactive_bucket.outcomes.keys().next().unwrap().clone();
                let active_outcome = active_bucket.outcomes.keys().next().unwrap().clone();
                for model in INSPIRATION_COMPOSITE_MODELS {
                    let inactive_body =
                        inspiration_composite_model_body(model, &key.identity, inactive_state);
                    let active_body =
                        inspiration_composite_model_body(model, &key.identity, active_state);
                    let inactive_interval = inactive_body.and_then(|body| {
                        fixed_point_factor_interval(inactive_outcome.amount, body)
                    });
                    let active_interval = active_body
                        .and_then(|body| fixed_point_factor_interval(active_outcome.amount, body));
                    let compatible = inactive_interval.zip(active_interval).and_then(
                        |(inactive_interval, active_interval)| {
                            let minimum = inactive_interval.0.max(active_interval.0);
                            let maximum = inactive_interval.1.min(active_interval.1);
                            (minimum <= maximum).then_some((minimum, maximum))
                        },
                    );
                    let model_counters = models.get_mut(*model).unwrap();
                    model_counters.evaluated_pairs =
                        model_counters.evaluated_pairs.saturating_add(1);
                    let example = InspirationCompositeExample {
                        model: (*model).to_owned(),
                        calculation_identity: key.identity.clone(),
                        session_id: key.identity.session_id.clone(),
                        run_ordinal: key.identity.run_ordinal,
                        source_entity_uuid: key.identity.source_entity_uuid,
                        target_entity_uuid: key.identity.target_entity_uuid,
                        ability_id: key.identity.ability_id,
                        hit_event_id: key.identity.hit_event_id,
                        inactive_sequences: inactive_bucket.sequences.iter().copied().collect(),
                        active_sequences: active_bucket.sequences.iter().copied().collect(),
                        damage_id: active_state.damage_id.clone(),
                        damage_script: active_state.damage_script.clone(),
                        coefficient: active_state.coefficient,
                        fixed_parameter: active_state.fixed_parameter,
                        inactive_vector_attributes: inactive_state.vector_attributes.clone(),
                        active_vector_attributes: active_state.vector_attributes.clone(),
                        inactive_outcome: inactive_outcome.clone(),
                        active_outcome: active_outcome.clone(),
                        inactive_body,
                        active_body,
                        inactive_later_factor_minimum: inactive_interval.map(|value| value.0),
                        inactive_later_factor_maximum: inactive_interval.map(|value| value.1),
                        active_later_factor_minimum: active_interval.map(|value| value.0),
                        active_later_factor_maximum: active_interval.map(|value| value.1),
                        compatible_later_factor_minimum: compatible.map(|value| value.0),
                        compatible_later_factor_maximum: compatible.map(|value| value.1),
                        inactive_source_current_hp: inactive_bucket
                            .source_current_hp
                            .iter()
                            .copied()
                            .collect(),
                        active_source_current_hp: active_bucket
                            .source_current_hp
                            .iter()
                            .copied()
                            .collect(),
                        inactive_target_current_hp: inactive_bucket
                            .target_current_hp
                            .iter()
                            .copied()
                            .collect(),
                        active_target_current_hp: active_bucket
                            .target_current_hp
                            .iter()
                            .copied()
                            .collect(),
                    };
                    if compatible.is_some() {
                        model_counters.compatible_pairs =
                            model_counters.compatible_pairs.saturating_add(1);
                        if compatible_examples.len() < example_limit {
                            compatible_examples.push(example);
                        }
                    } else {
                        model_counters.rejected_pairs =
                            model_counters.rejected_pairs.saturating_add(1);
                        if rejected_examples.len() < example_limit {
                            rejected_examples.push(example);
                        }
                    }
                }
            }
        }
    }

    let nearest_active_inactive_mismatches = inspiration_nearest_mismatch_diagnostic(
        loose_groups,
        target_status_stack_isolation,
        example_limit,
    );

    InspirationCompositeTransitionDiagnostic {
        policy: InspirationCompositePolicy {
            runtime_authority: false,
            calculation_control: "same complete packet calculation identity, including source, target, ability, hit, outcome flags, property, level, stage, and component identity",
            source_attribute_control: "all source attributes equal except CurrentHP and the complete packet-observed Inspiration direct and derived attribute families",
            target_attribute_control: "all target attributes equal except CurrentHP",
            status_control: "all source statuses equal except exact effect 2202041 and all target statuses equal",
            damage_surface_control: "exact-build unique DamageAttr row with standard Attack or MAttack script, exact stage coefficient, and exact level fixed parameter",
            tested_stage_models: INSPIRATION_COMPOSITE_MODELS.to_vec(),
            interpretation_boundary: "a compatible shared later integer factor means only that the model survives this controlled witness; rejection disproves that model for the witness; no model becomes runtime authority automatically",
        },
        counters,
        models,
        compatible_examples,
        rejected_examples,
        nearest_active_inactive_mismatches,
    }
}

fn inspiration_nearest_mismatch_diagnostic(
    groups: HashMap<
        InspirationLooseControlKey,
        HashMap<InspirationLooseState, InspirationLooseBucket>,
    >,
    target_status_stack_isolation: &TargetStatusStackIsolationDiagnostic,
    example_limit: usize,
) -> InspirationNearestMismatchDiagnostic {
    let mut counters = InspirationNearestMismatchCounters::default();
    let mut mismatch_score_histogram = BTreeMap::new();
    let mut source_attribute_counts = BTreeMap::<i32, u64>::new();
    let mut target_attribute_counts = BTreeMap::<i32, u64>::new();
    let mut source_status_counts = BTreeMap::<(bool, StatusEntry), u64>::new();
    let mut target_status_counts = BTreeMap::<(bool, StatusEntry), u64>::new();
    let mut scoped_target_stack_models = SCOPED_INSPIRATION_MODELS
        .iter()
        .map(|model| {
            (
                (*model).to_owned(),
                InspirationCompositeModelCounters::default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut scoped_compatible_examples = Vec::new();
    let mut scoped_rejected_examples = Vec::new();
    let mut examples = Vec::new();

    counters.loose_control_groups = u64::try_from(groups.len()).unwrap_or(u64::MAX);
    for (key, states) in groups {
        let mut inactive = states
            .iter()
            .filter(|(state, _)| state.selected_statuses.is_empty())
            .collect::<Vec<_>>();
        let mut active = states
            .iter()
            .filter(|(state, _)| !state.selected_statuses.is_empty())
            .collect::<Vec<_>>();
        if inactive.is_empty() || active.is_empty() {
            continue;
        }
        counters.loose_control_groups_with_active_and_inactive_states = counters
            .loose_control_groups_with_active_and_inactive_states
            .saturating_add(1);
        inactive.sort_by(|left, right| left.0.cmp(right.0));
        active.sort_by(|left, right| left.0.cmp(right.0));

        let mut best: Option<(usize, u64, usize, usize)> = None;
        for (inactive_index, (inactive_state, inactive_bucket)) in inactive.iter().enumerate() {
            for (active_index, (active_state, active_bucket)) in active.iter().enumerate() {
                let score = inspiration_unexpected_mismatch_score(inactive_state, active_state);
                let sequence_gap =
                    minimum_sequence_gap(&inactive_bucket.sequences, &active_bucket.sequences);
                let candidate = (score, sequence_gap, inactive_index, active_index);
                if best.as_ref().is_none_or(|current| candidate < *current) {
                    best = Some(candidate);
                }
            }
        }
        let Some((mismatch_score, _, inactive_index, active_index)) = best else {
            continue;
        };
        counters.nearest_pairs = counters.nearest_pairs.saturating_add(1);
        if mismatch_score == 0 {
            counters.nearest_pairs_without_unexpected_differences = counters
                .nearest_pairs_without_unexpected_differences
                .saturating_add(1);
        }
        *mismatch_score_histogram.entry(mismatch_score).or_default() += 1;

        let (inactive_state, inactive_bucket) = inactive[inactive_index];
        let (active_state, active_bucket) = active[active_index];
        let source_attribute_changes = attribute_changes(
            &inactive_state.source_attributes_without_inspiration,
            &active_state.source_attributes_without_inspiration,
        );
        let target_attribute_changes = attribute_changes(
            &inactive_state.target_attributes_without_current_hp,
            &active_state.target_attributes_without_current_hp,
        );
        let (source_status_removed, source_status_added) = status_changes(
            &inactive_state.source_statuses_without_inspiration,
            &active_state.source_statuses_without_inspiration,
        );
        let (target_status_removed, target_status_added) = status_changes(
            &inactive_state.target_statuses,
            &active_state.target_statuses,
        );
        let inactive_outcomes = inactive_bucket.outcomes.keys().cloned().collect::<Vec<_>>();
        let active_outcomes = active_bucket.outcomes.keys().cloned().collect::<Vec<_>>();
        let source_current_hp_differs =
            inactive_bucket.source_current_hp != active_bucket.source_current_hp;
        let target_current_hp_differs =
            inactive_bucket.target_current_hp != active_bucket.target_current_hp;
        let scoped_target_stack_invariance = source_attribute_changes.is_empty()
            && target_attribute_changes.is_empty()
            && source_status_removed.is_empty()
            && source_status_added.is_empty()
            && target_stack_change_is_scoped_invariant(
                &key.identity,
                &target_status_removed,
                &target_status_added,
                target_status_stack_isolation,
            );
        let logical_active_light_from_mastery_delta = logical_light_from_mastery_delta(
            &inactive_state.vector_attributes,
            &active_state.vector_attributes,
        );

        if scoped_target_stack_invariance {
            counters.nearest_pairs_with_scoped_target_stack_invariance = counters
                .nearest_pairs_with_scoped_target_stack_invariance
                .saturating_add(1);
            if source_current_hp_differs {
                counters.scoped_pairs_with_source_current_hp_difference = counters
                    .scoped_pairs_with_source_current_hp_difference
                    .saturating_add(1);
            }
            if target_current_hp_differs {
                counters.scoped_pairs_with_target_current_hp_difference = counters
                    .scoped_pairs_with_target_current_hp_difference
                    .saturating_add(1);
            }
            if inactive_outcomes.len() == 1 && active_outcomes.len() == 1 {
                counters.scoped_pairs_with_deterministic_outcomes = counters
                    .scoped_pairs_with_deterministic_outcomes
                    .saturating_add(1);
                let inactive_outcome = inactive_outcomes[0].clone();
                let active_outcome = active_outcomes[0].clone();
                let inactive_composite_state = InspirationCompositeState {
                    selected_statuses: inactive_state.selected_statuses.clone(),
                    vector_attributes: inactive_state.vector_attributes.clone(),
                    coefficient: key.coefficient,
                    fixed_parameter: key.fixed_parameter,
                    damage_id: key.damage_id.clone(),
                    damage_script: key.damage_script.clone(),
                };
                let active_composite_state = InspirationCompositeState {
                    selected_statuses: active_state.selected_statuses.clone(),
                    vector_attributes: active_state.vector_attributes.clone(),
                    coefficient: key.coefficient,
                    fixed_parameter: key.fixed_parameter,
                    damage_id: key.damage_id.clone(),
                    damage_script: key.damage_script.clone(),
                };
                for model in SCOPED_INSPIRATION_MODELS {
                    let inactive_body = scoped_inspiration_model_body(
                        model,
                        &key.identity,
                        &inactive_composite_state,
                        None,
                    );
                    let active_body = scoped_inspiration_model_body(
                        model,
                        &key.identity,
                        &active_composite_state,
                        logical_active_light_from_mastery_delta,
                    );
                    let inactive_interval = inactive_body.and_then(|body| {
                        fixed_point_factor_interval(inactive_outcome.amount, body)
                    });
                    let active_interval = active_body
                        .and_then(|body| fixed_point_factor_interval(active_outcome.amount, body));
                    let compatible = inactive_interval.zip(active_interval).and_then(
                        |(inactive_interval, active_interval)| {
                            let minimum = inactive_interval.0.max(active_interval.0);
                            let maximum = inactive_interval.1.min(active_interval.1);
                            (minimum <= maximum).then_some((minimum, maximum))
                        },
                    );
                    let model_counters = scoped_target_stack_models.get_mut(*model).unwrap();
                    model_counters.evaluated_pairs =
                        model_counters.evaluated_pairs.saturating_add(1);
                    let example = InspirationCompositeExample {
                        model: (*model).to_owned(),
                        calculation_identity: key.identity.clone(),
                        session_id: key.identity.session_id.clone(),
                        run_ordinal: key.identity.run_ordinal,
                        source_entity_uuid: key.identity.source_entity_uuid,
                        target_entity_uuid: key.identity.target_entity_uuid,
                        ability_id: key.identity.ability_id,
                        hit_event_id: key.identity.hit_event_id,
                        inactive_sequences: inactive_bucket.sequences.iter().copied().collect(),
                        active_sequences: active_bucket.sequences.iter().copied().collect(),
                        damage_id: key.damage_id.clone(),
                        damage_script: key.damage_script.clone(),
                        coefficient: key.coefficient,
                        fixed_parameter: key.fixed_parameter,
                        inactive_vector_attributes: inactive_state.vector_attributes.clone(),
                        active_vector_attributes: active_state.vector_attributes.clone(),
                        inactive_outcome: inactive_outcome.clone(),
                        active_outcome: active_outcome.clone(),
                        inactive_body,
                        active_body,
                        inactive_later_factor_minimum: inactive_interval.map(|value| value.0),
                        inactive_later_factor_maximum: inactive_interval.map(|value| value.1),
                        active_later_factor_minimum: active_interval.map(|value| value.0),
                        active_later_factor_maximum: active_interval.map(|value| value.1),
                        compatible_later_factor_minimum: compatible.map(|value| value.0),
                        compatible_later_factor_maximum: compatible.map(|value| value.1),
                        inactive_source_current_hp: inactive_bucket
                            .source_current_hp
                            .iter()
                            .copied()
                            .collect(),
                        active_source_current_hp: active_bucket
                            .source_current_hp
                            .iter()
                            .copied()
                            .collect(),
                        inactive_target_current_hp: inactive_bucket
                            .target_current_hp
                            .iter()
                            .copied()
                            .collect(),
                        active_target_current_hp: active_bucket
                            .target_current_hp
                            .iter()
                            .copied()
                            .collect(),
                    };
                    if compatible.is_some() {
                        model_counters.compatible_pairs =
                            model_counters.compatible_pairs.saturating_add(1);
                        if scoped_compatible_examples.len() < example_limit {
                            scoped_compatible_examples.push(example);
                        }
                    } else {
                        model_counters.rejected_pairs =
                            model_counters.rejected_pairs.saturating_add(1);
                        if scoped_rejected_examples.len() < example_limit {
                            scoped_rejected_examples.push(example);
                        }
                    }
                }
            }
        }

        for change in &source_attribute_changes {
            *source_attribute_counts
                .entry(change.attribute_id)
                .or_default() += 1;
        }
        for change in &target_attribute_changes {
            *target_attribute_counts
                .entry(change.attribute_id)
                .or_default() += 1;
        }
        for status in &source_status_removed {
            *source_status_counts
                .entry((true, status.clone()))
                .or_default() += 1;
        }
        for status in &source_status_added {
            *source_status_counts
                .entry((false, status.clone()))
                .or_default() += 1;
        }
        for status in &target_status_removed {
            *target_status_counts
                .entry((true, status.clone()))
                .or_default() += 1;
        }
        for status in &target_status_added {
            *target_status_counts
                .entry((false, status.clone()))
                .or_default() += 1;
        }

        examples.push(InspirationNearestMismatchExample {
            calculation_identity: key.identity.clone(),
            session_id: key.identity.session_id.clone(),
            run_ordinal: key.identity.run_ordinal,
            source_entity_uuid: key.identity.source_entity_uuid,
            target_entity_uuid: key.identity.target_entity_uuid,
            ability_id: key.identity.ability_id,
            hit_event_id: key.identity.hit_event_id,
            damage_id: key.damage_id.clone(),
            damage_script: key.damage_script.clone(),
            coefficient: key.coefficient,
            fixed_parameter: key.fixed_parameter,
            mismatch_score,
            inactive_sequences: inactive_bucket.sequences.iter().copied().collect(),
            active_sequences: active_bucket.sequences.iter().copied().collect(),
            inactive_vector_attributes: inactive_state.vector_attributes.clone(),
            active_vector_attributes: active_state.vector_attributes.clone(),
            inactive_control_source_attributes: inactive_state
                .source_attributes_without_inspiration
                .clone(),
            active_control_source_attributes: active_state
                .source_attributes_without_inspiration
                .clone(),
            inactive_control_target_attributes: inactive_state
                .target_attributes_without_current_hp
                .clone(),
            active_control_target_attributes: active_state
                .target_attributes_without_current_hp
                .clone(),
            inactive_occurrences: inactive_bucket.occurrences,
            active_occurrences: active_bucket.occurrences,
            inactive_outcomes,
            active_outcomes,
            inactive_source_current_hp: inactive_bucket.source_current_hp.iter().copied().collect(),
            active_source_current_hp: active_bucket.source_current_hp.iter().copied().collect(),
            inactive_target_current_hp: inactive_bucket.target_current_hp.iter().copied().collect(),
            active_target_current_hp: active_bucket.target_current_hp.iter().copied().collect(),
            scoped_target_stack_invariance,
            source_current_hp_differs,
            target_current_hp_differs,
            logical_active_light_from_mastery_delta,
            source_attribute_changes,
            target_attribute_changes,
            source_status_removed,
            source_status_added,
            target_status_removed,
            target_status_added,
        });
    }

    let mut source_attribute_mismatches = source_attribute_counts
        .into_iter()
        .map(
            |(attribute_id, nearest_group_count)| InspirationAttributeMismatchCount {
                attribute_id,
                nearest_group_count,
            },
        )
        .collect::<Vec<_>>();
    let mut target_attribute_mismatches = target_attribute_counts
        .into_iter()
        .map(
            |(attribute_id, nearest_group_count)| InspirationAttributeMismatchCount {
                attribute_id,
                nearest_group_count,
            },
        )
        .collect::<Vec<_>>();
    let mut source_status_mismatches = source_status_counts
        .into_iter()
        .map(
            |((removed, status), nearest_group_count)| InspirationStatusMismatchCount {
                direction: if removed { "removed" } else { "added" },
                status,
                nearest_group_count,
            },
        )
        .collect::<Vec<_>>();
    let mut target_status_mismatches = target_status_counts
        .into_iter()
        .map(
            |((removed, status), nearest_group_count)| InspirationStatusMismatchCount {
                direction: if removed { "removed" } else { "added" },
                status,
                nearest_group_count,
            },
        )
        .collect::<Vec<_>>();
    source_attribute_mismatches.sort_by(|left, right| {
        right
            .nearest_group_count
            .cmp(&left.nearest_group_count)
            .then(left.attribute_id.cmp(&right.attribute_id))
    });
    target_attribute_mismatches.sort_by(|left, right| {
        right
            .nearest_group_count
            .cmp(&left.nearest_group_count)
            .then(left.attribute_id.cmp(&right.attribute_id))
    });
    source_status_mismatches.sort_by(|left, right| {
        right
            .nearest_group_count
            .cmp(&left.nearest_group_count)
            .then(left.status.cmp(&right.status))
            .then(left.direction.cmp(right.direction))
    });
    target_status_mismatches.sort_by(|left, right| {
        right
            .nearest_group_count
            .cmp(&left.nearest_group_count)
            .then(left.status.cmp(&right.status))
            .then(left.direction.cmp(right.direction))
    });
    examples.sort_by(|left, right| {
        left.mismatch_score
            .cmp(&right.mismatch_score)
            .then(left.session_id.cmp(&right.session_id))
            .then(left.run_ordinal.cmp(&right.run_ordinal))
            .then(left.source_entity_uuid.cmp(&right.source_entity_uuid))
            .then(left.target_entity_uuid.cmp(&right.target_entity_uuid))
            .then(left.ability_id.cmp(&right.ability_id))
            .then(left.hit_event_id.cmp(&right.hit_event_id))
    });
    examples.truncate(example_limit);

    InspirationNearestMismatchDiagnostic {
        runtime_authority: false,
        comparison_control: "same complete packet calculation identity and exact DamageAttr coefficient/fixed row; Inspiration's complete direct/derived vector and effect 2202041 may differ, while every other attribute and full status entry is counted as a mismatch",
        interpretation_boundary: "only score zero is a strict isolated Inspiration transition; a nonzero target-stack mismatch may enter the separately labeled scoped diagnostic only when the exact ability/hit family has deterministic equal-output stack-change evidence and no divergent or nondeterministic evidence. CurrentHP differences remain explicit formula blockers, every emitted proc remains counted, and no scoped model is runtime authority",
        counters,
        mismatch_score_histogram,
        source_attribute_mismatches,
        target_attribute_mismatches,
        source_status_mismatches,
        target_status_mismatches,
        scoped_target_stack_models,
        scoped_compatible_examples,
        scoped_rejected_examples,
        examples,
    }
}

fn inspiration_unexpected_mismatch_score(
    inactive: &InspirationLooseState,
    active: &InspirationLooseState,
) -> usize {
    attribute_changes(
        &inactive.source_attributes_without_inspiration,
        &active.source_attributes_without_inspiration,
    )
    .len()
        + attribute_changes(
            &inactive.target_attributes_without_current_hp,
            &active.target_attributes_without_current_hp,
        )
        .len()
        + status_change_count(
            &inactive.source_statuses_without_inspiration,
            &active.source_statuses_without_inspiration,
        )
        + status_change_count(&inactive.target_statuses, &active.target_statuses)
}

fn target_stack_change_is_scoped_invariant(
    identity: &CalculationIdentity,
    removed: &[StatusEntry],
    added: &[StatusEntry],
    isolation: &TargetStatusStackIsolationDiagnostic,
) -> bool {
    if removed.len() != 1 || added.len() != 1 {
        return false;
    }
    let left = &removed[0];
    let right = &added[0];
    if left.effect_id != isolation.policy.selected_effect_id
        || right.effect_id != isolation.policy.selected_effect_id
        || left.source_entity_uuid != right.source_entity_uuid
        || left.level != right.level
        || left.origin_source_type_id != right.origin_source_type_id
        || left.origin_source_config_id != right.origin_source_config_id
        || left.stacks == right.stacks
    {
        return false;
    }
    let ability_key = format!(
        "{}:{}",
        identity.ability_id,
        identity.hit_event_id.unwrap_or_default()
    );
    isolation
        .by_ability_hit
        .get(&ability_key)
        .is_some_and(|evidence| {
            evidence.deterministic_equal_output_pairs > 0
                && evidence.deterministic_divergent_output_pairs == 0
                && evidence.nondeterministic_or_partially_overlapping_pairs == 0
        })
}

fn logical_light_from_mastery_delta(
    inactive_attributes: &[AttributeEntry],
    active_attributes: &[AttributeEntry],
) -> Option<i64> {
    let inactive_mastery = attribute_value(inactive_attributes, MASTERY_ATTRIBUTE_ID)?;
    let active_mastery = attribute_value(active_attributes, MASTERY_ATTRIBUTE_ID)?;
    let inactive_light = attribute_value(inactive_attributes, LIGHT_DAMAGE_ATTRIBUTE_ID)?;
    let mastery_delta = i128::from(active_mastery).checked_sub(i128::from(inactive_mastery))?;
    let light_delta = mastery_delta.checked_mul(60)?.div_euclid(100);
    i64::try_from(i128::from(inactive_light).checked_add(light_delta)?).ok()
}

fn scoped_inspiration_model_body(
    model: &str,
    identity: &CalculationIdentity,
    state: &InspirationCompositeState,
    logical_light_override: Option<i64>,
) -> Option<i64> {
    let (base_model, use_logical_light) = match model {
        "attack_coefficient_plus_fixed" => ("attack_coefficient_plus_fixed", false),
        "attack_hit_outcome" => ("attack_hit_outcome", false),
        "external_then_serialized_light" => ("external_then_light", false),
        "serialized_light_then_external" => ("light_then_external", false),
        "external_plus_serialized_light_single_bucket" => {
            ("external_plus_light_single_bucket", false)
        }
        "external_serialized_light_product_single_floor" => {
            ("external_light_product_single_floor", false)
        }
        "external_then_logical_light_from_mastery_delta" => ("external_then_light", true),
        "logical_light_from_mastery_delta_then_external" => ("light_then_external", true),
        "external_plus_logical_light_single_bucket" => ("external_plus_light_single_bucket", true),
        "external_logical_light_product_single_floor" => {
            ("external_light_product_single_floor", true)
        }
        _ => return None,
    };
    if !use_logical_light {
        return inspiration_composite_model_body(base_model, identity, state);
    }
    let mut adjusted = state.clone();
    let logical_light = logical_light_override
        .or_else(|| attribute_value(&state.vector_attributes, LIGHT_DAMAGE_ATTRIBUTE_ID))?;
    let light = adjusted
        .vector_attributes
        .iter_mut()
        .find(|entry| entry.attribute_id == LIGHT_DAMAGE_ATTRIBUTE_ID)?;
    light.value = logical_light;
    inspiration_composite_model_body(base_model, identity, &adjusted)
}

fn attribute_changes(
    inactive: &[AttributeEntry],
    active: &[AttributeEntry],
) -> Vec<InspirationAttributeChange> {
    let inactive = inactive
        .iter()
        .map(|entry| (entry.attribute_id, entry.value))
        .collect::<BTreeMap<_, _>>();
    let active = active
        .iter()
        .map(|entry| (entry.attribute_id, entry.value))
        .collect::<BTreeMap<_, _>>();
    inactive
        .keys()
        .chain(active.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|attribute_id| {
            let inactive_value = inactive.get(&attribute_id).copied();
            let active_value = active.get(&attribute_id).copied();
            (inactive_value != active_value).then_some(InspirationAttributeChange {
                attribute_id,
                inactive_value,
                active_value,
            })
        })
        .collect()
}

fn status_changes(
    inactive: &[StatusEntry],
    active: &[StatusEntry],
) -> (Vec<StatusEntry>, Vec<StatusEntry>) {
    let inactive = inactive.iter().cloned().collect::<BTreeSet<_>>();
    let active = active.iter().cloned().collect::<BTreeSet<_>>();
    (
        inactive.difference(&active).cloned().collect(),
        active.difference(&inactive).cloned().collect(),
    )
}

fn status_change_count(inactive: &[StatusEntry], active: &[StatusEntry]) -> usize {
    let (removed, added) = status_changes(inactive, active);
    removed.len() + added.len()
}

fn minimum_sequence_gap(left: &BTreeSet<u64>, right: &BTreeSet<u64>) -> u64 {
    left.iter()
        .flat_map(|left| right.iter().map(move |right| left.abs_diff(*right)))
        .min()
        .unwrap_or(u64::MAX)
}

fn inspiration_attribute_ids() -> BTreeSet<i32> {
    INSPIRATION_DIRECT_ATTRIBUTE_IDS
        .iter()
        .chain(INSPIRATION_DERIVED_ATTRIBUTE_IDS)
        .copied()
        .collect()
}

fn attributes_without_current_hp_and_set(
    attributes: &[AttributeEntry],
    ignored: &BTreeSet<i32>,
) -> Vec<AttributeEntry> {
    attributes
        .iter()
        .filter(|entry| {
            entry.attribute_id != CURRENT_HP_ATTRIBUTE_ID && !ignored.contains(&entry.attribute_id)
        })
        .cloned()
        .collect()
}

fn partition_inspiration_statuses(
    statuses: &[StatusEntry],
) -> (Vec<StatusEntry>, Vec<StatusEntry>) {
    statuses
        .iter()
        .cloned()
        .partition(|status| status.effect_id == INSPIRATION_EFFECT_ID)
}

fn select_stage_coefficient(coefficients: &[i64], owner_stage: Option<i32>) -> Option<i64> {
    if coefficients.len() == 1 {
        return (owner_stage.unwrap_or_default() >= 0)
            .then_some(coefficients[0])
            .filter(|value| *value > 0);
    }
    let index = usize::try_from(owner_stage.unwrap_or_default()).ok()?;
    coefficients.get(index).copied().filter(|value| *value > 0)
}

fn select_level_fixed_parameter(parameters: &[i64], owner_level: Option<i32>) -> Option<i64> {
    if parameters.is_empty() {
        return Some(0);
    }
    let level = usize::try_from(owner_level?).ok()?;
    level
        .checked_sub(1)
        .and_then(|index| parameters.get(index))
        .copied()
}

fn inspiration_composite_model_body(
    model: &str,
    identity: &CalculationIdentity,
    state: &InspirationCompositeState,
) -> Option<i64> {
    let attack_attribute_id = match state.damage_script.as_str() {
        "Attack" => ATTACK_ATTRIBUTE_ID,
        "MAttack" => MAGIC_ATTACK_ATTRIBUTE_ID,
        _ => return None,
    };
    let attack = attribute_value(&state.vector_attributes, attack_attribute_id)?;
    let base =
        mul_div_floor(attack, state.coefficient, 10_000)?.checked_add(state.fixed_parameter)?;
    if model == "attack_coefficient_plus_fixed" {
        return Some(base);
    }
    let mut hit = base;
    if identity.critical == Some(true) {
        hit = mul_div_floor(
            hit,
            10_000_i64.checked_add(attribute_value(
                &state.vector_attributes,
                CRITICAL_DAMAGE_ATTRIBUTE_ID,
            )?)?,
            10_000,
        )?;
    }
    if identity.lucky == Some(true) {
        hit = mul_div_floor(
            hit,
            attribute_value(&state.vector_attributes, LUCKY_DAMAGE_ATTRIBUTE_ID)?,
            10_000,
        )?;
    }
    if model == "attack_hit_outcome" {
        return Some(hit);
    }
    let external = attribute_value(&state.vector_attributes, EXTERNAL_DAMAGE_ATTRIBUTE_ID)?;
    let light = attribute_value(&state.vector_attributes, LIGHT_DAMAGE_ATTRIBUTE_ID)?;
    match model {
        "external_then_light" => mul_div_floor(
            mul_div_floor(hit, 10_000_i64.checked_add(external)?, 10_000)?,
            10_000_i64.checked_add(light)?,
            10_000,
        ),
        "light_then_external" => mul_div_floor(
            mul_div_floor(hit, 10_000_i64.checked_add(light)?, 10_000)?,
            10_000_i64.checked_add(external)?,
            10_000,
        ),
        "external_plus_light_single_bucket" => mul_div_floor(
            hit,
            10_000_i64.checked_add(external)?.checked_add(light)?,
            10_000,
        ),
        "external_light_product_single_floor" => {
            let numerator = i128::from(hit)
                .checked_mul(i128::from(10_000_i64.checked_add(external)?))?
                .checked_mul(i128::from(10_000_i64.checked_add(light)?))?;
            i64::try_from(numerator.checked_div(100_000_000)?).ok()
        }
        _ => None,
    }
}

fn mul_div_floor(value: i64, numerator: i64, denominator: i64) -> Option<i64> {
    if value < 0 || numerator <= 0 || denominator <= 0 {
        return None;
    }
    i64::try_from(
        i128::from(value)
            .checked_mul(i128::from(numerator))?
            .checked_div(i128::from(denominator))?,
    )
    .ok()
}

fn fixed_point_factor_interval(output: i64, body: i64) -> Option<(i64, i64)> {
    if output < 0 || body <= 0 {
        return None;
    }
    let minimum = ceil_div(i128::from(output).checked_mul(10_000)?, i128::from(body))?;
    let maximum = ceil_div(
        i128::from(output.checked_add(1)?).checked_mul(10_000)?,
        i128::from(body),
    )?
    .checked_sub(1)?;
    (minimum <= maximum)
        .then(|| Some((i64::try_from(minimum).ok()?, i64::try_from(maximum).ok()?)))?
}

fn attribute_stage_isolation_diagnostic(
    cohort: &FormulaCohort,
    example_limit: usize,
) -> AttributeStageIsolationDiagnostic {
    let axes = [
        (
            "external_damage_11840",
            EXTERNAL_DAMAGE_ATTRIBUTE_ID,
            (EXTERNAL_DAMAGE_ATTRIBUTE_ID..=EXTERNAL_DAMAGE_ATTRIBUTE_ID + 5).collect::<Vec<_>>(),
        ),
        (
            "light_damage_13170",
            LIGHT_DAMAGE_ATTRIBUTE_ID,
            (LIGHT_DAMAGE_ATTRIBUTE_ID..=LIGHT_DAMAGE_ATTRIBUTE_ID + 5).collect::<Vec<_>>(),
        ),
    ];
    let axes = axes
        .into_iter()
        .map(|(name, current_attribute_id, family_attribute_ids)| {
            (
                name.to_owned(),
                attribute_stage_isolation_axis(
                    cohort,
                    current_attribute_id,
                    family_attribute_ids,
                    example_limit,
                ),
            )
        })
        .collect();

    AttributeStageIsolationDiagnostic {
        policy: AttributeStageIsolationPolicy {
            runtime_authority: false,
            calculation_control: "packet calculation identity, source and target entity, hit flags, property, owner level/stage, and skill-effect component identity are exact",
            state_control: "source and target statuses are exact; every packet attribute is exact except volatile CurrentHP 11310 and the one audited six-member attribute family; all relaxed HP values and both complete family states remain in each witness",
            exact_model_tested: "for deterministic pairs, test whether one shared non-negative integer pre-factor value can exactly produce both packet amounts through floor(base*(10000+current_axis_raw)/10000)",
            rejection_boundary: "a rejected pair disproves only this independent final-stage placement for that controlled witness; it does not prove that the attribute is inactive, because additive bucket composition, earlier placement, later rounding, or another packet-unexposed stage may remain",
        },
        axes,
    }
}

fn attribute_stage_isolation_axis(
    cohort: &FormulaCohort,
    current_attribute_id: i32,
    family_attribute_ids: Vec<i32>,
    example_limit: usize,
) -> AttributeStageIsolationAxis {
    type ControlledKey = (
        CalculationIdentity,
        usize,
        usize,
        Vec<AttributeEntry>,
        Vec<AttributeEntry>,
    );

    let family = family_attribute_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut groups: HashMap<ControlledKey, HashMap<Vec<AttributeEntry>, StatusOutcomeBucket>> =
        HashMap::new();
    for sample in &cohort.samples {
        let source_attributes = &cohort.attribute_states[sample.source_attribute_state_id];
        let target_attributes = &cohort.attribute_states[sample.target_attribute_state_id];
        let axis_state = source_attributes
            .iter()
            .filter(|entry| family.contains(&entry.attribute_id))
            .cloned()
            .collect::<Vec<_>>();
        if attribute_value(&axis_state, current_attribute_id).is_none() {
            continue;
        }
        let bucket = groups
            .entry((
                calculation_identity(sample),
                sample.source_status_state_id,
                sample.target_status_state_id,
                attributes_without_current_hp_and_family(source_attributes, &family),
                attributes_without_current_hp(target_attributes),
            ))
            .or_default()
            .entry(axis_state)
            .or_default();
        bucket.occurrences = bucket.occurrences.saturating_add(1);
        bucket
            .source_current_hp
            .insert(attribute_value(source_attributes, CURRENT_HP_ATTRIBUTE_ID));
        bucket
            .target_current_hp
            .insert(attribute_value(target_attributes, CURRENT_HP_ATTRIBUTE_ID));
        *bucket
            .outcomes
            .entry(StatusOutcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
    }

    let mut counters = AttributeStageIsolationCounters::default();
    let mut exact_examples = Vec::new();
    let mut rejected_examples = Vec::new();
    for ((identity, _, _, _, _), states) in groups {
        if states.len() < 2 {
            continue;
        }
        counters.controlled_groups = counters.controlled_groups.saturating_add(1);
        let mut states = states.into_iter().collect::<Vec<_>>();
        states.sort_by(|left, right| left.0.cmp(&right.0));
        for left_index in 0..states.len() {
            for right_index in left_index.saturating_add(1)..states.len() {
                let (left_state, left) = &states[left_index];
                let (right_state, right) = &states[right_index];
                let Some(left_raw) = attribute_value(left_state, current_attribute_id) else {
                    continue;
                };
                let Some(right_raw) = attribute_value(right_state, current_attribute_id) else {
                    continue;
                };
                if left_raw == right_raw {
                    continue;
                }
                counters.distinct_axis_state_pairs =
                    counters.distinct_axis_state_pairs.saturating_add(1);
                let left_outcomes = left.outcomes.keys().cloned().collect::<Vec<_>>();
                let right_outcomes = right.outcomes.keys().cloned().collect::<Vec<_>>();
                if left_outcomes.len() != 1 || right_outcomes.len() != 1 {
                    counters.nondeterministic_or_partially_overlapping_pairs = counters
                        .nondeterministic_or_partially_overlapping_pairs
                        .saturating_add(1);
                    continue;
                }
                counters.deterministic_state_pairs =
                    counters.deterministic_state_pairs.saturating_add(1);
                if left_outcomes == right_outcomes {
                    counters.equal_output_pairs = counters.equal_output_pairs.saturating_add(1);
                } else {
                    counters.divergent_output_pairs =
                        counters.divergent_output_pairs.saturating_add(1);
                }
                let compatible = shared_pre_factor_interval(
                    left_outcomes[0].amount,
                    left_raw,
                    right_outcomes[0].amount,
                    right_raw,
                );
                let example = || AttributeStageIsolationExample {
                    session_id: identity.session_id.clone(),
                    run_ordinal: identity.run_ordinal,
                    source_entity_uuid: identity.source_entity_uuid,
                    target_entity_uuid: identity.target_entity_uuid,
                    ability_id: identity.ability_id,
                    left_axis_state: left_state.clone(),
                    right_axis_state: right_state.clone(),
                    left_source_current_hp: left.source_current_hp.iter().copied().collect(),
                    right_source_current_hp: right.source_current_hp.iter().copied().collect(),
                    left_target_current_hp: left.target_current_hp.iter().copied().collect(),
                    right_target_current_hp: right.target_current_hp.iter().copied().collect(),
                    left_outcomes: left_outcomes.clone(),
                    right_outcomes: right_outcomes.clone(),
                    compatible_pre_factor_minimum: compatible.map(|value| value.0.to_string()),
                    compatible_pre_factor_maximum: compatible.map(|value| value.1.to_string()),
                };
                if compatible.is_some() {
                    counters.exact_independent_final_stage_pairs = counters
                        .exact_independent_final_stage_pairs
                        .saturating_add(1);
                    if exact_examples.len() < example_limit {
                        exact_examples.push(example());
                    }
                } else {
                    counters.rejected_independent_final_stage_pairs = counters
                        .rejected_independent_final_stage_pairs
                        .saturating_add(1);
                    if rejected_examples.len() < example_limit {
                        rejected_examples.push(example());
                    }
                }
            }
        }
    }

    AttributeStageIsolationAxis {
        current_attribute_id,
        family_attribute_ids,
        counters,
        exact_examples,
        rejected_examples,
    }
}

fn attributes_without_current_hp_and_family(
    attributes: &[AttributeEntry],
    family: &BTreeSet<i32>,
) -> Vec<AttributeEntry> {
    attributes
        .iter()
        .filter(|entry| {
            entry.attribute_id != CURRENT_HP_ATTRIBUTE_ID && !family.contains(&entry.attribute_id)
        })
        .cloned()
        .collect()
}

fn shared_pre_factor_interval(
    left_output: i64,
    left_raw: i64,
    right_output: i64,
    right_raw: i64,
) -> Option<(i128, i128)> {
    let left = fixed_point_preimage(left_output, 10_000_i64.checked_add(left_raw)?)?;
    let right = fixed_point_preimage(right_output, 10_000_i64.checked_add(right_raw)?)?;
    let minimum = left.0.max(right.0);
    let maximum = left.1.min(right.1);
    (minimum <= maximum).then_some((minimum, maximum))
}

fn fixed_point_preimage(output: i64, factor: i64) -> Option<(i128, i128)> {
    if output < 0 || factor <= 0 {
        return None;
    }
    let scale = 10_000_i128;
    let factor = i128::from(factor);
    let output = i128::from(output);
    let minimum = ceil_div(output.checked_mul(scale)?, factor)?;
    let maximum = ceil_div(output.checked_add(1)?.checked_mul(scale)?, factor)?.checked_sub(1)?;
    (minimum <= maximum).then_some((minimum, maximum))
}

fn occurrence_control_diagnostic(
    cohort: &FormulaCohort,
    gap_by_event: &HashMap<(String, u64), GapSample>,
    indices_by_identity: &HashMap<CalculationIdentity, Vec<usize>>,
    scoped_neutral_status_transitions: &BTreeSet<ScopedStatusTransitionKey>,
    example_limit: usize,
) -> OccurrenceControlDiagnostic {
    let mut counters = OccurrenceControlCounters::default();
    let mut by_ability: BTreeMap<i64, OccurrenceControlAbilityCounters> = BTreeMap::new();
    let mut overlap_examples = Vec::new();
    let mut disjoint_examples = Vec::new();
    let mut status_mismatch_examples = Vec::new();
    let mut status_difference_accumulators: BTreeMap<
        StatusDifferenceKey,
        StatusDifferenceAccumulator,
    > = BTreeMap::new();
    for (gap_sample_index, sample) in cohort.samples.iter().enumerate() {
        let Some(gap_sample) = gap_by_event.get(&(sample.session_id.clone(), sample.sequence))
        else {
            continue;
        };
        let (Some(serialized_light), Some(logical_light)) =
            (gap_sample.serialized_light, gap_sample.logical_light)
        else {
            continue;
        };
        let Some(candidate_indices) = indices_by_identity.get(&calculation_identity(sample)) else {
            continue;
        };
        let mut gap_has_pair = false;
        let mut gap_has_equal_amount_witness = false;
        for &candidate_index in candidate_indices {
            if candidate_index == gap_sample_index {
                continue;
            }
            let stable = &cohort.samples[candidate_index];
            if attribute_value(
                &cohort.attribute_states[stable.source_attribute_state_id],
                LIGHT_DAMAGE_ATTRIBUTE_ID,
            ) != Some(logical_light)
            {
                continue;
            }
            counters.candidate_pairs = counters.candidate_pairs.saturating_add(1);
            if !same_attributes_except(
                &cohort.attribute_states[sample.source_attribute_state_id],
                &cohort.attribute_states[stable.source_attribute_state_id],
                OCCURRENCE_CONTROL_SOURCE_IGNORED_ATTRIBUTE_IDS,
            ) || !same_attributes_except(
                &cohort.attribute_states[sample.target_attribute_state_id],
                &cohort.attribute_states[stable.target_attribute_state_id],
                &[CURRENT_HP_ATTRIBUTE_ID],
            ) {
                continue;
            }
            counters.complete_attribute_control_pairs =
                counters.complete_attribute_control_pairs.saturating_add(1);
            let gap_source_statuses = &cohort.status_states[sample.source_status_state_id];
            let stable_source_statuses = &cohort.status_states[stable.source_status_state_id];
            let gap_target_statuses = &cohort.status_states[sample.target_status_state_id];
            let stable_target_statuses = &cohort.status_states[stable.target_status_state_id];
            let source_statuses_equal = gap_source_statuses == stable_source_statuses;
            let target_statuses_equal = gap_target_statuses == stable_target_statuses;
            if source_statuses_equal {
                counters.complete_source_status_control_pairs = counters
                    .complete_source_status_control_pairs
                    .saturating_add(1);
            }
            if target_statuses_equal {
                counters.complete_target_status_control_pairs = counters
                    .complete_target_status_control_pairs
                    .saturating_add(1);
            }
            if !source_statuses_equal || !target_statuses_equal {
                let (source_status_removed, source_status_added) =
                    status_entry_differences(gap_source_statuses, stable_source_statuses);
                let (target_status_removed, target_status_added) =
                    status_entry_differences(gap_target_statuses, stable_target_statuses);
                let difference_count = source_status_removed
                    .len()
                    .saturating_add(source_status_added.len())
                    .saturating_add(target_status_removed.len())
                    .saturating_add(target_status_added.len());
                status_mismatch_examples.push((
                    difference_count,
                    sample.observed_micros.abs_diff(stable.observed_micros),
                    StatusMismatchExample {
                        session_id: sample.session_id.clone(),
                        run_ordinal: sample.run_ordinal,
                        ability_id: sample.ability_id,
                        gap_sequence: sample.sequence,
                        stable_sequence: stable.sequence,
                        gap_wire_capture_sequence: sample.wire_capture_sequence,
                        stable_wire_capture_sequence: stable.wire_capture_sequence,
                        serialized_light,
                        logical_light,
                        gap_amount: sample.amount,
                        stable_amount: stable.amount,
                        gap_normal_value: sample.normal_value,
                        stable_normal_value: stable.normal_value,
                        critical: sample.critical,
                        lucky: sample.lucky,
                        gap_to_stable_micros: i64::try_from(stable.observed_micros)
                            .unwrap_or(i64::MAX)
                            .saturating_sub(
                                i64::try_from(sample.observed_micros).unwrap_or(i64::MAX),
                            ),
                        source_status_removed: source_status_removed.clone(),
                        source_status_added: source_status_added.clone(),
                        target_status_removed: target_status_removed.clone(),
                        target_status_added: target_status_added.clone(),
                    },
                ));
                let ability = by_ability.entry(sample.ability_id).or_default();
                let transition = format!("{serialized_light}->{logical_light}");
                let equal_amount = sample.amount == stable.amount;
                let normal_value_outcome = match (sample.normal_value, stable.normal_value) {
                    (Some(gap), Some(stable)) => Some(gap == stable),
                    _ => None,
                };
                let changed_effects = source_status_removed
                    .iter()
                    .chain(&source_status_added)
                    .chain(&target_status_removed)
                    .chain(&target_status_added)
                    .map(|status| status.effect_id)
                    .collect::<BTreeSet<_>>();
                let scoped_neutral_status_control = !changed_effects.is_empty()
                    && changed_effects.iter().all(|effect_id| {
                        let retain_effect = |status: &&StatusEntry| status.effect_id == *effect_id;
                        scoped_neutral_status_transitions.contains(
                            &canonical_scoped_status_transition_key(
                                calculation_identity(sample),
                                *effect_id,
                                source_status_removed
                                    .iter()
                                    .filter(retain_effect)
                                    .cloned()
                                    .collect(),
                                source_status_added
                                    .iter()
                                    .filter(retain_effect)
                                    .cloned()
                                    .collect(),
                                target_status_removed
                                    .iter()
                                    .filter(retain_effect)
                                    .cloned()
                                    .collect(),
                                target_status_added
                                    .iter()
                                    .filter(retain_effect)
                                    .cloned()
                                    .collect(),
                            ),
                        )
                    });
                let status_difference = status_difference_accumulators
                    .entry(StatusDifferenceKey {
                        source_status_removed,
                        source_status_added,
                        target_status_removed,
                        target_status_added,
                    })
                    .or_default();
                status_difference.gap_examples.insert((
                    sample.session_id.clone(),
                    sample.run_ordinal,
                    sample.sequence,
                ));
                let status_ability = status_difference
                    .by_ability
                    .entry(sample.ability_id)
                    .or_default();
                let status_transition = status_difference
                    .by_light_transition
                    .entry(transition.clone())
                    .or_default();
                if equal_amount {
                    status_difference.equal_amount_pairs =
                        status_difference.equal_amount_pairs.saturating_add(1);
                    status_ability.equal_amount_pairs =
                        status_ability.equal_amount_pairs.saturating_add(1);
                    status_transition.equal_amount_pairs =
                        status_transition.equal_amount_pairs.saturating_add(1);
                    counters.status_mismatch_equal_amount_pairs = counters
                        .status_mismatch_equal_amount_pairs
                        .saturating_add(1);
                    ability.status_mismatch_equal_amount_pairs =
                        ability.status_mismatch_equal_amount_pairs.saturating_add(1);
                    *counters
                        .equal_amount_pairs_by_light_transition
                        .entry(transition.clone())
                        .or_default() += 1;
                    *ability
                        .equal_amount_pairs_by_light_transition
                        .entry(transition)
                        .or_default() += 1;
                    gap_has_equal_amount_witness = true;
                } else {
                    status_difference.divergent_amount_pairs =
                        status_difference.divergent_amount_pairs.saturating_add(1);
                    status_ability.divergent_amount_pairs =
                        status_ability.divergent_amount_pairs.saturating_add(1);
                    status_transition.divergent_amount_pairs =
                        status_transition.divergent_amount_pairs.saturating_add(1);
                    counters.status_mismatch_divergent_amount_pairs = counters
                        .status_mismatch_divergent_amount_pairs
                        .saturating_add(1);
                    ability.status_mismatch_divergent_amount_pairs = ability
                        .status_mismatch_divergent_amount_pairs
                        .saturating_add(1);
                    *counters
                        .divergent_amount_pairs_by_light_transition
                        .entry(transition.clone())
                        .or_default() += 1;
                    *ability
                        .divergent_amount_pairs_by_light_transition
                        .entry(transition)
                        .or_default() += 1;
                }
                match normal_value_outcome {
                    Some(equal_normal_value) => {
                        status_difference.pairs_with_both_normal_values = status_difference
                            .pairs_with_both_normal_values
                            .saturating_add(1);
                        status_ability.pairs_with_both_normal_values = status_ability
                            .pairs_with_both_normal_values
                            .saturating_add(1);
                        status_transition.pairs_with_both_normal_values = status_transition
                            .pairs_with_both_normal_values
                            .saturating_add(1);
                        counters.status_mismatch_pairs_with_both_normal_values = counters
                            .status_mismatch_pairs_with_both_normal_values
                            .saturating_add(1);
                        ability.status_mismatch_pairs_with_both_normal_values = ability
                            .status_mismatch_pairs_with_both_normal_values
                            .saturating_add(1);
                        if equal_normal_value {
                            status_difference.equal_normal_value_pairs =
                                status_difference.equal_normal_value_pairs.saturating_add(1);
                            status_ability.equal_normal_value_pairs =
                                status_ability.equal_normal_value_pairs.saturating_add(1);
                            status_transition.equal_normal_value_pairs =
                                status_transition.equal_normal_value_pairs.saturating_add(1);
                            counters.status_mismatch_equal_normal_value_pairs = counters
                                .status_mismatch_equal_normal_value_pairs
                                .saturating_add(1);
                            ability.status_mismatch_equal_normal_value_pairs = ability
                                .status_mismatch_equal_normal_value_pairs
                                .saturating_add(1);
                        } else {
                            status_difference.divergent_normal_value_pairs = status_difference
                                .divergent_normal_value_pairs
                                .saturating_add(1);
                            status_ability.divergent_normal_value_pairs = status_ability
                                .divergent_normal_value_pairs
                                .saturating_add(1);
                            status_transition.divergent_normal_value_pairs = status_transition
                                .divergent_normal_value_pairs
                                .saturating_add(1);
                            counters.status_mismatch_divergent_normal_value_pairs = counters
                                .status_mismatch_divergent_normal_value_pairs
                                .saturating_add(1);
                            ability.status_mismatch_divergent_normal_value_pairs = ability
                                .status_mismatch_divergent_normal_value_pairs
                                .saturating_add(1);
                        }
                    }
                    None => {
                        status_difference.pairs_missing_any_normal_value = status_difference
                            .pairs_missing_any_normal_value
                            .saturating_add(1);
                        status_ability.pairs_missing_any_normal_value = status_ability
                            .pairs_missing_any_normal_value
                            .saturating_add(1);
                        status_transition.pairs_missing_any_normal_value = status_transition
                            .pairs_missing_any_normal_value
                            .saturating_add(1);
                        counters.status_mismatch_pairs_missing_any_normal_value = counters
                            .status_mismatch_pairs_missing_any_normal_value
                            .saturating_add(1);
                        ability.status_mismatch_pairs_missing_any_normal_value = ability
                            .status_mismatch_pairs_missing_any_normal_value
                            .saturating_add(1);
                    }
                }
                if scoped_neutral_status_control {
                    counters.scoped_neutral_status_control_pairs = counters
                        .scoped_neutral_status_control_pairs
                        .saturating_add(1);
                    ability.scoped_neutral_status_control_pairs = ability
                        .scoped_neutral_status_control_pairs
                        .saturating_add(1);
                } else {
                    continue;
                }
            }
            counters.complete_status_control_pairs =
                counters.complete_status_control_pairs.saturating_add(1);
            let critical = sample.critical.unwrap_or(false);
            let lucky = sample.lucky.unwrap_or(false);
            if critical && lucky {
                counters.unsupported_combined_critical_lucky_pairs = counters
                    .unsupported_combined_critical_lucky_pairs
                    .saturating_add(1);
                continue;
            }
            let gap_attributes = &cohort.attribute_states[sample.source_attribute_state_id];
            let stable_attributes = &cohort.attribute_states[stable.source_attribute_state_id];
            let multiplier_attribute_id = if critical {
                Some(12_510)
            } else if lucky {
                Some(12_530)
            } else {
                None
            };
            let (gap_multiplier_raw, stable_multiplier_raw) = match multiplier_attribute_id {
                Some(attribute_id) => {
                    let (Some(gap), Some(stable)) = (
                        attribute_value(gap_attributes, attribute_id),
                        attribute_value(stable_attributes, attribute_id),
                    ) else {
                        counters.missing_multiplier_attribute_pairs = counters
                            .missing_multiplier_attribute_pairs
                            .saturating_add(1);
                        continue;
                    };
                    (gap, stable)
                }
                None => (0, 0),
            };
            let gap_factor_numerator = if critical {
                10_000_i64.saturating_add(gap_multiplier_raw)
            } else if lucky {
                gap_multiplier_raw
            } else {
                10_000
            };
            let stable_factor_numerator = if critical {
                10_000_i64.saturating_add(stable_multiplier_raw)
            } else if lucky {
                stable_multiplier_raw
            } else {
                10_000
            };
            let (Some((gap_base_min, gap_base_max)), Some((stable_base_min, stable_base_max))) = (
                inverse_floor_multiplier_interval(sample.amount, gap_factor_numerator, 10_000),
                inverse_floor_multiplier_interval(stable.amount, stable_factor_numerator, 10_000),
            ) else {
                continue;
            };
            let overlap_min_value = gap_base_min.max(stable_base_min);
            let overlap_max_value = gap_base_max.min(stable_base_max);
            let overlaps = overlap_min_value <= overlap_max_value;
            let example = RejectedShortcutPairExample {
                session_id: sample.session_id.clone(),
                run_ordinal: sample.run_ordinal,
                ability_id: sample.ability_id,
                gap_sequence: sample.sequence,
                stable_sequence: stable.sequence,
                serialized_light,
                logical_light,
                gap_amount: sample.amount,
                stable_amount: stable.amount,
                critical,
                lucky,
                gap_multiplier_raw,
                stable_multiplier_raw,
                gap_base_min,
                gap_base_max,
                stable_base_min,
                stable_base_max,
                overlap_min: overlaps.then_some(overlap_min_value),
                overlap_max: overlaps.then_some(overlap_max_value),
            };
            gap_has_pair = true;
            let ability = by_ability.entry(sample.ability_id).or_default();
            ability.rejected_shortcut_control_pairs =
                ability.rejected_shortcut_control_pairs.saturating_add(1);
            if overlaps {
                counters.rejected_shortcut_interval_overlap_pairs = counters
                    .rejected_shortcut_interval_overlap_pairs
                    .saturating_add(1);
                ability.rejected_shortcut_interval_overlap_pairs = ability
                    .rejected_shortcut_interval_overlap_pairs
                    .saturating_add(1);
                if overlap_examples.len() < example_limit {
                    overlap_examples.push(example);
                }
            } else {
                counters.rejected_shortcut_interval_disjoint_pairs = counters
                    .rejected_shortcut_interval_disjoint_pairs
                    .saturating_add(1);
                ability.rejected_shortcut_interval_disjoint_pairs = ability
                    .rejected_shortcut_interval_disjoint_pairs
                    .saturating_add(1);
                if disjoint_examples.len() < example_limit {
                    disjoint_examples.push(example);
                }
            }
        }
        if gap_has_pair {
            counters.gap_examples_with_any_rejected_shortcut_pair = counters
                .gap_examples_with_any_rejected_shortcut_pair
                .saturating_add(1);
        }
        if gap_has_equal_amount_witness {
            counters.gap_examples_with_equal_amount_witness = counters
                .gap_examples_with_equal_amount_witness
                .saturating_add(1);
        }
    }
    status_mismatch_examples.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.session_id.cmp(&right.2.session_id))
            .then_with(|| left.2.run_ordinal.cmp(&right.2.run_ordinal))
            .then_with(|| left.2.gap_sequence.cmp(&right.2.gap_sequence))
            .then_with(|| left.2.stable_sequence.cmp(&right.2.stable_sequence))
    });
    let status_difference_aggregates = status_difference_accumulators
        .into_iter()
        .map(|(key, accumulator)| StatusDifferenceAggregate {
            source_status_removed: key.source_status_removed,
            source_status_added: key.source_status_added,
            target_status_removed: key.target_status_removed,
            target_status_added: key.target_status_added,
            equal_amount_pairs: accumulator.equal_amount_pairs,
            divergent_amount_pairs: accumulator.divergent_amount_pairs,
            pairs_with_both_normal_values: accumulator.pairs_with_both_normal_values,
            equal_normal_value_pairs: accumulator.equal_normal_value_pairs,
            divergent_normal_value_pairs: accumulator.divergent_normal_value_pairs,
            pairs_missing_any_normal_value: accumulator.pairs_missing_any_normal_value,
            distinct_gap_examples: accumulator.gap_examples.len(),
            by_ability: accumulator.by_ability,
            by_light_transition: accumulator.by_light_transition,
        })
        .collect();
    OccurrenceControlDiagnostic {
        policy: OccurrenceControlPolicy {
            runtime_authority: false,
            source_attribute_control: "all source attributes equal except volatile CurrentHP, delayed Light Damage, Attack Speed, Critical Damage, Lucky Damage, and Lucky Healing families; target attributes except CurrentHP are exact; source and target status equality is counted separately",
            scoped_status_control: "a mismatched status transition may proceed only when every changed effect has repeated deterministic equal-output evidence with zero divergent or nondeterministic evidence for the exact complete calculation identity and exact source/target lifecycle transition; this is an offline comparison scope, never a global ignored-effect list",
            rejected_shortcut: "the retained interval calculation assumes critical or lucky damage is the final multiplier over an otherwise identical base. Packet evidence has not established that stage order, so overlap and disjoint counts are rejected diagnostics and cannot prove elemental snapshot behavior or drive runtime attribution",
            retained_value: "equal and divergent final packet amounts, packet-retained normal_value equality/divergence/missingness, complete attribute/status controls, status differences, critical/lucky flags, and serialized-to-logical Light transitions remain authoritative observations; normal_value is not assigned a formula stage by this diagnostic",
            interpretation_boundary: "equal-output witnesses with mismatched statuses are supporting observations only; exact state controls and an independently proven final-damage stage are required before Mastery-derived Light may contribute rDPS",
        },
        counters,
        by_ability,
        rejected_shortcut_overlap_examples: overlap_examples,
        rejected_shortcut_disjoint_examples: disjoint_examples,
        nearest_status_mismatch_examples: status_mismatch_examples
            .into_iter()
            .take(example_limit)
            .map(|(_, _, example)| example)
            .collect(),
        status_difference_aggregates,
    }
}

fn inverse_floor_multiplier_interval(
    observed: i64,
    factor_numerator: i64,
    factor_denominator: i64,
) -> Option<(i64, i64)> {
    if observed < 0 || factor_numerator <= 0 || factor_denominator <= 0 {
        return None;
    }
    let observed = i128::from(observed);
    let numerator = i128::from(factor_numerator);
    let denominator = i128::from(factor_denominator);
    let lower = ceil_div(observed.checked_mul(denominator)?, numerator)?;
    let upper_exclusive = ceil_div(
        observed.checked_add(1)?.checked_mul(denominator)?,
        numerator,
    )?;
    let upper = upper_exclusive.checked_sub(1)?;
    Some((i64::try_from(lower).ok()?, i64::try_from(upper).ok()?))
}

fn ceil_div(numerator: i128, denominator: i128) -> Option<i128> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    numerator
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)
}

fn calculation_identity(sample: &FormulaSample) -> CalculationIdentity {
    CalculationIdentity {
        session_id: sample.session_id.clone(),
        run_ordinal: sample.run_ordinal,
        source_entity_uuid: sample.source_entity_uuid,
        direct_source_entity_uuid: sample.direct_source_entity_uuid,
        target_entity_uuid: sample.target_entity_uuid,
        ability_id: sample.ability_id,
        passive_uuid: sample.passive_uuid,
        hit_event_id: sample.hit_event_id,
        damage_source: sample.damage_source,
        damage_type: sample.damage_type,
        critical: sample.critical,
        lucky: sample.lucky,
        type_flags: sample.packet.type_flags,
        owner_level: sample.packet.owner_level,
        owner_stage: sample.packet.owner_stage,
        normal_hit: sample.packet.normal_hit,
        property: sample.packet.property,
        packet_passive_uuid: sample.packet.passive_uuid,
        rainbow: sample.packet.rainbow,
        damage_mode: sample.packet.damage_mode,
        skill_effect_uuid: sample.packet.skill_effect_uuid,
        skill_effect_component_index: sample.packet.skill_effect_component_index,
        skill_effect_component_count: sample.packet.skill_effect_component_count,
        raw_attacker_uuid: sample.packet.attacker_uuid,
        raw_top_summoner_uuid: sample.packet.top_summoner_uuid,
        raw_owner_id: sample.packet.owner_id,
        damage_weight_bits: sample.packet.damage_weight.map(|weight| {
            (
                weight.x.map(f32::to_bits),
                weight.y.map(f32::to_bits),
                weight.z.map(f32::to_bits),
            )
        }),
        hit_part_ids: sample
            .packet
            .hit_parts
            .iter()
            .map(|part| part.part_id)
            .collect(),
    }
}

fn attribute_value(attributes: &[AttributeEntry], attribute_id: i32) -> Option<i64> {
    attributes
        .iter()
        .find(|attribute| attribute.attribute_id == attribute_id)
        .map(|attribute| attribute.value)
}

fn attributes_without_current_hp(attributes: &[AttributeEntry]) -> Vec<AttributeEntry> {
    attributes
        .iter()
        .filter(|entry| entry.attribute_id != CURRENT_HP_ATTRIBUTE_ID)
        .cloned()
        .collect()
}

fn same_attributes_except(
    left: &[AttributeEntry],
    right: &[AttributeEntry],
    ignored: &[i32],
) -> bool {
    left.iter()
        .filter(|entry| !ignored.contains(&entry.attribute_id))
        .eq(right
            .iter()
            .filter(|entry| !ignored.contains(&entry.attribute_id)))
}

fn attribute_difference_ids(
    left: &[AttributeEntry],
    right: &[AttributeEntry],
    ignored: &[i32],
) -> Vec<AttributeDifference> {
    let left = left
        .iter()
        .filter(|entry| !ignored.contains(&entry.attribute_id))
        .map(|entry| (entry.attribute_id, entry.value))
        .collect::<BTreeMap<_, _>>();
    let right = right
        .iter()
        .filter(|entry| !ignored.contains(&entry.attribute_id))
        .map(|entry| (entry.attribute_id, entry.value))
        .collect::<BTreeMap<_, _>>();
    left.keys()
        .chain(right.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter_map(|attribute_id| {
            let gap_value = left.get(&attribute_id).copied();
            let stable_value = right.get(&attribute_id).copied();
            (gap_value != stable_value).then_some(AttributeDifference {
                attribute_id,
                gap_value,
                stable_value,
            })
        })
        .collect()
}

fn status_difference_ids(left: &[StatusEntry], right: &[StatusEntry]) -> (Vec<i64>, Vec<i64>) {
    let left_counts = status_counts(left);
    let right_counts = status_counts(right);
    let removed = left_counts
        .iter()
        .filter(|(status, count)| right_counts.get(*status).unwrap_or(&0) < *count)
        .flat_map(|(status, count)| {
            std::iter::repeat_n(
                status.effect_id,
                count.saturating_sub(*right_counts.get(status).unwrap_or(&0)),
            )
        })
        .collect();
    let added = right_counts
        .iter()
        .filter(|(status, count)| left_counts.get(*status).unwrap_or(&0) < *count)
        .flat_map(|(status, count)| {
            std::iter::repeat_n(
                status.effect_id,
                count.saturating_sub(*left_counts.get(status).unwrap_or(&0)),
            )
        })
        .collect();
    (removed, added)
}

fn status_entry_differences(
    left: &[StatusEntry],
    right: &[StatusEntry],
) -> (Vec<StatusEntry>, Vec<StatusEntry>) {
    let left_counts = status_counts(left);
    let right_counts = status_counts(right);
    let removed = left_counts
        .iter()
        .filter(|(status, count)| right_counts.get(*status).unwrap_or(&0) < *count)
        .flat_map(|(status, count)| {
            std::iter::repeat_n(
                status.clone(),
                count.saturating_sub(*right_counts.get(status).unwrap_or(&0)),
            )
        })
        .collect();
    let added = right_counts
        .iter()
        .filter(|(status, count)| left_counts.get(*status).unwrap_or(&0) < *count)
        .flat_map(|(status, count)| {
            std::iter::repeat_n(
                status.clone(),
                count.saturating_sub(*left_counts.get(status).unwrap_or(&0)),
            )
        })
        .collect();
    (removed, added)
}

fn status_counts(statuses: &[StatusEntry]) -> BTreeMap<StatusEntry, usize> {
    let mut counts = BTreeMap::new();
    for status in statuses {
        *counts.entry(status.clone()).or_default() += 1;
    }
    counts
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let cohort = PathBuf::from(take_value(&mut values, "--cohort")?);
    let gap_proof = PathBuf::from(take_value(&mut values, "--gap-proof")?);
    let damage_surface = PathBuf::from(take_value(&mut values, "--damage-surface")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let example_limit = take_optional_value(&mut values, "--example-limit")?
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<usize>()
                .map_err(|_| "--example-limit requires a non-negative integer".to_owned())
        })
        .transpose()?
        .unwrap_or(24);
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        cohort,
        gap_proof,
        damage_surface,
        output,
        example_limit,
    })
}

fn take_value(
    values: &mut Vec<std::ffi::OsString>,
    option: &str,
) -> Result<std::ffi::OsString, String> {
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
    values: &mut Vec<std::ffi::OsString>,
    option: &str,
) -> Result<Option<std::ffi::OsString>, String> {
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
    "usage: rlogs-bpsr-inspiration-elemental-snapshot-proof --cohort <formula-cohort.json> --gap-proof <inspiration-mastery-gap-proof.json> --damage-surface <damage-correlation.json> --output <audit.json> [--example-limit <count>]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{AttributeEntry, SOURCE_IGNORED_ATTRIBUTE_IDS, same_attributes_except};

    #[test]
    fn ignored_delayed_attribute_does_not_change_state_identity() {
        let left = vec![
            AttributeEntry {
                attribute_id: 1,
                value: 10,
            },
            AttributeEntry {
                attribute_id: 13_170,
                value: 1_320,
            },
        ];
        let right = vec![
            AttributeEntry {
                attribute_id: 1,
                value: 10,
            },
            AttributeEntry {
                attribute_id: 13_170,
                value: 1_500,
            },
        ];
        assert!(same_attributes_except(&left, &right, &[13_170]));
        assert!(!same_attributes_except(&left, &right, &[]));
    }

    #[test]
    fn complete_delayed_light_family_does_not_change_state_identity() {
        let left = (13_170..=13_175)
            .map(|attribute_id| AttributeEntry {
                attribute_id,
                value: 1_320,
            })
            .chain(std::iter::once(AttributeEntry {
                attribute_id: 11_330,
                value: 6_058,
            }))
            .collect::<Vec<_>>();
        let right = (13_170..=13_175)
            .map(|attribute_id| AttributeEntry {
                attribute_id,
                value: 1_500,
            })
            .chain(std::iter::once(AttributeEntry {
                attribute_id: 11_330,
                value: 6_058,
            }))
            .collect::<Vec<_>>();
        assert!(same_attributes_except(
            &left,
            &right,
            SOURCE_IGNORED_ATTRIBUTE_IDS
        ));
    }
}
