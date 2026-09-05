use std::{
    collections::{BTreeMap, BTreeSet, HashMap, hash_map::DefaultHasher},
    env,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rlogs_events::DamagePacketDetail;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 22;
const REQUIRED_FORMULA_COHORT_SCHEMA_VERSIONS: [u16; 9] = [40, 41, 42, 43, 44, 45, 46, 47, 48];
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const PHYSICAL_DEFENSE_ATTRIBUTE_ID: i32 = 11_350;
const BLADE_SWEEP_EFFECT_ID: i64 = 2_110_092;
const FATAL_SPIRAL_EFFECT_ID: i64 = 2_110_125;
const ALL_ELEMENT_CURRENT_ATTRIBUTE_ID: i32 = 13_100;
const BLADE_SWEEP_ARMOR_PENETRATION_BASIS_POINTS: i64 = 650;
const TARGET_DEFENSE_CURVE_CONSTANT: i64 = 22_000;
const BASIS_POINT_SCALE: i64 = 10_000;
const MIN_PARTITIONS: usize = 16;
const MAX_PARTITIONS: usize = 4_096;
const RAW_PARTITION_MEMORY_DIVISOR: u64 = 32;
const MEMORY_CHECK_INTERVAL_SAMPLES: usize = 4_096;
const PARTITION_WRITER_BUFFER_BYTES: usize = 1_024;
const NEAR_MAX_TARGET_ATTRIBUTE_TRANSITIONS: usize = 2;
const NEAR_MAX_TARGET_STATUS_CO_TRANSITIONS: usize = 4;
const SOURCE_STATUS_TRANSITION_REVIEW_LIMIT: usize = 12;
static OBSERVED_MAX_WORKING_SET_BYTES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct Arguments {
    cohort: PathBuf,
    baseline_proof: Option<PathBuf>,
    output: PathBuf,
    effect_ids: BTreeSet<i64>,
    source_character_ids: BTreeSet<String>,
    source_entity_uuids: BTreeSet<i64>,
    example_limit: usize,
    histogram_limit: usize,
    memory_limit_mib: usize,
    source_transition_attribute_ids: BTreeSet<i32>,
    cross_entity_formula_state_diagnostic: bool,
}

#[derive(Debug)]
struct PartitionedCohort {
    game_build: Option<String>,
    source_inputs: Vec<String>,
    input_bytes: u64,
    input_sha256: String,
    attribute_states: Vec<Vec<Attribute>>,
    status_states: Vec<Vec<Status>>,
    source_status_usage: Vec<u64>,
    target_status_usage: Vec<u64>,
    rlogs: Vec<String>,
    sessions: Vec<String>,
    scanned_sample_count: usize,
    sample_count: usize,
    partition_paths: Vec<PathBuf>,
    cross_entity_partition_paths: Vec<PathBuf>,
    work_dir: PathBuf,
    largest_partition_bytes: u64,
    largest_cross_entity_partition_bytes: u64,
    configured_memory_limit_bytes: u64,
}

impl Drop for PartitionedCohort {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.work_dir);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct Attribute {
    attribute_id: i32,
    value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct Status {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Sample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    #[serde(default)]
    scene_id: Option<i32>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    #[serde(default)]
    source_actor_identity: Option<ActorIdentity>,
    #[serde(default)]
    direct_source_actor_identity: Option<ActorIdentity>,
    #[serde(default)]
    target_actor_identity: Option<ActorIdentity>,
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    amount: i64,
    actual_amount: Option<i64>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    packet: DamagePacketDetail,
    source_attribute_state_id: u32,
    #[serde(default)]
    direct_source_attribute_state_id: Option<u32>,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
    #[serde(default)]
    status_provider_attribute_states: Vec<ProviderAttributeStateRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
struct ActorIdentity {
    entity_type_id: i32,
    monster_id: Option<i64>,
    character_id: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
enum Locus {
    Source,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProviderRelationship {
    CreditedDamageSource,
    DirectDamageSource,
    DamageTarget,
    ThirdParty,
    MissingProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct VariantKey {
    locus: Locus,
    status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct Outcome {
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
struct Identity {
    rlog_id: u32,
    session_id: u32,
    run_ordinal: u32,
    scene_id: Option<i32>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    source_actor_identity: Option<ActorIdentity>,
    direct_source_actor_identity: Option<ActorIdentity>,
    target_actor_identity: Option<ActorIdentity>,
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    packet_input_fingerprint: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
struct ExactKey {
    identity: Identity,
    source_attribute_state_id: u32,
    direct_source_attribute_state_id: Option<u32>,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
    status_provider_attribute_states: Vec<ProviderAttributeStateRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NearTargetKey {
    identity: Identity,
    source_attribute_state_id: u32,
    direct_source_attribute_state_id: Option<u32>,
    source_status_state_id: u32,
    status_provider_attribute_states: Vec<ProviderAttributeStateRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NearSourceKey {
    identity: Identity,
    direct_source_attribute_state_id: Option<u32>,
    target_attribute_state_id: u32,
    target_status_state_id: u32,
    status_provider_attribute_states: Vec<ProviderAttributeStateRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
struct FormulaActorIdentity {
    entity_type_id: i32,
    monster_id: Option<i64>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CrossEntityPacketIdentity {
    source_actor_identity: Option<FormulaActorIdentity>,
    direct_source_actor_identity: Option<FormulaActorIdentity>,
    target_actor_identity: Option<FormulaActorIdentity>,
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    packet_input_fingerprint: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
struct FormulaStatus {
    effect_id: i64,
    provider_relationship: ProviderRelationship,
    provider_attribute_state_observed: bool,
    provider_attribute_state_id: Option<u32>,
    stacks: Option<u32>,
    level: Option<i32>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CrossEntityExactKey {
    identity: CrossEntityPacketIdentity,
    source_attribute_state_id: u32,
    direct_source_attribute_state_id: Option<u32>,
    target_attribute_state_id: u32,
    source_statuses: Vec<FormulaStatus>,
    target_statuses: Vec<FormulaStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CrossEntitySourceTransitionKey {
    identity: CrossEntityPacketIdentity,
    direct_source_attribute_state_id: Option<u32>,
    target_attribute_state_id: u32,
    target_statuses: Vec<FormulaStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CrossEntityVariantKey {
    locus: Locus,
    status: FormulaStatus,
}

#[derive(Debug, Default)]
struct Bucket {
    sample_count: u64,
    outcomes: BTreeMap<Outcome, u64>,
    sequences: BTreeSet<u64>,
    representative_sample: Option<PartitionSample>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PartitionSample {
    identity: Identity,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    normalized_packet_inputs: DamagePacketDetail,
    outcome: Outcome,
    source_attribute_state_id: u32,
    direct_source_attribute_state_id: Option<u32>,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
    status_provider_attribute_states: Vec<ProviderAttributeStateRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct ProviderAttributeStateRef {
    provider_entity_uuid: i64,
    attribute_state_id: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct ObservationStats {
    observed_status_states: u64,
    observed_samples: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
struct ModeStats {
    present_groups: u64,
    present_samples: u64,
    absent_status_state_unobserved_groups: u64,
    absent_identity_group_unobserved_groups: u64,
    controlled_groups: u64,
    sample_comparisons: u64,
    deterministic_groups: u64,
    equal_output_groups: u64,
    divergent_output_groups: u64,
    nondeterministic_groups: u64,
    amount_differences: Vec<DifferenceCount>,
    normal_value_differences: Vec<DifferenceCount>,
    amount_ratio_basis_points: Vec<DifferenceCount>,
    divergent_provider_relationship_groups: Vec<ProviderRelationshipCount>,
    nondeterministic_examples: Vec<NondeterministicExample>,
    divergent_examples: Vec<Example>,
    blade_sweep_candidate_projection: Option<BladeSweepCandidateProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
enum EffectiveDefenseRounding {
    Floor,
    Ceil,
    RoundHalfUp,
}

#[derive(Debug, Clone, Default)]
struct CandidateVariantAccum {
    compatible_groups: u64,
    rejected_groups: u64,
}

#[derive(Debug, Clone, Default)]
struct BladeSweepCandidateAccum {
    controlled_divergent_groups: u64,
    groups_with_target_physical_defense: u64,
    groups_missing_target_physical_defense: u64,
    groups_with_invalid_nonnegative_inputs: u64,
    variants: BTreeMap<EffectiveDefenseRounding, CandidateVariantAccum>,
    examples: Vec<BladeSweepCandidateExample>,
}

#[derive(Debug, Clone, Serialize)]
struct BladeSweepCandidateProjection {
    model_id: &'static str,
    effect_id: i64,
    armor_penetration_basis_points: i64,
    defense_curve_constant: i64,
    controlled_divergent_groups: u64,
    groups_with_target_physical_defense: u64,
    groups_missing_target_physical_defense: u64,
    groups_with_invalid_nonnegative_inputs: u64,
    variants: Vec<CandidateVariantStats>,
    examples: Vec<BladeSweepCandidateExample>,
    candidate_selected: bool,
    exact_damage_projection_proven: bool,
    exact_operation_order_proven: bool,
    exact_integer_rounding_proven: bool,
    formula_authority: bool,
    runtime_authority: bool,
    ui_display_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CandidateVariantStats {
    rounding: EffectiveDefenseRounding,
    compatible_groups: u64,
    rejected_groups: u64,
}

#[derive(Debug, Clone, Serialize)]
struct BladeSweepCandidateExample {
    target_physical_defense_raw: i64,
    absent_damage: i64,
    present_damage: i64,
    compatible_base_minimum: String,
    compatible_base_maximum: String,
    variants: Vec<CandidateVariantExample>,
}

#[derive(Debug, Clone, Serialize)]
struct CandidateVariantExample {
    rounding: EffectiveDefenseRounding,
    effective_target_physical_defense_raw: i64,
    predicted_present_damage_minimum: String,
    predicted_present_damage_maximum: String,
    observed_present_damage_compatible: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DifferenceCount {
    value: i64,
    comparisons: u64,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderRelationshipCount {
    relationship: ProviderRelationship,
    groups: u64,
    sample_comparisons: u64,
}

#[derive(Debug, Clone, Serialize)]
struct OutcomeCount {
    outcome: Outcome,
    samples: u64,
}

#[derive(Debug, Clone, Serialize)]
struct NondeterministicExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    status: Status,
    locus: Locus,
    provider_relationship: ProviderRelationship,
    present_sample_count: u64,
    absent_sample_count: u64,
    present_unique_outcomes: usize,
    absent_unique_outcomes: usize,
    present_outcomes: Vec<OutcomeCount>,
    absent_outcomes: Vec<OutcomeCount>,
    present_sequences: Vec<u64>,
    absent_sequences: Vec<u64>,
    present_formula_context: FormulaContext,
    absent_formula_context: FormulaContext,
}

#[derive(Debug, Clone, Serialize)]
struct Example {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    status: Status,
    locus: Locus,
    provider_relationship: ProviderRelationship,
    present_outcome: Outcome,
    absent_outcome: Outcome,
    present_sequences: Vec<u64>,
    absent_sequences: Vec<u64>,
    present_formula_context: FormulaContext,
    absent_formula_context: FormulaContext,
}

#[derive(Debug, Clone, Serialize)]
struct FormulaContext {
    representative_sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    normalized_packet_input_sha256: String,
    normalized_packet_inputs: DamagePacketDetail,
    source_attribute_state_id: u32,
    source_attributes: Vec<Attribute>,
    direct_source_attribute_state_id: Option<u32>,
    direct_source_attributes: Vec<Attribute>,
    target_attribute_state_id: u32,
    target_attributes: Vec<Attribute>,
    source_status_state_id: u32,
    source_statuses: Vec<Status>,
    target_status_state_id: u32,
    target_statuses: Vec<Status>,
    status_provider_attributes: Vec<FormulaProviderContext>,
}

#[derive(Debug, Clone, Serialize)]
struct FormulaProviderContext {
    provider_entity_uuid: i64,
    attribute_state_observed: bool,
    attribute_state_id: Option<u32>,
    attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, Serialize)]
struct AttributeTransition {
    attribute_id: i32,
    present_value: Option<i64>,
    absent_value: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct NearTargetExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    candidate_status: Status,
    provider_relationship: ProviderRelationship,
    target_attribute_transitions_excluding_current_hp: Vec<AttributeTransition>,
    target_status_present_only_co_transitions: Vec<Status>,
    target_status_absent_only_co_transitions: Vec<Status>,
    transition_distance: usize,
    outputs_equal: bool,
    present_outcome: Outcome,
    absent_outcome: Outcome,
    present_sequences: Vec<u64>,
    absent_sequences: Vec<u64>,
    present_formula_context: FormulaContext,
    absent_formula_context: FormulaContext,
}

#[derive(Debug, Default)]
struct NearTargetAccum {
    candidate_present_groups: u64,
    candidate_absent_near_pairs: u64,
    sample_comparisons: u64,
    deterministic_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    nondeterministic_pairs: u64,
    minimum_transition_distance: Option<usize>,
    transition_distance_counts: BTreeMap<usize, u64>,
    examples: Vec<NearTargetExample>,
}

#[derive(Debug, Serialize)]
struct NearTargetVariantReport {
    status: Status,
    candidate_present_groups: u64,
    candidate_absent_near_pairs: u64,
    sample_comparisons: u64,
    deterministic_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    nondeterministic_pairs: u64,
    minimum_transition_distance: Option<usize>,
    transition_distance_counts: Vec<NearDistanceCount>,
    examples: Vec<NearTargetExample>,
}

#[derive(Debug, Serialize)]
struct NearDistanceCount {
    transition_distance: usize,
    pairs: u64,
}

#[derive(Debug, Serialize)]
struct NearTargetEffectReport {
    locus: Locus,
    effect_id: i64,
    candidate_absent_near_pairs: u64,
    divergent_output_pairs: u64,
    minimum_transition_distance: Option<usize>,
    variants: Vec<NearTargetVariantReport>,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct NearSourceExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    candidate_status: Status,
    provider_relationship: ProviderRelationship,
    source_attribute_transitions: Vec<AttributeTransition>,
    source_status_present_only_co_transitions: Vec<Status>,
    source_status_absent_only_co_transitions: Vec<Status>,
    transition_distance: usize,
    outputs_equal: bool,
    present_outcome: Outcome,
    absent_outcome: Outcome,
    present_sequences: Vec<u64>,
    absent_sequences: Vec<u64>,
    present_formula_context: FormulaContext,
    absent_formula_context: FormulaContext,
}

#[derive(Debug, Default)]
struct NearSourceAccum {
    candidate_present_groups: u64,
    present_groups_without_effect_absent_identity_state: u64,
    effect_absent_identity_state_candidates: u64,
    rejected_without_source_attribute_transition: u64,
    rejected_with_unselected_source_attribute_transition: u64,
    rejected_with_excess_source_status_co_transitions: u64,
    rejected_source_attribute_transition_sets: BTreeMap<Vec<i32>, u64>,
    candidate_absent_near_pairs: u64,
    sample_comparisons: u64,
    deterministic_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    nondeterministic_pairs: u64,
    minimum_transition_distance: Option<usize>,
    transition_distance_counts: BTreeMap<usize, u64>,
    examples: Vec<NearSourceExample>,
}

#[derive(Debug, Serialize)]
struct NearSourceVariantReport {
    status: Status,
    candidate_present_groups: u64,
    present_groups_without_effect_absent_identity_state: u64,
    effect_absent_identity_state_candidates: u64,
    rejected_without_source_attribute_transition: u64,
    rejected_with_unselected_source_attribute_transition: u64,
    rejected_with_excess_source_status_co_transitions: u64,
    rejected_source_attribute_transition_sets: Vec<SourceAttributeTransitionSetCount>,
    candidate_absent_near_pairs: u64,
    sample_comparisons: u64,
    deterministic_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    nondeterministic_pairs: u64,
    minimum_transition_distance: Option<usize>,
    transition_distance_counts: Vec<NearDistanceCount>,
    examples: Vec<NearSourceExample>,
}

#[derive(Debug, Serialize)]
struct SourceAttributeTransitionSetCount {
    attribute_ids: Vec<i32>,
    candidates: u64,
}

#[derive(Debug, Serialize)]
struct NearSourceEffectReport {
    locus: Locus,
    effect_id: i64,
    selected_source_attribute_ids: Vec<i32>,
    candidate_absent_near_pairs: u64,
    divergent_output_pairs: u64,
    minimum_transition_distance: Option<usize>,
    variants: Vec<NearSourceVariantReport>,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Default)]
struct ModeAccum {
    present_groups: u64,
    present_samples: u64,
    absent_status_state_unobserved_groups: u64,
    absent_identity_group_unobserved_groups: u64,
    controlled_groups: u64,
    sample_comparisons: u64,
    deterministic_groups: u64,
    equal_output_groups: u64,
    divergent_output_groups: u64,
    nondeterministic_groups: u64,
    amount_differences: BTreeMap<i64, u64>,
    normal_value_differences: BTreeMap<i64, u64>,
    amount_ratio_basis_points: BTreeMap<i64, u64>,
    divergent_provider_relationship_groups: BTreeMap<ProviderRelationship, (u64, u64)>,
    nondeterministic_examples: Vec<NondeterministicExample>,
    divergent_examples: Vec<Example>,
    blade_sweep_candidate_projection: Option<BladeSweepCandidateAccum>,
}

#[derive(Debug, Default)]
struct CrossEntityAccum {
    present_groups: u64,
    present_samples: u64,
    absent_formula_state_unobserved_groups: u64,
    controlled_groups: u64,
    sample_comparisons: u64,
    deterministic_groups: u64,
    equal_output_groups: u64,
    divergent_output_groups: u64,
    nondeterministic_groups: u64,
    divergent_examples: Vec<CrossEntityExample>,
}

#[derive(Debug, Default)]
struct CrossEntitySourceTransitionAccum {
    candidate_present_groups: u64,
    present_groups_without_absent_status_state: u64,
    candidate_absent_formula_state_pairs: u64,
    rejected_without_source_attribute_transition: u64,
    rejected_with_unselected_source_attribute_transition: u64,
    rejected_with_excess_target_status_co_transitions: u64,
    target_status_transition_pairs: u64,
    rejected_with_excess_source_status_co_transitions: u64,
    source_status_transition_pairs: u64,
    source_status_transition_distance_counts: BTreeMap<usize, u64>,
    source_status_transition_review_band_pairs: u64,
    source_status_transition_review_band_pairs_without_source_attribute_transition: u64,
    source_status_transition_review_band_pairs_with_unselected_source_attribute_transition: u64,
    source_status_transition_review_band_pairs_with_selected_source_attribute_transition: u64,
    source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit:
        u64,
    controlled_pairs: u64,
    sample_comparisons: u64,
    deterministic_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    nondeterministic_pairs: u64,
    examples: Vec<CrossEntitySourceTransitionExample>,
    all_element_damage_candidate_projection: Option<AllElementDamageCandidateAccum>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FixedPointDamageRounding {
    Floor,
    NearestHalfUp,
}

#[derive(Debug, Clone, Default)]
struct AllElementDamageCandidateAccum {
    deterministic_pairs: u64,
    deterministic_divergent_pairs: u64,
    pairs_with_current_attribute_transition: u64,
    pairs_missing_current_attribute_transition: u64,
    pairs_with_invalid_inputs: u64,
    variants: BTreeMap<FixedPointDamageRounding, CandidateVariantAccum>,
    examples: Vec<AllElementDamageCandidateExample>,
}

#[derive(Debug, Clone, Serialize)]
struct AllElementDamageCandidateProjection {
    model_id: &'static str,
    effect_id: i64,
    current_attribute_id: i32,
    fixed_point_denominator: i64,
    deterministic_pairs: u64,
    deterministic_divergent_pairs: u64,
    pairs_with_current_attribute_transition: u64,
    pairs_missing_current_attribute_transition: u64,
    pairs_with_invalid_inputs: u64,
    variants: Vec<AllElementCandidateVariantStats>,
    examples: Vec<AllElementDamageCandidateExample>,
    candidate_selected: bool,
    exact_damage_stage_binding_proven: bool,
    exact_operation_order_proven: bool,
    exact_integer_rounding_proven: bool,
    conservation_proven: bool,
    formula_authority: bool,
    runtime_authority: bool,
    ui_display_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AllElementCandidateVariantStats {
    rounding: FixedPointDamageRounding,
    compatible_pairs: u64,
    rejected_pairs: u64,
}

#[derive(Debug, Clone, Serialize)]
struct AllElementDamageCandidateExample {
    absent_bonus_basis_points: i64,
    present_bonus_basis_points: i64,
    absent_damage: i64,
    present_damage: i64,
    variants: Vec<AllElementDamageCandidateVariantExample>,
}

#[derive(Debug, Clone, Serialize)]
struct AllElementDamageCandidateVariantExample {
    rounding: FixedPointDamageRounding,
    absent_subtotal_minimum: String,
    absent_subtotal_maximum: String,
    present_subtotal_minimum: String,
    present_subtotal_maximum: String,
    compatible_subtotal_minimum: Option<String>,
    compatible_subtotal_maximum: Option<String>,
    compatible: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CrossEntityProvenance {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    scene_id: Option<i32>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CrossEntityExample {
    status: FormulaStatus,
    locus: Locus,
    present_provenance: CrossEntityProvenance,
    absent_provenance: CrossEntityProvenance,
    present_outcome: Outcome,
    absent_outcome: Outcome,
    present_formula_context: FormulaContext,
    absent_formula_context: FormulaContext,
}

#[derive(Debug, Clone, Serialize)]
struct CrossEntitySourceTransitionExample {
    status: FormulaStatus,
    locus: Locus,
    source_attribute_transitions: Vec<AttributeTransition>,
    target_status_present_only_co_transitions: Vec<FormulaStatus>,
    target_status_absent_only_co_transitions: Vec<FormulaStatus>,
    target_status_transition_distance: usize,
    source_status_present_only_co_transitions: Vec<FormulaStatus>,
    source_status_absent_only_co_transitions: Vec<FormulaStatus>,
    source_status_transition_distance: usize,
    present_provenance: CrossEntityProvenance,
    absent_provenance: CrossEntityProvenance,
    present_outcome: Outcome,
    absent_outcome: Outcome,
    present_formula_context: FormulaContext,
    absent_formula_context: FormulaContext,
}

#[derive(Debug, Serialize)]
struct CrossEntityVariantReport {
    status: FormulaStatus,
    present_groups: u64,
    present_samples: u64,
    absent_formula_state_unobserved_groups: u64,
    controlled_groups: u64,
    sample_comparisons: u64,
    deterministic_groups: u64,
    equal_output_groups: u64,
    divergent_output_groups: u64,
    nondeterministic_groups: u64,
    divergent_examples: Vec<CrossEntityExample>,
}

#[derive(Debug, Serialize)]
struct CrossEntityEffectReport {
    locus: Locus,
    effect_id: i64,
    controlled_groups: u64,
    divergent_output_groups: u64,
    variants: Vec<CrossEntityVariantReport>,
    formula_authority: bool,
    runtime_authority: bool,
    ui_display_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Serialize)]
struct CrossEntitySourceTransitionVariantReport {
    status: FormulaStatus,
    candidate_present_groups: u64,
    present_groups_without_absent_status_state: u64,
    candidate_absent_formula_state_pairs: u64,
    rejected_without_source_attribute_transition: u64,
    rejected_with_unselected_source_attribute_transition: u64,
    rejected_with_excess_target_status_co_transitions: u64,
    target_status_transition_pairs: u64,
    rejected_with_excess_source_status_co_transitions: u64,
    source_status_transition_pairs: u64,
    source_status_transition_distance_counts: Vec<NearDistanceCount>,
    source_status_transition_review_band_pairs: u64,
    source_status_transition_review_band_pairs_without_source_attribute_transition: u64,
    source_status_transition_review_band_pairs_with_unselected_source_attribute_transition: u64,
    source_status_transition_review_band_pairs_with_selected_source_attribute_transition: u64,
    source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit:
        u64,
    controlled_pairs: u64,
    sample_comparisons: u64,
    deterministic_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    nondeterministic_pairs: u64,
    examples: Vec<CrossEntitySourceTransitionExample>,
    all_element_damage_candidate_projection: Option<AllElementDamageCandidateProjection>,
}

#[derive(Debug, Serialize)]
struct CrossEntitySourceTransitionEffectReport {
    locus: Locus,
    effect_id: i64,
    selected_source_attribute_ids: Vec<i32>,
    controlled_pairs: u64,
    divergent_output_pairs: u64,
    variants: Vec<CrossEntitySourceTransitionVariantReport>,
    formula_authority: bool,
    runtime_authority: bool,
    ui_display_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Serialize)]
struct VariantReport {
    status: Status,
    observation: ObservationStats,
    exact_recorded_inputs: ModeStats,
    target_current_hp_excluded_diagnostic: ModeStats,
}

#[derive(Debug, Serialize)]
struct EffectReport {
    locus: Locus,
    effect_id: i64,
    observation: ObservationStats,
    exact_recorded_inputs: ModeStats,
    target_current_hp_excluded_diagnostic: ModeStats,
    variants: Vec<VariantReport>,
}

#[derive(Debug, Serialize)]
struct Summary {
    samples: usize,
    attribute_states: usize,
    status_states: usize,
    distinct_effect_loci: usize,
    distinct_status_variants: usize,
    exact_controlled_groups: u64,
    exact_divergent_output_groups: u64,
    relaxed_controlled_groups: u64,
    relaxed_divergent_output_groups: u64,
    near_controlled_target_pairs: u64,
    near_controlled_target_divergent_pairs: u64,
    near_controlled_source_pairs: u64,
    near_controlled_source_divergent_pairs: u64,
    cross_entity_formula_state_controlled_groups: u64,
    cross_entity_formula_state_divergent_groups: u64,
    cross_entity_source_transition_controlled_pairs: u64,
    cross_entity_source_transition_divergent_pairs: u64,
    cross_entity_source_transition_target_current_hp_excluded_controlled_pairs: u64,
    cross_entity_source_transition_target_current_hp_excluded_divergent_pairs: u64,
    cross_entity_source_transition_target_current_hp_excluded_target_status_transition_controlled_pairs:
        u64,
    cross_entity_source_transition_target_current_hp_excluded_target_status_transition_divergent_pairs:
        u64,
    cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_controlled_pairs:
        u64,
    cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_divergent_pairs:
        u64,
    cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs: u64,
    cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_without_source_attribute_transition:
        u64,
    cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_unselected_source_attribute_transition:
        u64,
    cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_transition:
        u64,
    cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit:
        u64,
}

#[derive(Debug, Serialize)]
struct Output {
    schema_version: u16,
    generated_by: &'static str,
    game_build: Option<String>,
    policy: Policy,
    processing: Processing,
    input: InputEvidence,
    summary: Summary,
    effects: Vec<EffectReport>,
    near_controlled_target_diagnostic: Vec<NearTargetEffectReport>,
    near_controlled_source_attribute_diagnostic: Vec<NearSourceEffectReport>,
    cross_entity_formula_state_diagnostic: Vec<CrossEntityEffectReport>,
    cross_entity_source_transition_diagnostic: Vec<CrossEntitySourceTransitionEffectReport>,
    cross_entity_source_transition_target_current_hp_excluded_diagnostic:
        Vec<CrossEntitySourceTransitionEffectReport>,
    cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic:
        Vec<CrossEntitySourceTransitionEffectReport>,
    cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic:
        Vec<CrossEntitySourceTransitionEffectReport>,
}

#[derive(Debug, Serialize)]
struct InputEvidence {
    path: String,
    bytes: u64,
    sha256: String,
    source_inputs: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Processing {
    memory_limit_mib: usize,
    partition_count: usize,
    largest_partition_bytes: u64,
    largest_cross_entity_partition_bytes: u64,
    measured_peak_working_set_bytes: Option<u64>,
    measured_peak_working_set_mib: Option<f64>,
    measured_peak_within_configured_limit: Option<bool>,
    selected_effect_ids: Vec<i64>,
    selected_source_character_ids: Vec<String>,
    selected_source_entity_uuids: Vec<i64>,
    scanned_samples: usize,
    retained_samples: usize,
    selected_source_transition_attribute_ids: Vec<i32>,
    cross_entity_formula_state_diagnostic_enabled: bool,
    partition_key: &'static str,
}

#[derive(Debug, Serialize)]
struct Policy {
    runtime_authority: bool,
    formula_authority: bool,
    unresolved_evidence_is_hidden: bool,
    packet_container_ordinals_retained_as_evidence: bool,
    packet_container_ordinals_are_formula_identity: bool,
    packet_container_ordinal_proof: &'static str,
    distinct_direct_source_attribute_state_required: bool,
    legacy_distinct_direct_source_without_attribute_state: &'static str,
    exact_mode: &'static str,
    relaxed_mode: &'static str,
    comparison_rule: &'static str,
    divergent_example_context: &'static str,
    candidate_projection_rule: &'static str,
    candidate_projection_authority: bool,
    all_element_damage_candidate_projection_rule: &'static str,
    all_element_damage_candidate_projection_authority: bool,
    near_controlled_diagnostic: &'static str,
    near_controlled_diagnostic_authority: bool,
    near_controlled_source_attribute_diagnostic: &'static str,
    near_controlled_source_attribute_diagnostic_authority: bool,
    cross_entity_formula_state_diagnostic: &'static str,
    cross_entity_formula_state_diagnostic_authority: bool,
    cross_entity_source_transition_diagnostic: &'static str,
    cross_entity_source_transition_diagnostic_authority: bool,
    cross_entity_source_transition_target_current_hp_excluded_diagnostic: &'static str,
    cross_entity_source_transition_target_current_hp_excluded_diagnostic_authority: bool,
    cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic:
        &'static str,
    cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic_authority:
        bool,
    cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic:
        &'static str,
    cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic_authority:
        bool,
    cross_entity_source_status_transition_review_band_diagnostic: &'static str,
    cross_entity_source_status_transition_review_band_diagnostic_authority: bool,
    structurally_absent_remote_skill_cast_packets_required: bool,
    promotion_rule: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Exact,
    TargetCurrentHpExcluded,
}

struct CohortSeed {
    partition_count: usize,
    work_dir: PathBuf,
    memory_limit_bytes: u64,
    cross_entity_formula_state_diagnostic: bool,
    write_exact_partitions: bool,
    source_character_ids: BTreeSet<String>,
    source_entity_uuids: BTreeSet<i64>,
}

impl<'de> DeserializeSeed<'de> for CohortSeed {
    type Value = PartitionedCohort;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(CohortVisitor {
            partition_count: self.partition_count,
            work_dir: self.work_dir,
            memory_limit_bytes: self.memory_limit_bytes,
            cross_entity_formula_state_diagnostic: self.cross_entity_formula_state_diagnostic,
            write_exact_partitions: self.write_exact_partitions,
            source_character_ids: self.source_character_ids,
            source_entity_uuids: self.source_entity_uuids,
        })
    }
}

struct CohortVisitor {
    partition_count: usize,
    work_dir: PathBuf,
    memory_limit_bytes: u64,
    cross_entity_formula_state_diagnostic: bool,
    write_exact_partitions: bool,
    source_character_ids: BTreeSet<String>,
    source_entity_uuids: BTreeSet<i64>,
}

impl<'de> Visitor<'de> for CohortVisitor {
    type Value = PartitionedCohort;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a formula cohort object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let partition_paths = if self.write_exact_partitions {
            (0..self.partition_count)
                .map(|index| self.work_dir.join(format!("partition-{index:04}.ndjson")))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let cross_entity_partition_paths = if self.cross_entity_formula_state_diagnostic {
            (0..self.partition_count)
                .map(|index| {
                    self.work_dir
                        .join(format!("cross-entity-partition-{index:04}.ndjson"))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut writers = partition_paths
            .iter()
            .map(|path| {
                File::create(path)
                    .map(|file| BufWriter::with_capacity(PARTITION_WRITER_BUFFER_BYTES, file))
                    .map_err(serde::de::Error::custom)
            })
            .collect::<Result<Vec<_>, A::Error>>()?;
        let mut cross_entity_writers = cross_entity_partition_paths
            .iter()
            .map(|path| {
                File::create(path)
                    .map(|file| BufWriter::with_capacity(PARTITION_WRITER_BUFFER_BYTES, file))
                    .map_err(serde::de::Error::custom)
            })
            .collect::<Result<Vec<_>, A::Error>>()?;
        let mut game_build = None;
        let mut schema_version = None;
        let mut source_inputs = Vec::new();
        let mut attribute_states = None;
        let mut status_states = None;
        let mut source_status_usage = Vec::new();
        let mut target_status_usage = Vec::new();
        let mut rlogs = Vec::new();
        let mut sessions = Vec::new();
        let mut rlog_ids = HashMap::new();
        let mut session_ids = HashMap::new();
        let mut scanned_sample_count = 0usize;
        let mut sample_count = 0usize;
        let mut saw_samples = false;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "schema_version" => schema_version = Some(map.next_value::<u16>()?),
                "game_build" => game_build = map.next_value()?,
                "inputs" => source_inputs = map.next_value()?,
                "attribute_states" => attribute_states = Some(map.next_value()?),
                "status_states" => status_states = Some(map.next_value()?),
                "samples" => {
                    if saw_samples {
                        return Err(serde::de::Error::duplicate_field("samples"));
                    }
                    saw_samples = true;
                    map.next_value_seed(SamplesSeed {
                        writers: &mut writers,
                        source_status_usage: &mut source_status_usage,
                        target_status_usage: &mut target_status_usage,
                        rlogs: &mut rlogs,
                        sessions: &mut sessions,
                        rlog_ids: &mut rlog_ids,
                        session_ids: &mut session_ids,
                        source_character_ids: &self.source_character_ids,
                        source_entity_uuids: &self.source_entity_uuids,
                        scanned_sample_count: &mut scanned_sample_count,
                        sample_count: &mut sample_count,
                        memory_limit_bytes: self.memory_limit_bytes,
                        cross_entity_writers: &mut cross_entity_writers,
                    })?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        for writer in &mut writers {
            writer.flush().map_err(serde::de::Error::custom)?;
        }
        for writer in &mut cross_entity_writers {
            writer.flush().map_err(serde::de::Error::custom)?;
        }
        drop(writers);
        drop(cross_entity_writers);
        if !saw_samples {
            return Err(serde::de::Error::missing_field("samples"));
        }
        if !formula_cohort_schema_is_supported(schema_version) {
            return Err(serde::de::Error::custom(format!(
                "formula cohort schema must be one of {REQUIRED_FORMULA_COHORT_SCHEMA_VERSIONS:?} so third-party status-provider attribute states are present"
            )));
        }
        let attribute_states =
            attribute_states.ok_or_else(|| serde::de::Error::missing_field("attribute_states"))?;
        let status_states =
            status_states.ok_or_else(|| serde::de::Error::missing_field("status_states"))?;
        let largest_partition_bytes = partition_paths
            .iter()
            .map(|path| fs::metadata(path).map(|metadata| metadata.len()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?
            .into_iter()
            .max()
            .unwrap_or(0);
        let largest_cross_entity_partition_bytes = cross_entity_partition_paths
            .iter()
            .map(|path| fs::metadata(path).map(|metadata| metadata.len()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?
            .into_iter()
            .max()
            .unwrap_or(0);
        Ok(PartitionedCohort {
            game_build,
            source_inputs,
            input_bytes: 0,
            input_sha256: String::new(),
            attribute_states,
            status_states,
            source_status_usage,
            target_status_usage,
            rlogs,
            sessions,
            scanned_sample_count,
            sample_count,
            partition_paths,
            cross_entity_partition_paths,
            work_dir: self.work_dir,
            largest_partition_bytes,
            largest_cross_entity_partition_bytes,
            configured_memory_limit_bytes: self.memory_limit_bytes,
        })
    }
}

fn formula_cohort_schema_is_supported(schema_version: Option<u16>) -> bool {
    schema_version.is_some_and(|version| REQUIRED_FORMULA_COHORT_SCHEMA_VERSIONS.contains(&version))
}

struct SamplesSeed<'a> {
    writers: &'a mut [BufWriter<File>],
    source_status_usage: &'a mut Vec<u64>,
    target_status_usage: &'a mut Vec<u64>,
    rlogs: &'a mut Vec<String>,
    sessions: &'a mut Vec<String>,
    rlog_ids: &'a mut HashMap<String, u32>,
    session_ids: &'a mut HashMap<String, u32>,
    source_character_ids: &'a BTreeSet<String>,
    source_entity_uuids: &'a BTreeSet<i64>,
    scanned_sample_count: &'a mut usize,
    sample_count: &'a mut usize,
    memory_limit_bytes: u64,
    cross_entity_writers: &'a mut [BufWriter<File>],
}

impl<'de> DeserializeSeed<'de> for SamplesSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for SamplesSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an array of damage samples")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(sample) = sequence.next_element::<Sample>()? {
            *self.scanned_sample_count = self.scanned_sample_count.saturating_add(1);
            if *self.scanned_sample_count % MEMORY_CHECK_INTERVAL_SAMPLES == 0 {
                enforce_memory_limit_bytes(
                    self.memory_limit_bytes,
                    "streaming formula-cohort partitioning",
                )
                .map_err(serde::de::Error::custom)?;
            }
            if !source_character_matches(
                sample.source_actor_identity.as_ref(),
                self.source_character_ids,
            ) || !source_entity_matches(sample.source_entity_uuid, self.source_entity_uuids)
            {
                continue;
            }
            increment_usage(self.source_status_usage, sample.source_status_state_id);
            increment_usage(self.target_status_usage, sample.target_status_state_id);
            let identity = Identity {
                rlog_id: intern_value(self.rlog_ids, self.rlogs, &sample.rlog),
                session_id: intern_value(self.session_ids, self.sessions, &sample.session_id),
                run_ordinal: sample.run_ordinal,
                scene_id: sample.scene_id,
                source_entity_uuid: sample.source_entity_uuid,
                direct_source_entity_uuid: sample.direct_source_entity_uuid,
                target_entity_uuid: sample.target_entity_uuid,
                source_actor_identity: sample.source_actor_identity.clone(),
                direct_source_actor_identity: sample.direct_source_actor_identity.clone(),
                target_actor_identity: sample.target_actor_identity.clone(),
                ability_id: sample.ability_id,
                passive_uuid: sample.passive_uuid,
                hit_event_id: sample.hit_event_id,
                damage_source: sample.damage_source,
                damage_type: sample.damage_type,
                critical: sample.critical,
                lucky: sample.lucky,
                packet_input_fingerprint: packet_input_fingerprint(&sample.packet)
                    .map_err(serde::de::Error::custom)?,
            };
            let partition_sample = PartitionSample {
                identity,
                sequence: sample.sequence,
                observed_micros: sample.observed_micros,
                wire_capture_sequence: sample.wire_capture_sequence,
                normalized_packet_inputs: normalized_packet_inputs(&sample.packet),
                outcome: outcome(&sample),
                source_attribute_state_id: sample.source_attribute_state_id,
                direct_source_attribute_state_id: sample.direct_source_attribute_state_id,
                target_attribute_state_id: sample.target_attribute_state_id,
                source_status_state_id: sample.source_status_state_id,
                target_status_state_id: sample.target_status_state_id,
                status_provider_attribute_states: sample.status_provider_attribute_states,
            };
            if !self.writers.is_empty() {
                let partition_index = partition_index(&partition_sample, self.writers.len());
                serde_json::to_writer(&mut self.writers[partition_index], &partition_sample)
                    .map_err(serde::de::Error::custom)?;
                self.writers[partition_index]
                    .write_all(b"\n")
                    .map_err(serde::de::Error::custom)?;
            }
            if !self.cross_entity_writers.is_empty() {
                let cross_entity_partition_index = cross_entity_partition_index(
                    &partition_sample,
                    self.cross_entity_writers.len(),
                );
                serde_json::to_writer(
                    &mut self.cross_entity_writers[cross_entity_partition_index],
                    &partition_sample,
                )
                .map_err(serde::de::Error::custom)?;
                self.cross_entity_writers[cross_entity_partition_index]
                    .write_all(b"\n")
                    .map_err(serde::de::Error::custom)?;
            }
            *self.sample_count = self.sample_count.saturating_add(1);
        }
        Ok(())
    }
}

fn source_character_matches(
    identity: Option<&ActorIdentity>,
    selected_character_ids: &BTreeSet<String>,
) -> bool {
    selected_character_ids.is_empty()
        || identity
            .and_then(|identity| identity.character_id.as_ref())
            .is_some_and(|character_id| selected_character_ids.contains(character_id))
}

fn source_entity_matches(source_entity_uuid: i64, selected_entity_uuids: &BTreeSet<i64>) -> bool {
    selected_entity_uuids.is_empty() || selected_entity_uuids.contains(&source_entity_uuid)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    enforce_memory_limit_bytes(
        memory_limit_bytes(args.memory_limit_mib),
        "analyzer startup",
    )?;
    let cohort = load_partitioned_cohort(&args)?;
    enforce_memory_limit(&cohort, "formula-cohort load")?;
    validate_state_references(&cohort)?;
    enforce_memory_limit(&cohort, "formula-cohort validation")?;
    if let Some(baseline_proof) = &args.baseline_proof {
        return write_cross_entity_baseline_extension(&args, &cohort, baseline_proof);
    }

    let observations = observe_variants(&cohort, &args.effect_ids);
    let removals = status_removals(&cohort.status_states, &args.effect_ids);
    let exact_target_attribute_ids = (0..cohort.attribute_states.len())
        .map(|index| u32::try_from(index).unwrap_or(u32::MAX))
        .collect::<Vec<_>>();
    let relaxed_target_attribute_ids = current_hp_excluded_attribute_ids(&cohort.attribute_states);

    let exact = analyze_mode(
        &cohort,
        &removals,
        &exact_target_attribute_ids,
        Mode::Exact,
        args.example_limit,
    )?;
    let relaxed = analyze_mode(
        &cohort,
        &removals,
        &relaxed_target_attribute_ids,
        Mode::TargetCurrentHpExcluded,
        args.example_limit,
    )?;
    let near_controlled_target_diagnostic = build_near_target_reports(
        analyze_near_target_mode(&cohort, &args.effect_ids, args.example_limit)?,
        args.example_limit,
    );
    let near_controlled_source_attribute_diagnostic =
        if args.source_transition_attribute_ids.is_empty() {
            Vec::new()
        } else {
            build_near_source_reports(
                analyze_near_source_mode(
                    &cohort,
                    &args.effect_ids,
                    &args.source_transition_attribute_ids,
                    args.example_limit,
                )?,
                &args.source_transition_attribute_ids,
                args.example_limit,
            )
        };
    let cross_entity_formula_state_diagnostic = if args.cross_entity_formula_state_diagnostic {
        build_cross_entity_reports(analyze_cross_entity_formula_state_mode(
            &cohort,
            &args.effect_ids,
            args.example_limit,
        )?)
    } else {
        Vec::new()
    };
    let cross_entity_source_transition_diagnostic = if args.cross_entity_formula_state_diagnostic
        && !args.source_transition_attribute_ids.is_empty()
    {
        build_cross_entity_source_transition_reports(
            analyze_cross_entity_source_transition_mode(
                &cohort,
                &args.effect_ids,
                &args.source_transition_attribute_ids,
                &exact_target_attribute_ids,
                false,
                false,
                args.example_limit,
            )?,
            &args.source_transition_attribute_ids,
        )
    } else {
        Vec::new()
    };
    let cross_entity_source_transition_target_current_hp_excluded_diagnostic = if args
        .cross_entity_formula_state_diagnostic
        && !args.source_transition_attribute_ids.is_empty()
    {
        build_cross_entity_source_transition_reports(
            analyze_cross_entity_source_transition_mode(
                &cohort,
                &args.effect_ids,
                &args.source_transition_attribute_ids,
                &relaxed_target_attribute_ids,
                false,
                false,
                args.example_limit,
            )?,
            &args.source_transition_attribute_ids,
        )
    } else {
        Vec::new()
    };
    let cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic =
        if args.cross_entity_formula_state_diagnostic
            && !args.source_transition_attribute_ids.is_empty()
        {
            build_cross_entity_source_transition_reports(
                analyze_cross_entity_source_transition_mode(
                    &cohort,
                    &args.effect_ids,
                    &args.source_transition_attribute_ids,
                    &relaxed_target_attribute_ids,
                    false,
                    true,
                    args.example_limit,
                )?,
                &args.source_transition_attribute_ids,
            )
        } else {
            Vec::new()
        };
    let cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic =
        if args.cross_entity_formula_state_diagnostic
            && !args.source_transition_attribute_ids.is_empty()
        {
            build_cross_entity_source_transition_reports(
                analyze_cross_entity_source_transition_mode(
                    &cohort,
                    &args.effect_ids,
                    &args.source_transition_attribute_ids,
                    &relaxed_target_attribute_ids,
                    true,
                    true,
                    args.example_limit,
                )?,
                &args.source_transition_attribute_ids,
            )
        } else {
            Vec::new()
        };
    let effects = build_reports(
        observations,
        exact,
        relaxed,
        args.example_limit,
        args.histogram_limit,
    );
    let summary = Summary {
        samples: cohort.sample_count,
        attribute_states: cohort.attribute_states.len(),
        status_states: cohort.status_states.len(),
        distinct_effect_loci: effects.len(),
        distinct_status_variants: effects.iter().map(|effect| effect.variants.len()).sum(),
        exact_controlled_groups: effects
            .iter()
            .map(|effect| effect.exact_recorded_inputs.controlled_groups)
            .sum(),
        exact_divergent_output_groups: effects
            .iter()
            .map(|effect| effect.exact_recorded_inputs.divergent_output_groups)
            .sum(),
        relaxed_controlled_groups: effects
            .iter()
            .map(|effect| {
                effect
                    .target_current_hp_excluded_diagnostic
                    .controlled_groups
            })
            .sum(),
        relaxed_divergent_output_groups: effects
            .iter()
            .map(|effect| {
                effect
                    .target_current_hp_excluded_diagnostic
                    .divergent_output_groups
            })
            .sum(),
        near_controlled_target_pairs: near_controlled_target_diagnostic
            .iter()
            .map(|effect| effect.candidate_absent_near_pairs)
            .sum(),
        near_controlled_target_divergent_pairs: near_controlled_target_diagnostic
            .iter()
            .map(|effect| effect.divergent_output_pairs)
            .sum(),
        near_controlled_source_pairs: near_controlled_source_attribute_diagnostic
            .iter()
            .map(|effect| effect.candidate_absent_near_pairs)
            .sum(),
        near_controlled_source_divergent_pairs: near_controlled_source_attribute_diagnostic
            .iter()
            .map(|effect| effect.divergent_output_pairs)
            .sum(),
        cross_entity_formula_state_controlled_groups: cross_entity_formula_state_diagnostic
            .iter()
            .map(|effect| effect.controlled_groups)
            .sum(),
        cross_entity_formula_state_divergent_groups: cross_entity_formula_state_diagnostic
            .iter()
            .map(|effect| effect.divergent_output_groups)
            .sum(),
        cross_entity_source_transition_controlled_pairs: cross_entity_source_transition_diagnostic
            .iter()
            .map(|effect| effect.controlled_pairs)
            .sum(),
        cross_entity_source_transition_divergent_pairs: cross_entity_source_transition_diagnostic
            .iter()
            .map(|effect| effect.divergent_output_pairs)
            .sum(),
        cross_entity_source_transition_target_current_hp_excluded_controlled_pairs:
            cross_entity_source_transition_target_current_hp_excluded_diagnostic
                .iter()
                .map(|effect| effect.controlled_pairs)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_divergent_pairs:
            cross_entity_source_transition_target_current_hp_excluded_diagnostic
                .iter()
                .map(|effect| effect.divergent_output_pairs)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_target_status_transition_controlled_pairs:
            cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic
                .iter()
                .map(|effect| effect.controlled_pairs)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_target_status_transition_divergent_pairs:
            cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic
                .iter()
                .map(|effect| effect.divergent_output_pairs)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_controlled_pairs:
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
                .iter()
                .map(|effect| effect.controlled_pairs)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_divergent_pairs:
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
                .iter()
                .map(|effect| effect.divergent_output_pairs)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs:
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
                .iter()
                .flat_map(|effect| &effect.variants)
                .map(|variant| variant.source_status_transition_review_band_pairs)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_without_source_attribute_transition:
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
                .iter()
                .flat_map(|effect| &effect.variants)
                .map(|variant| variant.source_status_transition_review_band_pairs_without_source_attribute_transition)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_unselected_source_attribute_transition:
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
                .iter()
                .flat_map(|effect| &effect.variants)
                .map(|variant| variant.source_status_transition_review_band_pairs_with_unselected_source_attribute_transition)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_transition:
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
                .iter()
                .flat_map(|effect| &effect.variants)
                .map(|variant| variant.source_status_transition_review_band_pairs_with_selected_source_attribute_transition)
                .sum(),
        cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit:
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
                .iter()
                .flat_map(|effect| &effect.variants)
                .map(|variant| variant.source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit)
                .sum(),
    };
    enforce_memory_limit(&cohort, "final output assembly")?;
    let measured_peak_working_set_bytes = observed_max_working_set_bytes();
    let configured_memory_limit_bytes = memory_limit_bytes(args.memory_limit_mib);
    let output = Output {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-status-effect-counterfactual-proof",
        game_build: cohort.game_build.clone(),
        policy: Policy {
            runtime_authority: false,
            formula_authority: false,
            unresolved_evidence_is_hidden: false,
            packet_container_ordinals_retained_as_evidence: true,
            packet_container_ordinals_are_formula_identity: false,
            packet_container_ordinal_proof: "canonical skill_effect_component_index is produced locally by enumerate() over AoiSyncDelta.skill_effects.damage and component_count is that array's length; hit_event_id and the remaining packet fields retain semantic damage identity",
            distinct_direct_source_attribute_state_required: true,
            legacy_distinct_direct_source_without_attribute_state: "excluded from every comparison; schema-47 cohorts retain the direct attacker's wire-start attributes, while older cohorts remain readable only for samples whose direct source is absent or identical to the credited source",
            exact_mode: "same capture, session, run, scene, source/direct-source/target actor identity, entity UUIDs, ability, passive, hit identity, damage flags, normalized packet formula inputs, complete credited-source, distinct direct-source, and target attributes, every non-candidate source and target status, and the complete at-event attribute states of every provider referenced by a remaining status; a provider referenced only by the removed candidate status is not a formula input in the observed absent state; absent actor or scene fields remain exact absent values",
            relaxed_mode: "same as exact mode except target CurrentHP attribute 11310 is excluded; diagnostic only and never promoted directly",
            comparison_rule: "one exact status record is removed; a pair exists only when that absent status state and an otherwise-identical damage group were both observed",
            divergent_example_context: "each divergent or nondeterministic example embeds both representative samples' normalized packet inputs plus complete source, target, and status-provider attribute states and source/target status states; nondeterministic examples additionally retain bounded present/absent outcome histograms so hidden roll variation remains auditable without loading the source cohort",
            candidate_projection_rule: "for exact current-build target-locus effect 2110092 comparisons only, infer the complete integer base preimage from the absent packet amount and raw physical defense, then test floor, ceil, and round-half-up effective-defense variants of the 650-basis-point pre-mitigation hypothesis; incompatible variants are rejected but compatible variants never gain formula or UI authority here",
            candidate_projection_authority: false,
            all_element_damage_candidate_projection_rule: "for exact current-build source-locus effect 2110125 source-transition comparisons only, require observed all-element Current attribute 13100 values and ordinary unshielded non-clamped HP damage, derive the exact nonnegative integer subtotal preimage interval independently for the absent and present amounts, then intersect those intervals for floor and nearest-half-up fixed-point final-multiplier candidates; compatibility never proves damage-stage binding, operation order, rounding, conservation, formula, runtime, UI, or provider credit",
            all_element_damage_candidate_projection_authority: false,
            near_controlled_diagnostic: "same exact normalized packet identity, source attributes, source statuses, and complete status-provider attribute states; the selected target status is absent, while up to two non-CurrentHP target-attribute transitions and four additional exact target-status co-transitions are enumerated rather than ignored",
            near_controlled_diagnostic_authority: false,
            near_controlled_source_attribute_diagnostic: "opt-in diagnostic requiring the same exact normalized packet identity, target attributes and statuses, and complete status-provider attribute states; the selected source status is absent, every changed source attribute must be one of the explicitly selected exact numeric attribute IDs, and additional exact source-status co-transitions are enumerated rather than ignored",
            near_controlled_source_attribute_diagnostic_authority: false,
            cross_entity_formula_state_diagnostic: "opt-in diagnostic that permits different captures, sessions, runs, scenes, and entity UUIDs only when the exact build, structural actor identity excluding character ID, normalized damage packet inputs, source and target attributes, non-candidate statuses, status magnitude metadata, provider relationship, and observed provider attribute state match; absent actor fields remain exact absent values and are never synthesized",
            cross_entity_formula_state_diagnostic_authority: false,
            cross_entity_source_transition_diagnostic: "opt-in diagnostic that uses the cross-entity formula-state identity, requires the selected source status to be absent with every other source and target status exact, permits one or more source-attribute transitions only when every changed exact numeric attribute ID was explicitly selected, and embeds the full transition vector and both formula contexts; this never proves hidden inputs, operation order, rounding, ownership transfer, or conservation",
            cross_entity_source_transition_diagnostic_authority: false,
            cross_entity_source_transition_target_current_hp_excluded_diagnostic: "same as the cross-entity source-transition diagnostic except target CurrentHP exact numeric attribute 11310 is excluded from the comparison key; every other target attribute and every source/target status remain exact, and the result is diagnostic-only because CurrentHP can be formula-relevant for some damage scripts",
            cross_entity_source_transition_target_current_hp_excluded_diagnostic_authority: false,
            cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic: "same as the target-CurrentHP-excluded cross-entity source-transition diagnostic, while enumerating and permitting at most four exact target-status co-transitions; diagnostic-only because any co-transition can affect mitigation or damage taken and therefore confound the candidate effect",
            cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic_authority: false,
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic: "same as the target-CurrentHP-excluded cross-entity source-transition diagnostic, while enumerating and permitting at most four exact source-status and four exact target-status co-transitions; diagnostic-only because any co-transition can affect damage, mitigation, or status-derived attributes and therefore confound the candidate effect",
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic_authority: false,
            cross_entity_source_status_transition_review_band_diagnostic: "counts rejected candidate-absent pairs with five through twelve exact source-status co-transitions, then separately counts those that also satisfy the selected source-attribute and four-target-status limits; review-band pairs remain rejected and never become controlled counterfactuals",
            cross_entity_source_status_transition_review_band_diagnostic_authority: false,
            structurally_absent_remote_skill_cast_packets_required: false,
            promotion_rule: "this analyzer records controlled counterfactual evidence but cannot by itself promote an attribution rule; the status origin, recipient scope, stacking, exact transform, and conservation proof remain required",
        },
        processing: Processing {
            memory_limit_mib: args.memory_limit_mib,
            partition_count: cohort.partition_paths.len(),
            largest_partition_bytes: cohort.largest_partition_bytes,
            largest_cross_entity_partition_bytes: cohort.largest_cross_entity_partition_bytes,
            measured_peak_working_set_bytes,
            measured_peak_working_set_mib: measured_peak_working_set_bytes
                .map(|bytes| bytes as f64 / (1024.0 * 1024.0)),
            measured_peak_within_configured_limit: measured_peak_working_set_bytes
                .map(|peak| peak <= configured_memory_limit_bytes),
            selected_effect_ids: args.effect_ids.iter().copied().collect(),
            selected_source_character_ids: args.source_character_ids.iter().cloned().collect(),
            selected_source_entity_uuids: args.source_entity_uuids.iter().copied().collect(),
            scanned_samples: cohort.scanned_sample_count,
            retained_samples: cohort.sample_count,
            selected_source_transition_attribute_ids: args
                .source_transition_attribute_ids
                .iter()
                .copied()
                .collect(),
            cross_entity_formula_state_diagnostic_enabled: args
                .cross_entity_formula_state_diagnostic,
            partition_key: "normalized packet identity only; source and target attributes and both status states are intentionally excluded so exact, target-transition, and opt-in source-transition comparisons remain colocated",
        },
        input: InputEvidence {
            path: args.cohort.display().to_string(),
            bytes: cohort.input_bytes,
            sha256: cohort.input_sha256.clone(),
            source_inputs: cohort.source_inputs.clone(),
        },
        summary,
        effects,
        near_controlled_target_diagnostic,
        near_controlled_source_attribute_diagnostic,
        cross_entity_formula_state_diagnostic,
        cross_entity_source_transition_diagnostic,
        cross_entity_source_transition_target_current_hp_excluded_diagnostic,
        cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic,
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic,
    };

    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &output)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn write_cross_entity_baseline_extension(
    args: &Arguments,
    cohort: &PartitionedCohort,
    baseline_proof: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !args.cross_entity_formula_state_diagnostic {
        return Err(
            "--baseline-proof requires --cross-entity-formula-state-diagnostic true".into(),
        );
    }
    if args.output.exists() {
        return Err(format!("output already exists: {}", args.output.display()).into());
    }
    if !cohort.partition_paths.is_empty() || cohort.cross_entity_partition_paths.is_empty() {
        return Err("baseline extension must write only cross-entity partitions".into());
    }
    let baseline_bytes = fs::metadata(baseline_proof)?.len();
    let baseline_sha256 = sha256_file(baseline_proof)?;
    let mut baseline: serde_json::Value =
        serde_json::from_reader(BufReader::new(File::open(baseline_proof)?))?;
    enforce_memory_limit(cohort, "baseline proof load")?;
    if baseline
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(8)
        || baseline
            .get("generated_by")
            .and_then(serde_json::Value::as_str)
            != Some("rlogs-bpsr-status-effect-counterfactual-proof")
    {
        return Err("baseline proof must be an exact schema-8 counterfactual proof".into());
    }
    if baseline
        .get("game_build")
        .and_then(serde_json::Value::as_str)
        != cohort.game_build.as_deref()
    {
        return Err("baseline proof game_build does not match the cohort".into());
    }
    let baseline_input = baseline
        .get("input")
        .and_then(serde_json::Value::as_object)
        .ok_or("baseline proof input missing")?;
    if baseline_input
        .get("bytes")
        .and_then(serde_json::Value::as_u64)
        != Some(cohort.input_bytes)
        || baseline_input
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            != Some(cohort.input_sha256.as_str())
    {
        return Err("baseline proof does not describe the exact cohort bytes and sha256".into());
    }
    if baseline
        .pointer("/summary/samples")
        .and_then(serde_json::Value::as_u64)
        != u64::try_from(cohort.sample_count).ok()
    {
        return Err("baseline proof sample count does not match the streamed cohort".into());
    }
    if baseline
        .pointer("/policy/runtime_authority")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
        || baseline
            .pointer("/policy/formula_authority")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
    {
        return Err("baseline proof must explicitly deny formula and runtime authority".into());
    }
    let baseline_effect_ids = baseline
        .pointer("/processing/selected_effect_ids")
        .and_then(serde_json::Value::as_array)
        .ok_or("baseline selected_effect_ids missing")?
        .iter()
        .map(|value| value.as_i64().ok_or("baseline effect ID is not an integer"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if baseline_effect_ids != args.effect_ids {
        return Err("baseline selected effect IDs do not match this invocation".into());
    }
    let baseline_source_attribute_ids = baseline
        .pointer("/processing/selected_source_transition_attribute_ids")
        .and_then(serde_json::Value::as_array)
        .ok_or("baseline selected source transition attribute IDs missing")?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or("baseline source transition attribute ID is not an i32")
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if baseline_source_attribute_ids != args.source_transition_attribute_ids {
        return Err("baseline source transition attribute IDs do not match this invocation".into());
    }

    let cross_entity_formula_state_diagnostic = build_cross_entity_reports(
        analyze_cross_entity_formula_state_mode(cohort, &args.effect_ids, args.example_limit)?,
    );
    let exact_target_attribute_ids = (0..cohort.attribute_states.len())
        .map(|index| u32::try_from(index).unwrap_or(u32::MAX))
        .collect::<Vec<_>>();
    let relaxed_target_attribute_ids = current_hp_excluded_attribute_ids(&cohort.attribute_states);
    let cross_entity_source_transition_diagnostic = build_cross_entity_source_transition_reports(
        analyze_cross_entity_source_transition_mode(
            cohort,
            &args.effect_ids,
            &args.source_transition_attribute_ids,
            &exact_target_attribute_ids,
            false,
            false,
            args.example_limit,
        )?,
        &args.source_transition_attribute_ids,
    );
    let cross_entity_source_transition_target_current_hp_excluded_diagnostic =
        build_cross_entity_source_transition_reports(
            analyze_cross_entity_source_transition_mode(
                cohort,
                &args.effect_ids,
                &args.source_transition_attribute_ids,
                &relaxed_target_attribute_ids,
                false,
                false,
                args.example_limit,
            )?,
            &args.source_transition_attribute_ids,
        );
    let cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic =
        build_cross_entity_source_transition_reports(
            analyze_cross_entity_source_transition_mode(
                cohort,
                &args.effect_ids,
                &args.source_transition_attribute_ids,
                &relaxed_target_attribute_ids,
                false,
                true,
                args.example_limit,
            )?,
            &args.source_transition_attribute_ids,
        );
    let cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic =
        build_cross_entity_source_transition_reports(
            analyze_cross_entity_source_transition_mode(
                cohort,
                &args.effect_ids,
                &args.source_transition_attribute_ids,
                &relaxed_target_attribute_ids,
                true,
                true,
                args.example_limit,
            )?,
            &args.source_transition_attribute_ids,
        );
    let controlled_groups = cross_entity_formula_state_diagnostic
        .iter()
        .map(|effect| effect.controlled_groups)
        .sum::<u64>();
    let divergent_groups = cross_entity_formula_state_diagnostic
        .iter()
        .map(|effect| effect.divergent_output_groups)
        .sum::<u64>();
    let source_transition_controlled_pairs = cross_entity_source_transition_diagnostic
        .iter()
        .map(|effect| effect.controlled_pairs)
        .sum::<u64>();
    let source_transition_divergent_pairs = cross_entity_source_transition_diagnostic
        .iter()
        .map(|effect| effect.divergent_output_pairs)
        .sum::<u64>();
    let relaxed_source_transition_controlled_pairs =
        cross_entity_source_transition_target_current_hp_excluded_diagnostic
            .iter()
            .map(|effect| effect.controlled_pairs)
            .sum::<u64>();
    let relaxed_source_transition_divergent_pairs =
        cross_entity_source_transition_target_current_hp_excluded_diagnostic
            .iter()
            .map(|effect| effect.divergent_output_pairs)
            .sum::<u64>();
    let target_status_transition_controlled_pairs =
        cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic
            .iter()
            .map(|effect| effect.controlled_pairs)
            .sum::<u64>();
    let target_status_transition_divergent_pairs =
        cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic
            .iter()
            .map(|effect| effect.divergent_output_pairs)
            .sum::<u64>();
    let source_and_target_status_transition_controlled_pairs =
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
            .iter()
            .map(|effect| effect.controlled_pairs)
            .sum::<u64>();
    let source_and_target_status_transition_divergent_pairs =
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
            .iter()
            .map(|effect| effect.divergent_output_pairs)
            .sum::<u64>();
    let source_status_transition_review_band_pairs =
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
            .iter()
            .flat_map(|effect| &effect.variants)
            .map(|variant| variant.source_status_transition_review_band_pairs)
            .sum::<u64>();
    let source_status_transition_review_band_pairs_without_source_attribute_transition =
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
            .iter()
            .flat_map(|effect| &effect.variants)
            .map(|variant| {
                variant
                    .source_status_transition_review_band_pairs_without_source_attribute_transition
            })
            .sum::<u64>();
    let source_status_transition_review_band_pairs_with_unselected_source_attribute_transition =
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
            .iter()
            .flat_map(|effect| &effect.variants)
            .map(|variant| {
                variant
                    .source_status_transition_review_band_pairs_with_unselected_source_attribute_transition
            })
            .sum::<u64>();
    let source_status_transition_review_band_pairs_with_selected_source_attribute_transition =
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
            .iter()
            .flat_map(|effect| &effect.variants)
            .map(|variant| {
                variant
                    .source_status_transition_review_band_pairs_with_selected_source_attribute_transition
            })
            .sum::<u64>();
    let source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit =
        cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic
            .iter()
            .flat_map(|effect| &effect.variants)
            .map(|variant| variant.source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit)
            .sum::<u64>();
    enforce_memory_limit(cohort, "baseline extension output assembly")?;
    let measured_peak_working_set_bytes = observed_max_working_set_bytes();
    let configured_memory_limit_bytes = memory_limit_bytes(args.memory_limit_mib);

    baseline["schema_version"] = serde_json::json!(SCHEMA_VERSION);
    baseline["cross_entity_baseline_proof"] = serde_json::json!({
        "path": baseline_proof.display().to_string(),
        "bytes": baseline_bytes,
        "sha256": baseline_sha256,
        "schema_version": 8
    });
    let policy = baseline
        .get_mut("policy")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("baseline policy object missing")?;
    policy.insert(
        "distinct_direct_source_attribute_state_required".to_owned(),
        serde_json::json!(true),
    );
    policy.insert(
        "legacy_distinct_direct_source_without_attribute_state".to_owned(),
        serde_json::json!("excluded from every comparison; schema-47 cohorts retain the direct attacker's wire-start attributes, while older cohorts remain readable only for samples whose direct source is absent or identical to the credited source"),
    );
    policy.insert(
        "exact_mode".to_owned(),
        serde_json::json!("same capture, session, run, scene, source/direct-source/target actor identity, entity UUIDs, ability, passive, hit identity, damage flags, normalized packet formula inputs, complete credited-source, distinct direct-source, and target attributes, every non-candidate source and target status, and the complete at-event attribute states of every provider referenced by a remaining status; a provider referenced only by the removed candidate status is not a formula input in the observed absent state; absent actor or scene fields remain exact absent values"),
    );
    policy.insert(
        "cross_entity_formula_state_diagnostic".to_owned(),
        serde_json::json!("opt-in diagnostic that permits different captures, sessions, runs, scenes, and entity UUIDs only when the exact build, structural actor identity excluding character ID, normalized damage packet inputs, source and target attributes, non-candidate statuses, status magnitude metadata, provider relationship, and observed provider attribute state match; absent actor fields remain exact absent values and are never synthesized"),
    );
    policy.insert(
        "cross_entity_formula_state_diagnostic_authority".to_owned(),
        serde_json::json!(false),
    );
    policy.insert(
        "all_element_damage_candidate_projection_rule".to_owned(),
        serde_json::json!("for exact current-build source-locus effect 2110125 source-transition comparisons only, require observed all-element Current attribute 13100 values and ordinary unshielded non-clamped HP damage, derive the exact nonnegative integer subtotal preimage interval independently for the absent and present amounts, then intersect those intervals for floor and nearest-half-up fixed-point final-multiplier candidates; compatibility never proves damage-stage binding, operation order, rounding, conservation, formula, runtime, UI, or provider credit"),
    );
    policy.insert(
        "all_element_damage_candidate_projection_authority".to_owned(),
        serde_json::json!(false),
    );
    policy.insert(
        "cross_entity_source_transition_diagnostic".to_owned(),
        serde_json::json!("opt-in diagnostic that uses the cross-entity formula-state identity, requires the selected source status to be absent with every other source and target status exact, permits one or more source-attribute transitions only when every changed exact numeric attribute ID was explicitly selected, and embeds the full transition vector and both formula contexts; this never proves hidden inputs, operation order, rounding, ownership transfer, or conservation"),
    );
    policy.insert(
        "cross_entity_source_transition_diagnostic_authority".to_owned(),
        serde_json::json!(false),
    );
    policy.insert(
        "cross_entity_source_transition_target_current_hp_excluded_diagnostic".to_owned(),
        serde_json::json!("same as the cross-entity source-transition diagnostic except target CurrentHP exact numeric attribute 11310 is excluded from the comparison key; every other target attribute and every source/target status remain exact, and the result is diagnostic-only because CurrentHP can be formula-relevant for some damage scripts"),
    );
    policy.insert(
        "cross_entity_source_transition_target_current_hp_excluded_diagnostic_authority".to_owned(),
        serde_json::json!(false),
    );
    policy.insert(
        "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic".to_owned(),
        serde_json::json!("same as the target-CurrentHP-excluded cross-entity source-transition diagnostic, while enumerating and permitting at most four exact target-status co-transitions; diagnostic-only because any co-transition can affect mitigation or damage taken and therefore confound the candidate effect"),
    );
    policy.insert(
        "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic_authority".to_owned(),
        serde_json::json!(false),
    );
    policy.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic".to_owned(),
        serde_json::json!("same as the target-CurrentHP-excluded cross-entity source-transition diagnostic, while enumerating and permitting at most four exact source-status and four exact target-status co-transitions; diagnostic-only because any co-transition can affect damage, mitigation, or status-derived attributes and therefore confound the candidate effect"),
    );
    policy.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic_authority".to_owned(),
        serde_json::json!(false),
    );
    policy.insert(
        "cross_entity_source_status_transition_review_band_diagnostic".to_owned(),
        serde_json::json!("counts rejected candidate-absent pairs with five through twelve exact source-status co-transitions, then separately counts those that also satisfy the selected source-attribute and four-target-status limits; review-band pairs remain rejected and never become controlled counterfactuals"),
    );
    policy.insert(
        "cross_entity_source_status_transition_review_band_diagnostic_authority".to_owned(),
        serde_json::json!(false),
    );
    policy.insert(
        "structurally_absent_remote_skill_cast_packets_required".to_owned(),
        serde_json::json!(false),
    );
    let processing = baseline
        .get_mut("processing")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("baseline processing object missing")?;
    processing.insert(
        "cross_entity_formula_state_diagnostic_enabled".to_owned(),
        serde_json::json!(true),
    );
    processing.insert(
        "cross_entity_partition_count".to_owned(),
        serde_json::json!(cohort.cross_entity_partition_paths.len()),
    );
    processing.insert(
        "largest_cross_entity_partition_bytes".to_owned(),
        serde_json::json!(cohort.largest_cross_entity_partition_bytes),
    );
    processing.insert(
        "cross_entity_measured_peak_working_set_bytes".to_owned(),
        serde_json::json!(measured_peak_working_set_bytes),
    );
    processing.insert(
        "cross_entity_measured_peak_working_set_mib".to_owned(),
        serde_json::json!(
            measured_peak_working_set_bytes.map(|bytes| bytes as f64 / (1024.0 * 1024.0))
        ),
    );
    processing.insert(
        "cross_entity_measured_peak_within_configured_limit".to_owned(),
        serde_json::json!(
            measured_peak_working_set_bytes.map(|peak| peak <= configured_memory_limit_bytes)
        ),
    );
    let summary = baseline
        .get_mut("summary")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("baseline summary object missing")?;
    summary.insert(
        "cross_entity_formula_state_controlled_groups".to_owned(),
        serde_json::json!(controlled_groups),
    );
    summary.insert(
        "cross_entity_formula_state_divergent_groups".to_owned(),
        serde_json::json!(divergent_groups),
    );
    summary.insert(
        "cross_entity_source_transition_controlled_pairs".to_owned(),
        serde_json::json!(source_transition_controlled_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_divergent_pairs".to_owned(),
        serde_json::json!(source_transition_divergent_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_controlled_pairs".to_owned(),
        serde_json::json!(relaxed_source_transition_controlled_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_divergent_pairs".to_owned(),
        serde_json::json!(relaxed_source_transition_divergent_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_controlled_pairs".to_owned(),
        serde_json::json!(target_status_transition_controlled_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_target_status_transition_divergent_pairs".to_owned(),
        serde_json::json!(target_status_transition_divergent_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_controlled_pairs".to_owned(),
        serde_json::json!(source_and_target_status_transition_controlled_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_divergent_pairs".to_owned(),
        serde_json::json!(source_and_target_status_transition_divergent_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs"
            .to_owned(),
        serde_json::json!(source_status_transition_review_band_pairs),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_without_source_attribute_transition".to_owned(),
        serde_json::json!(source_status_transition_review_band_pairs_without_source_attribute_transition),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_unselected_source_attribute_transition".to_owned(),
        serde_json::json!(source_status_transition_review_band_pairs_with_unselected_source_attribute_transition),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_transition".to_owned(),
        serde_json::json!(source_status_transition_review_band_pairs_with_selected_source_attribute_transition),
    );
    summary.insert(
        "cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit".to_owned(),
        serde_json::json!(source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit),
    );
    baseline["cross_entity_formula_state_diagnostic"] =
        serde_json::to_value(cross_entity_formula_state_diagnostic)?;
    baseline["cross_entity_source_transition_diagnostic"] =
        serde_json::to_value(cross_entity_source_transition_diagnostic)?;
    baseline["cross_entity_source_transition_target_current_hp_excluded_diagnostic"] =
        serde_json::to_value(cross_entity_source_transition_target_current_hp_excluded_diagnostic)?;
    baseline["cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic"] =
        serde_json::to_value(
            cross_entity_source_transition_target_current_hp_excluded_target_status_transition_diagnostic,
        )?;
    baseline["cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic"] =
        serde_json::to_value(
            cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic,
        )?;

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &baseline)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "wrote schema-{SCHEMA_VERSION} baseline extension with {controlled_groups} cross-entity exact controlled groups, {divergent_groups} exact divergent groups, {source_transition_controlled_pairs} exact-target source-transition pairs, {source_transition_divergent_pairs} exact-target divergent pairs, {relaxed_source_transition_controlled_pairs} CurrentHP-excluded source-transition pairs, {relaxed_source_transition_divergent_pairs} CurrentHP-excluded divergent pairs, {target_status_transition_controlled_pairs} target-status-transition pairs, {target_status_transition_divergent_pairs} target-status-transition divergent pairs, {source_and_target_status_transition_controlled_pairs} source-and-target-status-transition pairs, {source_and_target_status_transition_divergent_pairs} source-and-target-status-transition divergent pairs, and {source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit} review-band pairs satisfying every other diagnostic gate to {}",
        args.output.display()
    );
    Ok(())
}

fn load_partitioned_cohort(
    args: &Arguments,
) -> Result<PartitionedCohort, Box<dyn std::error::Error>> {
    let input_bytes = fs::metadata(&args.cohort)?.len();
    let memory_bytes = memory_limit_bytes(args.memory_limit_mib);
    let raw_partition_target = (memory_bytes / RAW_PARTITION_MEMORY_DIVISOR).max(1);
    let estimated_partitions = input_bytes
        .saturating_add(raw_partition_target - 1)
        .checked_div(raw_partition_target)
        .unwrap_or(u64::MAX);
    let partition_count = usize::try_from(estimated_partitions)
        .unwrap_or(MAX_PARTITIONS)
        .clamp(MIN_PARTITIONS, MAX_PARTITIONS)
        .next_power_of_two();
    let output_parent = args.output.parent().unwrap_or_else(|| Path::new("."));
    let output_name = args
        .output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("counterfactual-proof");
    let work_dir = output_parent.join(format!(".{output_name}.partitions-{}", std::process::id()));
    fs::create_dir_all(output_parent)?;
    fs::create_dir(&work_dir).map_err(|error| {
        format!(
            "cannot create isolated partition directory {}: {error}",
            work_dir.display()
        )
    })?;
    let result = CohortSeed {
        partition_count,
        work_dir: work_dir.clone(),
        memory_limit_bytes: memory_bytes,
        cross_entity_formula_state_diagnostic: args.cross_entity_formula_state_diagnostic,
        write_exact_partitions: args.baseline_proof.is_none(),
        source_character_ids: args.source_character_ids.clone(),
        source_entity_uuids: args.source_entity_uuids.clone(),
    }
    .deserialize(&mut serde_json::Deserializer::from_reader(BufReader::new(
        File::open(&args.cohort)?,
    )));
    let mut cohort = match result {
        Ok(cohort) => cohort,
        Err(error) => {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(error.into());
        }
    };
    cohort.input_bytes = input_bytes;
    cohort.input_sha256 = sha256_file(&args.cohort)?;
    enforce_memory_limit(&cohort, "formula-cohort digest")?;
    if cohort.largest_partition_bytes > raw_partition_target.saturating_mul(2) {
        return Err(format!(
            "largest raw partition is {} bytes, exceeding the conservative {} MiB memory plan; rerun with a lower --memory-limit-mib to increase the partition count",
            cohort.largest_partition_bytes, args.memory_limit_mib
        )
        .into());
    }
    if cohort.largest_cross_entity_partition_bytes > raw_partition_target.saturating_mul(2) {
        return Err(format!(
            "largest cross-entity raw partition is {} bytes, exceeding the conservative {} MiB memory plan; rerun with a lower --memory-limit-mib to increase the partition count",
            cohort.largest_cross_entity_partition_bytes, args.memory_limit_mib
        )
        .into());
    }
    Ok(cohort)
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn memory_limit_bytes(memory_limit_mib: usize) -> u64 {
    u64::try_from(memory_limit_mib)
        .unwrap_or(u64::MAX)
        .saturating_mul(1024 * 1024)
}

#[cfg(windows)]
fn current_working_set_bytes() -> Option<u64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };

    let mut counters = unsafe { std::mem::zeroed::<PROCESS_MEMORY_COUNTERS>() };
    counters.cb = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    let result = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (result != 0).then_some(u64::try_from(counters.WorkingSetSize).unwrap_or(u64::MAX))
}

#[cfg(target_os = "linux")]
fn current_working_set_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let resident_kib = status.lines().find_map(|line| {
        let value = line.strip_prefix("VmRSS:")?.trim();
        value.split_whitespace().next()?.parse::<u64>().ok()
    })?;
    Some(resident_kib.saturating_mul(1024))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn current_working_set_bytes() -> Option<u64> {
    None
}

fn observe_current_working_set_bytes() -> Option<u64> {
    let current_bytes = current_working_set_bytes()?;
    OBSERVED_MAX_WORKING_SET_BYTES.fetch_max(current_bytes, Ordering::Relaxed);
    Some(current_bytes)
}

fn observed_max_working_set_bytes() -> Option<u64> {
    let observed_bytes = OBSERVED_MAX_WORKING_SET_BYTES.load(Ordering::Relaxed);
    (observed_bytes != 0).then_some(observed_bytes)
}

fn enforce_memory_limit_bytes(limit_bytes: u64, phase: &str) -> Result<(), String> {
    let Some(current_bytes) = observe_current_working_set_bytes() else {
        return Ok(());
    };
    enforce_observed_working_set_limit(current_bytes, limit_bytes, phase)
}

fn enforce_observed_working_set_limit(
    current_bytes: u64,
    limit_bytes: u64,
    phase: &str,
) -> Result<(), String> {
    if current_bytes > limit_bytes {
        return Err(format!(
            "configured memory limit exceeded during {phase}: current working set {current_bytes} bytes is greater than {limit_bytes} bytes"
        ));
    }
    Ok(())
}

fn enforce_memory_limit(cohort: &PartitionedCohort, phase: &str) -> Result<(), String> {
    enforce_memory_limit_bytes(cohort.configured_memory_limit_bytes, phase)
}

fn validate_state_references(cohort: &PartitionedCohort) -> Result<(), String> {
    if cohort.source_status_usage.len() > cohort.status_states.len()
        || cohort.target_status_usage.len() > cohort.status_states.len()
    {
        return Err("a sample references a missing interned status state".to_owned());
    }
    let validation_paths = if cohort.partition_paths.is_empty() {
        &cohort.cross_entity_partition_paths
    } else {
        &cohort.partition_paths
    };
    for path in validation_paths {
        let reader = BufReader::new(File::open(path).map_err(|error| error.to_string())?);
        for line in reader.lines() {
            let line = line.map_err(|error| error.to_string())?;
            let sample: PartitionSample =
                serde_json::from_str(&line).map_err(|error| error.to_string())?;
            if sample.source_attribute_state_id as usize >= cohort.attribute_states.len()
                || sample
                    .direct_source_attribute_state_id
                    .is_some_and(|id| id as usize >= cohort.attribute_states.len())
                || sample.target_attribute_state_id as usize >= cohort.attribute_states.len()
                || sample.source_status_state_id as usize >= cohort.status_states.len()
                || sample.target_status_state_id as usize >= cohort.status_states.len()
                || sample
                    .status_provider_attribute_states
                    .iter()
                    .any(|provider| {
                        provider
                            .attribute_state_id
                            .is_some_and(|id| id as usize >= cohort.attribute_states.len())
                    })
            {
                return Err(format!(
                    "sample sequence {} references a missing interned state",
                    sample.sequence
                ));
            }
            if sample
                .status_provider_attribute_states
                .windows(2)
                .any(|pair| pair[0].provider_entity_uuid >= pair[1].provider_entity_uuid)
            {
                return Err(format!(
                    "sample sequence {} has duplicate or unsorted provider attribute states",
                    sample.sequence
                ));
            }
        }
        enforce_memory_limit(cohort, "partition reference validation")?;
    }
    Ok(())
}

fn has_required_direct_source_attribute_state(sample: &PartitionSample) -> bool {
    match sample.identity.direct_source_entity_uuid {
        None => true,
        Some(direct_source) if direct_source == sample.identity.source_entity_uuid => true,
        Some(_) => sample.direct_source_attribute_state_id.is_some(),
    }
}

fn increment_usage(usage: &mut Vec<u64>, state_id: u32) {
    let index = state_id as usize;
    if usage.len() <= index {
        usage.resize(index + 1, 0);
    }
    usage[index] = usage[index].saturating_add(1);
}

fn partition_index(sample: &PartitionSample, count: usize) -> usize {
    debug_assert!(count.is_power_of_two());
    let mut hasher = DefaultHasher::new();
    sample.identity.hash(&mut hasher);
    hasher.finish() as usize & (count - 1)
}

fn cross_entity_partition_index(sample: &PartitionSample, count: usize) -> usize {
    debug_assert!(count.is_power_of_two());
    let mut hasher = DefaultHasher::new();
    cross_entity_packet_identity(&sample.identity).hash(&mut hasher);
    hasher.finish() as usize & (count - 1)
}

fn cross_entity_packet_identity(identity: &Identity) -> CrossEntityPacketIdentity {
    CrossEntityPacketIdentity {
        source_actor_identity: identity
            .source_actor_identity
            .as_ref()
            .map(formula_actor_identity),
        direct_source_actor_identity: identity
            .direct_source_actor_identity
            .as_ref()
            .map(formula_actor_identity),
        target_actor_identity: identity
            .target_actor_identity
            .as_ref()
            .map(formula_actor_identity),
        ability_id: identity.ability_id,
        passive_uuid: identity.passive_uuid,
        hit_event_id: identity.hit_event_id,
        damage_source: identity.damage_source,
        damage_type: identity.damage_type,
        critical: identity.critical,
        lucky: identity.lucky,
        packet_input_fingerprint: identity.packet_input_fingerprint,
    }
}

fn formula_actor_identity(identity: &ActorIdentity) -> FormulaActorIdentity {
    FormulaActorIdentity {
        entity_type_id: identity.entity_type_id,
        monster_id: identity.monster_id,
        class_id: identity.class_id,
        specialization_id: identity.specialization_id,
        level: identity.level,
    }
}

fn intern_value(values: &mut HashMap<String, u32>, ordered: &mut Vec<String>, value: &str) -> u32 {
    if let Some(id) = values.get(value) {
        return *id;
    }
    let id = u32::try_from(values.len()).unwrap_or(u32::MAX);
    values.insert(value.to_owned(), id);
    ordered.push(value.to_owned());
    id
}

fn packet_input_fingerprint(packet: &DamagePacketDetail) -> Result<[u8; 32], serde_json::Error> {
    let packet = normalized_packet_inputs(packet);
    let encoded = serde_json::to_vec(&packet)?;
    Ok(Sha256::digest(encoded).into())
}

fn normalized_packet_inputs(packet: &DamagePacketDetail) -> DamagePacketDetail {
    let mut packet = packet.clone();
    packet.dead = None;
    packet.normal_value = None;
    packet.lucky_value = None;
    packet.skill_effect_uuid = None;
    packet.skill_effect_total_damage = None;
    packet.skill_effect_group_index = None;
    packet.skill_effect_component_index = None;
    packet.skill_effect_component_count = None;
    for hit_part in &mut packet.hit_parts {
        hit_part.damage_value = None;
    }
    packet
}

fn observe_variants(
    cohort: &PartitionedCohort,
    effect_ids: &BTreeSet<i64>,
) -> BTreeMap<VariantKey, ObservationStats> {
    let mut observations = BTreeMap::new();
    for (state_id, state) in cohort.status_states.iter().enumerate() {
        for status in state.iter().copied().collect::<BTreeSet<_>>() {
            if !effect_ids.is_empty() && !effect_ids.contains(&status.effect_id) {
                continue;
            }
            let source_usage = cohort
                .source_status_usage
                .get(state_id)
                .copied()
                .unwrap_or(0);
            let target_usage = cohort
                .target_status_usage
                .get(state_id)
                .copied()
                .unwrap_or(0);
            if source_usage > 0 {
                let observation = observations
                    .entry(VariantKey {
                        locus: Locus::Source,
                        status,
                    })
                    .or_insert_with(ObservationStats::default);
                observation.observed_status_states += 1;
                observation.observed_samples += source_usage;
            }
            if target_usage > 0 {
                let observation = observations
                    .entry(VariantKey {
                        locus: Locus::Target,
                        status,
                    })
                    .or_insert_with(ObservationStats::default);
                observation.observed_status_states += 1;
                observation.observed_samples += target_usage;
            }
        }
    }
    observations
}

fn status_removals(
    states: &[Vec<Status>],
    effect_ids: &BTreeSet<i64>,
) -> Vec<Vec<(Status, Option<u32>)>> {
    let lookup = states
        .iter()
        .enumerate()
        .map(|(index, state)| (state.clone(), u32::try_from(index).unwrap_or(u32::MAX)))
        .collect::<HashMap<_, _>>();
    states
        .iter()
        .map(|state| {
            state
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|status| effect_ids.is_empty() || effect_ids.contains(&status.effect_id))
                .map(|status| {
                    let mut absent = state.clone();
                    if let Some(index) = absent.iter().position(|row| *row == status) {
                        absent.remove(index);
                    }
                    (status, lookup.get(&absent).copied())
                })
                .collect()
        })
        .collect()
}

fn current_hp_excluded_attribute_ids(states: &[Vec<Attribute>]) -> Vec<u32> {
    let mut ids = HashMap::<Vec<Attribute>, u32>::new();
    states
        .iter()
        .map(|state| {
            let normalized = state
                .iter()
                .copied()
                .filter(|row| row.attribute_id != CURRENT_HP_ATTRIBUTE_ID)
                .collect::<Vec<_>>();
            if let Some(id) = ids.get(&normalized) {
                *id
            } else {
                let id = u32::try_from(ids.len()).unwrap_or(u32::MAX);
                ids.insert(normalized, id);
                id
            }
        })
        .collect()
}

fn normalized_provider_attribute_states(
    sample: &PartitionSample,
    target_attribute_ids: &[u32],
) -> Vec<ProviderAttributeStateRef> {
    sample
        .status_provider_attribute_states
        .iter()
        .map(|provider| ProviderAttributeStateRef {
            provider_entity_uuid: provider.provider_entity_uuid,
            attribute_state_id: if provider.provider_entity_uuid
                == sample.identity.target_entity_uuid
            {
                provider
                    .attribute_state_id
                    .map(|id| target_attribute_ids[id as usize])
            } else {
                provider.attribute_state_id
            },
        })
        .collect()
}

fn retain_referenced_provider_attribute_states(
    provider_states: &mut Vec<ProviderAttributeStateRef>,
    source_statuses: &[Status],
    target_statuses: &[Status],
) {
    let referenced_providers = source_statuses
        .iter()
        .chain(target_statuses)
        .filter_map(|status| status.source_entity_uuid)
        .collect::<BTreeSet<_>>();
    provider_states
        .retain(|provider| referenced_providers.contains(&provider.provider_entity_uuid));
}

fn analyze_mode(
    cohort: &PartitionedCohort,
    removals: &[Vec<(Status, Option<u32>)>],
    target_attribute_ids: &[u32],
    mode: Mode,
    example_limit: usize,
) -> Result<BTreeMap<VariantKey, ModeAccum>, Box<dyn std::error::Error>> {
    let mut results = BTreeMap::<VariantKey, ModeAccum>::new();
    for partition_path in &cohort.partition_paths {
        let mut groups = HashMap::<ExactKey, Bucket>::new();
        let reader = BufReader::new(File::open(partition_path)?);
        for line in reader.lines() {
            let sample: PartitionSample = serde_json::from_str(&line?)?;
            if !has_required_direct_source_attribute_state(&sample) {
                continue;
            }
            let key = ExactKey {
                identity: sample.identity.clone(),
                source_attribute_state_id: sample.source_attribute_state_id,
                direct_source_attribute_state_id: sample.direct_source_attribute_state_id,
                target_attribute_state_id: target_attribute_ids
                    [sample.target_attribute_state_id as usize],
                source_status_state_id: sample.source_status_state_id,
                target_status_state_id: sample.target_status_state_id,
                status_provider_attribute_states: normalized_provider_attribute_states(
                    &sample,
                    target_attribute_ids,
                ),
            };
            let bucket = groups.entry(key).or_insert_with(|| Bucket {
                representative_sample: Some(sample.clone()),
                ..Bucket::default()
            });
            bucket.sample_count = bucket.sample_count.saturating_add(1);
            *bucket.outcomes.entry(sample.outcome.clone()).or_default() += 1;
            if bucket.sequences.len() < example_limit {
                bucket.sequences.insert(sample.sequence);
            }
        }
        for (present_key, present) in &groups {
            compare_locus(
                cohort,
                &groups,
                present_key,
                present,
                Locus::Source,
                &removals[present_key.source_status_state_id as usize],
                mode,
                example_limit,
                &mut results,
            );
            compare_locus(
                cohort,
                &groups,
                present_key,
                present,
                Locus::Target,
                &removals[present_key.target_status_state_id as usize],
                mode,
                example_limit,
                &mut results,
            );
        }
        enforce_memory_limit(cohort, "exact counterfactual partition analysis")?;
    }
    Ok(results)
}

fn analyze_cross_entity_formula_state_mode(
    cohort: &PartitionedCohort,
    effect_ids: &BTreeSet<i64>,
    example_limit: usize,
) -> Result<BTreeMap<CrossEntityVariantKey, CrossEntityAccum>, Box<dyn std::error::Error>> {
    let mut results = BTreeMap::<CrossEntityVariantKey, CrossEntityAccum>::new();
    for partition_path in &cohort.cross_entity_partition_paths {
        let mut groups = HashMap::<CrossEntityExactKey, Bucket>::new();
        let reader = BufReader::new(File::open(partition_path)?);
        for line in reader.lines() {
            let sample: PartitionSample = serde_json::from_str(&line?)?;
            if !has_required_direct_source_attribute_state(&sample) {
                continue;
            }
            let key = CrossEntityExactKey {
                identity: cross_entity_packet_identity(&sample.identity),
                source_attribute_state_id: sample.source_attribute_state_id,
                direct_source_attribute_state_id: sample.direct_source_attribute_state_id,
                target_attribute_state_id: sample.target_attribute_state_id,
                source_statuses: formula_statuses(
                    &cohort.status_states[sample.source_status_state_id as usize],
                    &sample,
                ),
                target_statuses: formula_statuses(
                    &cohort.status_states[sample.target_status_state_id as usize],
                    &sample,
                ),
            };
            let bucket = groups.entry(key).or_insert_with(|| Bucket {
                representative_sample: Some(sample.clone()),
                ..Bucket::default()
            });
            bucket.sample_count = bucket.sample_count.saturating_add(1);
            *bucket.outcomes.entry(sample.outcome.clone()).or_default() += 1;
            if bucket.sequences.len() < example_limit {
                bucket.sequences.insert(sample.sequence);
            }
        }
        for (present_key, present) in &groups {
            compare_cross_entity_locus(
                cohort,
                &groups,
                present_key,
                present,
                Locus::Source,
                effect_ids,
                example_limit,
                &mut results,
            );
            compare_cross_entity_locus(
                cohort,
                &groups,
                present_key,
                present,
                Locus::Target,
                effect_ids,
                example_limit,
                &mut results,
            );
        }
        enforce_memory_limit(cohort, "cross-entity formula-state partition analysis")?;
    }
    Ok(results)
}

fn analyze_cross_entity_source_transition_mode(
    cohort: &PartitionedCohort,
    effect_ids: &BTreeSet<i64>,
    selected_source_attribute_ids: &BTreeSet<i32>,
    target_attribute_state_ids: &[u32],
    allow_source_status_co_transitions: bool,
    allow_target_status_co_transitions: bool,
    example_limit: usize,
) -> Result<
    BTreeMap<CrossEntityVariantKey, CrossEntitySourceTransitionAccum>,
    Box<dyn std::error::Error>,
> {
    let mut results = BTreeMap::<CrossEntityVariantKey, CrossEntitySourceTransitionAccum>::new();
    if selected_source_attribute_ids.is_empty() {
        return Ok(results);
    }
    for partition_path in &cohort.cross_entity_partition_paths {
        let mut groups = HashMap::<
            CrossEntitySourceTransitionKey,
            HashMap<(u32, Vec<FormulaStatus>, Vec<FormulaStatus>), Bucket>,
        >::new();
        let reader = BufReader::new(File::open(partition_path)?);
        for line in reader.lines() {
            let sample: PartitionSample = serde_json::from_str(&line?)?;
            if !has_required_direct_source_attribute_state(&sample) {
                continue;
            }
            let target_statuses = formula_statuses(
                &cohort.status_states[sample.target_status_state_id as usize],
                &sample,
            );
            let key = CrossEntitySourceTransitionKey {
                identity: cross_entity_packet_identity(&sample.identity),
                direct_source_attribute_state_id: sample.direct_source_attribute_state_id,
                target_attribute_state_id: target_attribute_state_ids
                    [sample.target_attribute_state_id as usize],
                target_statuses: if allow_target_status_co_transitions {
                    Vec::new()
                } else {
                    target_statuses.clone()
                },
            };
            let source_statuses = formula_statuses(
                &cohort.status_states[sample.source_status_state_id as usize],
                &sample,
            );
            let bucket = groups
                .entry(key)
                .or_default()
                .entry((
                    sample.source_attribute_state_id,
                    source_statuses,
                    target_statuses,
                ))
                .or_insert_with(|| Bucket {
                    representative_sample: Some(sample.clone()),
                    ..Bucket::default()
                });
            bucket.sample_count = bucket.sample_count.saturating_add(1);
            *bucket.outcomes.entry(sample.outcome.clone()).or_default() += 1;
            if bucket.sequences.len() < example_limit {
                bucket.sequences.insert(sample.sequence);
            }
        }
        for states in groups.values() {
            for (
                (present_attribute_state_id, present_statuses, present_target_statuses),
                present,
            ) in states
            {
                for status in present_statuses
                    .iter()
                    .filter(|status| {
                        effect_ids.is_empty() || effect_ids.contains(&status.effect_id)
                    })
                    .cloned()
                    .collect::<BTreeSet<_>>()
                {
                    let variant = CrossEntityVariantKey {
                        locus: Locus::Source,
                        status: status.clone(),
                    };
                    let stats = results.entry(variant).or_default();
                    let all_element_candidate_applicable = cohort.game_build.as_deref()
                        == Some("24687926")
                        && status.effect_id == FATAL_SPIRAL_EFFECT_ID;
                    if all_element_candidate_applicable {
                        stats
                            .all_element_damage_candidate_projection
                            .get_or_insert_with(AllElementDamageCandidateAccum::default);
                    }
                    stats.candidate_present_groups =
                        stats.candidate_present_groups.saturating_add(1);
                    let mut absent_statuses = present_statuses.clone();
                    let Some(index) = absent_statuses.iter().position(|row| row == &status) else {
                        continue;
                    };
                    absent_statuses.remove(index);
                    let mut saw_absent_status_state = false;
                    for (
                        (
                            absent_attribute_state_id,
                            candidate_absent_statuses,
                            absent_target_statuses,
                        ),
                        absent,
                    ) in states
                    {
                        if candidate_absent_statuses
                            .iter()
                            .any(|row| row.effect_id == status.effect_id)
                        {
                            continue;
                        }
                        saw_absent_status_state = true;
                        stats.candidate_absent_formula_state_pairs =
                            stats.candidate_absent_formula_state_pairs.saturating_add(1);
                        let (source_present_only, source_absent_only) =
                            formula_status_transitions(&absent_statuses, candidate_absent_statuses);
                        let source_status_transition_distance = source_present_only
                            .len()
                            .saturating_add(source_absent_only.len());
                        *stats
                            .source_status_transition_distance_counts
                            .entry(source_status_transition_distance)
                            .or_default() += 1;
                        if allow_source_status_co_transitions
                            && source_status_transition_distance
                                > NEAR_MAX_TARGET_STATUS_CO_TRANSITIONS
                            && source_status_transition_distance
                                <= SOURCE_STATUS_TRANSITION_REVIEW_LIMIT
                        {
                            stats.source_status_transition_review_band_pairs = stats
                                .source_status_transition_review_band_pairs
                                .saturating_add(1);
                            let review_transitions = attribute_transitions(
                                &cohort.attribute_states[*present_attribute_state_id as usize],
                                &cohort.attribute_states[*absent_attribute_state_id as usize],
                            );
                            if review_transitions.is_empty() {
                                stats.source_status_transition_review_band_pairs_without_source_attribute_transition = stats
                                    .source_status_transition_review_band_pairs_without_source_attribute_transition
                                    .saturating_add(1);
                            } else if review_transitions.iter().any(|transition| {
                                !selected_source_attribute_ids.contains(&transition.attribute_id)
                            }) {
                                stats.source_status_transition_review_band_pairs_with_unselected_source_attribute_transition = stats
                                    .source_status_transition_review_band_pairs_with_unselected_source_attribute_transition
                                    .saturating_add(1);
                            } else {
                                stats.source_status_transition_review_band_pairs_with_selected_source_attribute_transition = stats
                                    .source_status_transition_review_band_pairs_with_selected_source_attribute_transition
                                    .saturating_add(1);
                                let (review_target_present_only, review_target_absent_only) =
                                    formula_status_transitions(
                                        present_target_statuses,
                                        absent_target_statuses,
                                    );
                                if review_target_present_only
                                    .len()
                                    .saturating_add(review_target_absent_only.len())
                                    <= NEAR_MAX_TARGET_STATUS_CO_TRANSITIONS
                                {
                                    stats.source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit = stats
                                        .source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit
                                        .saturating_add(1);
                                }
                            }
                        }
                        if (!allow_source_status_co_transitions
                            && source_status_transition_distance != 0)
                            || source_status_transition_distance
                                > NEAR_MAX_TARGET_STATUS_CO_TRANSITIONS
                        {
                            stats.rejected_with_excess_source_status_co_transitions = stats
                                .rejected_with_excess_source_status_co_transitions
                                .saturating_add(1);
                            continue;
                        }
                        if source_status_transition_distance > 0 {
                            stats.source_status_transition_pairs =
                                stats.source_status_transition_pairs.saturating_add(1);
                        }
                        let transitions = attribute_transitions(
                            &cohort.attribute_states[*present_attribute_state_id as usize],
                            &cohort.attribute_states[*absent_attribute_state_id as usize],
                        );
                        if transitions.is_empty() {
                            stats.rejected_without_source_attribute_transition = stats
                                .rejected_without_source_attribute_transition
                                .saturating_add(1);
                            continue;
                        }
                        if transitions.iter().any(|transition| {
                            !selected_source_attribute_ids.contains(&transition.attribute_id)
                        }) {
                            stats.rejected_with_unselected_source_attribute_transition = stats
                                .rejected_with_unselected_source_attribute_transition
                                .saturating_add(1);
                            continue;
                        }
                        let (target_present_only, target_absent_only) = formula_status_transitions(
                            present_target_statuses,
                            absent_target_statuses,
                        );
                        let target_status_transition_distance = target_present_only
                            .len()
                            .saturating_add(target_absent_only.len());
                        if target_status_transition_distance > NEAR_MAX_TARGET_STATUS_CO_TRANSITIONS
                        {
                            stats.rejected_with_excess_target_status_co_transitions = stats
                                .rejected_with_excess_target_status_co_transitions
                                .saturating_add(1);
                            continue;
                        }
                        if target_status_transition_distance > 0 {
                            stats.target_status_transition_pairs =
                                stats.target_status_transition_pairs.saturating_add(1);
                        }
                        stats.controlled_pairs = stats.controlled_pairs.saturating_add(1);
                        let comparisons = present.sample_count.saturating_mul(absent.sample_count);
                        stats.sample_comparisons =
                            stats.sample_comparisons.saturating_add(comparisons);
                        if present.outcomes.len() != 1 || absent.outcomes.len() != 1 {
                            stats.nondeterministic_pairs =
                                stats.nondeterministic_pairs.saturating_add(1);
                            continue;
                        }
                        stats.deterministic_pairs = stats.deterministic_pairs.saturating_add(1);
                        let present_outcome =
                            present.outcomes.keys().next().expect("one present outcome");
                        let absent_outcome =
                            absent.outcomes.keys().next().expect("one absent outcome");
                        let present_sample = present
                            .representative_sample
                            .as_ref()
                            .expect("every populated bucket has a representative sample");
                        let absent_sample = absent
                            .representative_sample
                            .as_ref()
                            .expect("every populated bucket has a representative sample");
                        if present_outcome == absent_outcome {
                            stats.equal_output_pairs = stats.equal_output_pairs.saturating_add(1);
                        } else {
                            stats.divergent_output_pairs =
                                stats.divergent_output_pairs.saturating_add(1);
                        }
                        if all_element_candidate_applicable {
                            evaluate_all_element_damage_candidate(
                                &transitions,
                                present_outcome,
                                absent_outcome,
                                example_limit,
                                stats
                                    .all_element_damage_candidate_projection
                                    .as_mut()
                                    .expect("applicable all-element accumulator exists"),
                            );
                        }
                        if stats.examples.len() < example_limit {
                            stats.examples.push(CrossEntitySourceTransitionExample {
                                status: status.clone(),
                                locus: Locus::Source,
                                source_attribute_transitions: transitions,
                                target_status_present_only_co_transitions: target_present_only,
                                target_status_absent_only_co_transitions: target_absent_only,
                                target_status_transition_distance,
                                source_status_present_only_co_transitions: source_present_only,
                                source_status_absent_only_co_transitions: source_absent_only,
                                source_status_transition_distance,
                                present_provenance: cross_entity_provenance(cohort, present_sample),
                                absent_provenance: cross_entity_provenance(cohort, absent_sample),
                                present_outcome: present_outcome.clone(),
                                absent_outcome: absent_outcome.clone(),
                                present_formula_context: formula_context(cohort, present_sample),
                                absent_formula_context: formula_context(cohort, absent_sample),
                            });
                        }
                    }
                    if !saw_absent_status_state {
                        stats.present_groups_without_absent_status_state = stats
                            .present_groups_without_absent_status_state
                            .saturating_add(1);
                    }
                }
            }
        }
        enforce_memory_limit(cohort, "cross-entity source-transition partition analysis")?;
    }
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
fn compare_cross_entity_locus(
    cohort: &PartitionedCohort,
    groups: &HashMap<CrossEntityExactKey, Bucket>,
    present_key: &CrossEntityExactKey,
    present: &Bucket,
    locus: Locus,
    effect_ids: &BTreeSet<i64>,
    example_limit: usize,
    results: &mut BTreeMap<CrossEntityVariantKey, CrossEntityAccum>,
) {
    let statuses = match locus {
        Locus::Source => &present_key.source_statuses,
        Locus::Target => &present_key.target_statuses,
    };
    for status in statuses
        .iter()
        .filter(|status| effect_ids.is_empty() || effect_ids.contains(&status.effect_id))
        .cloned()
        .collect::<BTreeSet<_>>()
    {
        let variant = CrossEntityVariantKey {
            locus,
            status: status.clone(),
        };
        let stats = results.entry(variant).or_default();
        stats.present_groups = stats.present_groups.saturating_add(1);
        stats.present_samples = stats.present_samples.saturating_add(present.sample_count);
        let mut absent_key = present_key.clone();
        let absent_statuses = match locus {
            Locus::Source => &mut absent_key.source_statuses,
            Locus::Target => &mut absent_key.target_statuses,
        };
        let Some(index) = absent_statuses.iter().position(|row| row == &status) else {
            continue;
        };
        absent_statuses.remove(index);
        let Some(absent) = groups.get(&absent_key) else {
            stats.absent_formula_state_unobserved_groups = stats
                .absent_formula_state_unobserved_groups
                .saturating_add(1);
            continue;
        };
        stats.controlled_groups = stats.controlled_groups.saturating_add(1);
        let comparisons = present.sample_count.saturating_mul(absent.sample_count);
        stats.sample_comparisons = stats.sample_comparisons.saturating_add(comparisons);
        if present.outcomes.len() != 1 || absent.outcomes.len() != 1 {
            stats.nondeterministic_groups = stats.nondeterministic_groups.saturating_add(1);
            continue;
        }
        stats.deterministic_groups = stats.deterministic_groups.saturating_add(1);
        let present_outcome = present.outcomes.keys().next().expect("one present outcome");
        let absent_outcome = absent.outcomes.keys().next().expect("one absent outcome");
        if present_outcome == absent_outcome {
            stats.equal_output_groups = stats.equal_output_groups.saturating_add(1);
            continue;
        }
        stats.divergent_output_groups = stats.divergent_output_groups.saturating_add(1);
        if stats.divergent_examples.len() < example_limit {
            let present_sample = present
                .representative_sample
                .as_ref()
                .expect("every populated bucket has a representative sample");
            let absent_sample = absent
                .representative_sample
                .as_ref()
                .expect("every populated bucket has a representative sample");
            stats.divergent_examples.push(CrossEntityExample {
                status: status.clone(),
                locus,
                present_provenance: cross_entity_provenance(cohort, present_sample),
                absent_provenance: cross_entity_provenance(cohort, absent_sample),
                present_outcome: present_outcome.clone(),
                absent_outcome: absent_outcome.clone(),
                present_formula_context: formula_context(cohort, present_sample),
                absent_formula_context: formula_context(cohort, absent_sample),
            });
        }
    }
}

fn formula_statuses(statuses: &[Status], sample: &PartitionSample) -> Vec<FormulaStatus> {
    let mut statuses = statuses
        .iter()
        .copied()
        .map(|status| formula_status(status, sample))
        .collect::<Vec<_>>();
    statuses.sort();
    statuses
}

fn formula_status_transitions(
    present: &[FormulaStatus],
    absent: &[FormulaStatus],
) -> (Vec<FormulaStatus>, Vec<FormulaStatus>) {
    let present = present.iter().cloned().collect::<BTreeSet<_>>();
    let absent = absent.iter().cloned().collect::<BTreeSet<_>>();
    (
        present.difference(&absent).cloned().collect(),
        absent.difference(&present).cloned().collect(),
    )
}

fn formula_status(status: Status, sample: &PartitionSample) -> FormulaStatus {
    let provider_state = status.source_entity_uuid.and_then(|provider_entity_uuid| {
        sample
            .status_provider_attribute_states
            .iter()
            .find(|row| row.provider_entity_uuid == provider_entity_uuid)
    });
    FormulaStatus {
        effect_id: status.effect_id,
        provider_relationship: provider_relationship(status, &sample.identity),
        provider_attribute_state_observed: provider_state
            .and_then(|row| row.attribute_state_id)
            .is_some(),
        provider_attribute_state_id: provider_state.and_then(|row| row.attribute_state_id),
        stacks: status.stacks,
        level: status.level,
        origin_source_type_id: status.origin_source_type_id,
        origin_source_config_id: status.origin_source_config_id,
    }
}

fn cross_entity_provenance(
    cohort: &PartitionedCohort,
    sample: &PartitionSample,
) -> CrossEntityProvenance {
    CrossEntityProvenance {
        rlog: cohort.rlogs[sample.identity.rlog_id as usize].clone(),
        session_id: cohort.sessions[sample.identity.session_id as usize].clone(),
        run_ordinal: sample.identity.run_ordinal,
        scene_id: sample.identity.scene_id,
        source_entity_uuid: sample.identity.source_entity_uuid,
        direct_source_entity_uuid: sample.identity.direct_source_entity_uuid,
        target_entity_uuid: sample.identity.target_entity_uuid,
        sequence: sample.sequence,
    }
}

fn build_cross_entity_reports(
    values: BTreeMap<CrossEntityVariantKey, CrossEntityAccum>,
) -> Vec<CrossEntityEffectReport> {
    let mut grouped = BTreeMap::<(Locus, i64), Vec<CrossEntityVariantReport>>::new();
    for (key, stats) in values {
        grouped
            .entry((key.locus, key.status.effect_id))
            .or_default()
            .push(CrossEntityVariantReport {
                status: key.status,
                present_groups: stats.present_groups,
                present_samples: stats.present_samples,
                absent_formula_state_unobserved_groups: stats
                    .absent_formula_state_unobserved_groups,
                controlled_groups: stats.controlled_groups,
                sample_comparisons: stats.sample_comparisons,
                deterministic_groups: stats.deterministic_groups,
                equal_output_groups: stats.equal_output_groups,
                divergent_output_groups: stats.divergent_output_groups,
                nondeterministic_groups: stats.nondeterministic_groups,
                divergent_examples: stats.divergent_examples,
            });
    }
    grouped
        .into_iter()
        .map(|((locus, effect_id), mut variants)| {
            variants.sort_by(|left, right| {
                right
                    .present_samples
                    .cmp(&left.present_samples)
                    .then_with(|| left.status.cmp(&right.status))
            });
            CrossEntityEffectReport {
                locus,
                effect_id,
                controlled_groups: variants.iter().map(|row| row.controlled_groups).sum(),
                divergent_output_groups: variants
                    .iter()
                    .map(|row| row.divergent_output_groups)
                    .sum(),
                variants,
                formula_authority: false,
                runtime_authority: false,
                ui_display_authority: false,
                provider_rdps_credit_allowed: false,
            }
        })
        .collect()
}

fn build_cross_entity_source_transition_reports(
    values: BTreeMap<CrossEntityVariantKey, CrossEntitySourceTransitionAccum>,
    selected_source_attribute_ids: &BTreeSet<i32>,
) -> Vec<CrossEntitySourceTransitionEffectReport> {
    let mut grouped =
        BTreeMap::<(Locus, i64), Vec<CrossEntitySourceTransitionVariantReport>>::new();
    for (key, stats) in values {
        grouped
            .entry((key.locus, key.status.effect_id))
            .or_default()
            .push(CrossEntitySourceTransitionVariantReport {
                status: key.status,
                candidate_present_groups: stats.candidate_present_groups,
                present_groups_without_absent_status_state: stats
                    .present_groups_without_absent_status_state,
                candidate_absent_formula_state_pairs: stats.candidate_absent_formula_state_pairs,
                rejected_without_source_attribute_transition: stats
                    .rejected_without_source_attribute_transition,
                rejected_with_unselected_source_attribute_transition: stats
                    .rejected_with_unselected_source_attribute_transition,
                rejected_with_excess_target_status_co_transitions: stats
                    .rejected_with_excess_target_status_co_transitions,
                target_status_transition_pairs: stats.target_status_transition_pairs,
                rejected_with_excess_source_status_co_transitions: stats
                    .rejected_with_excess_source_status_co_transitions,
                source_status_transition_pairs: stats.source_status_transition_pairs,
                source_status_transition_distance_counts: stats
                    .source_status_transition_distance_counts
                    .into_iter()
                    .map(|(transition_distance, pairs)| NearDistanceCount {
                        transition_distance,
                        pairs,
                    })
                    .collect(),
                source_status_transition_review_band_pairs: stats
                    .source_status_transition_review_band_pairs,
                source_status_transition_review_band_pairs_without_source_attribute_transition:
                    stats.source_status_transition_review_band_pairs_without_source_attribute_transition,
                source_status_transition_review_band_pairs_with_unselected_source_attribute_transition:
                    stats.source_status_transition_review_band_pairs_with_unselected_source_attribute_transition,
                source_status_transition_review_band_pairs_with_selected_source_attribute_transition:
                    stats.source_status_transition_review_band_pairs_with_selected_source_attribute_transition,
                source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit:
                    stats.source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit,
                controlled_pairs: stats.controlled_pairs,
                sample_comparisons: stats.sample_comparisons,
                deterministic_pairs: stats.deterministic_pairs,
                equal_output_pairs: stats.equal_output_pairs,
                divergent_output_pairs: stats.divergent_output_pairs,
                nondeterministic_pairs: stats.nondeterministic_pairs,
                examples: stats.examples,
                all_element_damage_candidate_projection: stats
                    .all_element_damage_candidate_projection
                    .map(finalize_all_element_damage_candidate),
            });
    }
    grouped
        .into_iter()
        .map(|((locus, effect_id), mut variants)| {
            variants.sort_by(|left, right| {
                right
                    .candidate_present_groups
                    .cmp(&left.candidate_present_groups)
                    .then_with(|| left.status.cmp(&right.status))
            });
            CrossEntitySourceTransitionEffectReport {
                locus,
                effect_id,
                selected_source_attribute_ids: selected_source_attribute_ids
                    .iter()
                    .copied()
                    .collect(),
                controlled_pairs: variants.iter().map(|row| row.controlled_pairs).sum(),
                divergent_output_pairs: variants.iter().map(|row| row.divergent_output_pairs).sum(),
                variants,
                formula_authority: false,
                runtime_authority: false,
                ui_display_authority: false,
                provider_rdps_credit_allowed: false,
            }
        })
        .collect()
}

fn analyze_near_target_mode(
    cohort: &PartitionedCohort,
    effect_ids: &BTreeSet<i64>,
    example_limit: usize,
) -> Result<BTreeMap<VariantKey, NearTargetAccum>, Box<dyn std::error::Error>> {
    let mut results = BTreeMap::<VariantKey, NearTargetAccum>::new();
    for partition_path in &cohort.partition_paths {
        let mut groups = HashMap::<NearTargetKey, HashMap<(u32, u32), Bucket>>::new();
        let reader = BufReader::new(File::open(partition_path)?);
        for line in reader.lines() {
            let sample: PartitionSample = serde_json::from_str(&line?)?;
            if !has_required_direct_source_attribute_state(&sample) {
                continue;
            }
            let key = NearTargetKey {
                identity: sample.identity.clone(),
                source_attribute_state_id: sample.source_attribute_state_id,
                direct_source_attribute_state_id: sample.direct_source_attribute_state_id,
                source_status_state_id: sample.source_status_state_id,
                status_provider_attribute_states: sample.status_provider_attribute_states.clone(),
            };
            let bucket = groups
                .entry(key)
                .or_default()
                .entry((
                    sample.target_attribute_state_id,
                    sample.target_status_state_id,
                ))
                .or_insert_with(|| Bucket {
                    representative_sample: Some(sample.clone()),
                    ..Bucket::default()
                });
            bucket.sample_count = bucket.sample_count.saturating_add(1);
            *bucket.outcomes.entry(sample.outcome.clone()).or_default() += 1;
            if bucket.sequences.len() < example_limit {
                bucket.sequences.insert(sample.sequence);
            }
        }

        for (near_key, states) in &groups {
            for ((present_attribute_state_id, present_status_state_id), present) in states {
                let present_statuses = &cohort.status_states[*present_status_state_id as usize];
                for candidate_status in present_statuses.iter().copied().filter(|status| {
                    effect_ids.is_empty() || effect_ids.contains(&status.effect_id)
                }) {
                    let variant = VariantKey {
                        locus: Locus::Target,
                        status: candidate_status,
                    };
                    let stats = results.entry(variant).or_default();
                    stats.candidate_present_groups =
                        stats.candidate_present_groups.saturating_add(1);

                    for ((absent_attribute_state_id, absent_status_state_id), absent) in states {
                        if present_attribute_state_id == absent_attribute_state_id
                            && present_status_state_id == absent_status_state_id
                        {
                            continue;
                        }
                        let absent_statuses =
                            &cohort.status_states[*absent_status_state_id as usize];
                        if absent_statuses
                            .iter()
                            .any(|status| status.effect_id == candidate_status.effect_id)
                        {
                            continue;
                        }
                        let attribute_transitions = target_attribute_transitions(
                            &cohort.attribute_states[*present_attribute_state_id as usize],
                            &cohort.attribute_states[*absent_attribute_state_id as usize],
                        );
                        if attribute_transitions.len() > NEAR_MAX_TARGET_ATTRIBUTE_TRANSITIONS {
                            continue;
                        }
                        let (present_only, absent_only) = status_co_transitions(
                            present_statuses,
                            absent_statuses,
                            candidate_status,
                        );
                        let status_transition_count =
                            present_only.len().saturating_add(absent_only.len());
                        if status_transition_count > NEAR_MAX_TARGET_STATUS_CO_TRANSITIONS {
                            continue;
                        }
                        let distance = attribute_transitions
                            .len()
                            .saturating_add(status_transition_count);
                        if distance == 0 {
                            continue;
                        }

                        stats.candidate_absent_near_pairs =
                            stats.candidate_absent_near_pairs.saturating_add(1);
                        let comparisons = present.sample_count.saturating_mul(absent.sample_count);
                        stats.sample_comparisons =
                            stats.sample_comparisons.saturating_add(comparisons);
                        *stats
                            .transition_distance_counts
                            .entry(distance)
                            .or_default() += 1;
                        stats.minimum_transition_distance = Some(
                            stats
                                .minimum_transition_distance
                                .map_or(distance, |current| current.min(distance)),
                        );
                        if present.outcomes.len() != 1 || absent.outcomes.len() != 1 {
                            stats.nondeterministic_pairs =
                                stats.nondeterministic_pairs.saturating_add(1);
                            continue;
                        }
                        stats.deterministic_pairs = stats.deterministic_pairs.saturating_add(1);
                        let present_outcome =
                            present.outcomes.keys().next().expect("one present outcome");
                        let absent_outcome =
                            absent.outcomes.keys().next().expect("one absent outcome");
                        let outputs_equal = present_outcome == absent_outcome;
                        if outputs_equal {
                            stats.equal_output_pairs = stats.equal_output_pairs.saturating_add(1);
                        } else {
                            stats.divergent_output_pairs =
                                stats.divergent_output_pairs.saturating_add(1);
                        }
                        if stats.examples.len() >= example_limit {
                            continue;
                        }
                        let present_sample = present
                            .representative_sample
                            .as_ref()
                            .expect("every populated bucket has a representative sample");
                        let absent_sample = absent
                            .representative_sample
                            .as_ref()
                            .expect("every populated bucket has a representative sample");
                        stats.examples.push(NearTargetExample {
                            rlog: cohort.rlogs[near_key.identity.rlog_id as usize].clone(),
                            session_id: cohort.sessions[near_key.identity.session_id as usize]
                                .clone(),
                            run_ordinal: near_key.identity.run_ordinal,
                            source_entity_uuid: near_key.identity.source_entity_uuid,
                            target_entity_uuid: near_key.identity.target_entity_uuid,
                            ability_id: near_key.identity.ability_id,
                            candidate_status,
                            provider_relationship: provider_relationship(
                                candidate_status,
                                &near_key.identity,
                            ),
                            target_attribute_transitions_excluding_current_hp:
                                attribute_transitions,
                            target_status_present_only_co_transitions: present_only,
                            target_status_absent_only_co_transitions: absent_only,
                            transition_distance: distance,
                            outputs_equal,
                            present_outcome: present_outcome.clone(),
                            absent_outcome: absent_outcome.clone(),
                            present_sequences: present.sequences.iter().copied().collect(),
                            absent_sequences: absent.sequences.iter().copied().collect(),
                            present_formula_context: formula_context(cohort, present_sample),
                            absent_formula_context: formula_context(cohort, absent_sample),
                        });
                    }
                }
            }
        }
        enforce_memory_limit(cohort, "near-target counterfactual partition analysis")?;
    }
    Ok(results)
}

fn analyze_near_source_mode(
    cohort: &PartitionedCohort,
    effect_ids: &BTreeSet<i64>,
    source_transition_attribute_ids: &BTreeSet<i32>,
    example_limit: usize,
) -> Result<BTreeMap<VariantKey, NearSourceAccum>, Box<dyn std::error::Error>> {
    let mut results = BTreeMap::<VariantKey, NearSourceAccum>::new();
    for partition_path in &cohort.partition_paths {
        let mut groups = HashMap::<NearSourceKey, HashMap<(u32, u32), Bucket>>::new();
        let reader = BufReader::new(File::open(partition_path)?);
        for line in reader.lines() {
            let sample: PartitionSample = serde_json::from_str(&line?)?;
            if !has_required_direct_source_attribute_state(&sample) {
                continue;
            }
            let key = NearSourceKey {
                identity: sample.identity.clone(),
                direct_source_attribute_state_id: sample.direct_source_attribute_state_id,
                target_attribute_state_id: sample.target_attribute_state_id,
                target_status_state_id: sample.target_status_state_id,
                status_provider_attribute_states: sample.status_provider_attribute_states.clone(),
            };
            let bucket = groups
                .entry(key)
                .or_default()
                .entry((
                    sample.source_attribute_state_id,
                    sample.source_status_state_id,
                ))
                .or_insert_with(|| Bucket {
                    representative_sample: Some(sample.clone()),
                    ..Bucket::default()
                });
            bucket.sample_count = bucket.sample_count.saturating_add(1);
            *bucket.outcomes.entry(sample.outcome.clone()).or_default() += 1;
            if bucket.sequences.len() < example_limit {
                bucket.sequences.insert(sample.sequence);
            }
        }

        for (near_key, states) in &groups {
            for ((present_attribute_state_id, present_status_state_id), present) in states {
                let present_statuses = &cohort.status_states[*present_status_state_id as usize];
                for candidate_status in present_statuses.iter().copied().filter(|status| {
                    effect_ids.is_empty() || effect_ids.contains(&status.effect_id)
                }) {
                    let variant = VariantKey {
                        locus: Locus::Source,
                        status: candidate_status,
                    };
                    let stats = results.entry(variant).or_default();
                    stats.candidate_present_groups =
                        stats.candidate_present_groups.saturating_add(1);

                    let mut saw_effect_absent_identity_state = false;
                    for ((absent_attribute_state_id, absent_status_state_id), absent) in states {
                        if present_attribute_state_id == absent_attribute_state_id
                            && present_status_state_id == absent_status_state_id
                        {
                            continue;
                        }
                        let absent_statuses =
                            &cohort.status_states[*absent_status_state_id as usize];
                        if absent_statuses
                            .iter()
                            .any(|status| status.effect_id == candidate_status.effect_id)
                        {
                            continue;
                        }
                        saw_effect_absent_identity_state = true;
                        stats.effect_absent_identity_state_candidates = stats
                            .effect_absent_identity_state_candidates
                            .saturating_add(1);
                        let attribute_transitions = attribute_transitions(
                            &cohort.attribute_states[*present_attribute_state_id as usize],
                            &cohort.attribute_states[*absent_attribute_state_id as usize],
                        );
                        if attribute_transitions.is_empty() {
                            stats.rejected_without_source_attribute_transition = stats
                                .rejected_without_source_attribute_transition
                                .saturating_add(1);
                            continue;
                        }
                        if attribute_transitions.iter().any(|transition| {
                            !source_transition_attribute_ids.contains(&transition.attribute_id)
                        }) {
                            stats.rejected_with_unselected_source_attribute_transition = stats
                                .rejected_with_unselected_source_attribute_transition
                                .saturating_add(1);
                            let attribute_ids = attribute_transitions
                                .iter()
                                .map(|transition| transition.attribute_id)
                                .collect::<Vec<_>>();
                            *stats
                                .rejected_source_attribute_transition_sets
                                .entry(attribute_ids)
                                .or_default() += 1;
                            continue;
                        }
                        let (present_only, absent_only) = status_co_transitions(
                            present_statuses,
                            absent_statuses,
                            candidate_status,
                        );
                        let status_transition_count =
                            present_only.len().saturating_add(absent_only.len());
                        if status_transition_count > NEAR_MAX_TARGET_STATUS_CO_TRANSITIONS {
                            stats.rejected_with_excess_source_status_co_transitions = stats
                                .rejected_with_excess_source_status_co_transitions
                                .saturating_add(1);
                            continue;
                        }
                        let distance = attribute_transitions
                            .len()
                            .saturating_add(status_transition_count);

                        stats.candidate_absent_near_pairs =
                            stats.candidate_absent_near_pairs.saturating_add(1);
                        let comparisons = present.sample_count.saturating_mul(absent.sample_count);
                        stats.sample_comparisons =
                            stats.sample_comparisons.saturating_add(comparisons);
                        *stats
                            .transition_distance_counts
                            .entry(distance)
                            .or_default() += 1;
                        stats.minimum_transition_distance = Some(
                            stats
                                .minimum_transition_distance
                                .map_or(distance, |current| current.min(distance)),
                        );
                        if present.outcomes.len() != 1 || absent.outcomes.len() != 1 {
                            stats.nondeterministic_pairs =
                                stats.nondeterministic_pairs.saturating_add(1);
                            continue;
                        }
                        stats.deterministic_pairs = stats.deterministic_pairs.saturating_add(1);
                        let present_outcome =
                            present.outcomes.keys().next().expect("one present outcome");
                        let absent_outcome =
                            absent.outcomes.keys().next().expect("one absent outcome");
                        let outputs_equal = present_outcome == absent_outcome;
                        if outputs_equal {
                            stats.equal_output_pairs = stats.equal_output_pairs.saturating_add(1);
                        } else {
                            stats.divergent_output_pairs =
                                stats.divergent_output_pairs.saturating_add(1);
                        }
                        if stats.examples.len() >= example_limit {
                            continue;
                        }
                        let present_sample = present
                            .representative_sample
                            .as_ref()
                            .expect("every populated bucket has a representative sample");
                        let absent_sample = absent
                            .representative_sample
                            .as_ref()
                            .expect("every populated bucket has a representative sample");
                        stats.examples.push(NearSourceExample {
                            rlog: cohort.rlogs[near_key.identity.rlog_id as usize].clone(),
                            session_id: cohort.sessions[near_key.identity.session_id as usize]
                                .clone(),
                            run_ordinal: near_key.identity.run_ordinal,
                            source_entity_uuid: near_key.identity.source_entity_uuid,
                            target_entity_uuid: near_key.identity.target_entity_uuid,
                            ability_id: near_key.identity.ability_id,
                            candidate_status,
                            provider_relationship: provider_relationship(
                                candidate_status,
                                &near_key.identity,
                            ),
                            source_attribute_transitions: attribute_transitions,
                            source_status_present_only_co_transitions: present_only,
                            source_status_absent_only_co_transitions: absent_only,
                            transition_distance: distance,
                            outputs_equal,
                            present_outcome: present_outcome.clone(),
                            absent_outcome: absent_outcome.clone(),
                            present_sequences: present.sequences.iter().copied().collect(),
                            absent_sequences: absent.sequences.iter().copied().collect(),
                            present_formula_context: formula_context(cohort, present_sample),
                            absent_formula_context: formula_context(cohort, absent_sample),
                        });
                    }
                    if !saw_effect_absent_identity_state {
                        stats.present_groups_without_effect_absent_identity_state = stats
                            .present_groups_without_effect_absent_identity_state
                            .saturating_add(1);
                    }
                }
            }
        }
        enforce_memory_limit(cohort, "near-source counterfactual partition analysis")?;
    }
    Ok(results)
}

fn attribute_transitions(present: &[Attribute], absent: &[Attribute]) -> Vec<AttributeTransition> {
    let present = present
        .iter()
        .map(|attribute| (attribute.attribute_id, attribute.value))
        .collect::<BTreeMap<_, _>>();
    let absent = absent
        .iter()
        .map(|attribute| (attribute.attribute_id, attribute.value))
        .collect::<BTreeMap<_, _>>();
    present
        .keys()
        .chain(absent.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|attribute_id| {
            let present_value = present.get(&attribute_id).copied();
            let absent_value = absent.get(&attribute_id).copied();
            (present_value != absent_value).then_some(AttributeTransition {
                attribute_id,
                present_value,
                absent_value,
            })
        })
        .collect()
}

fn target_attribute_transitions(
    present: &[Attribute],
    absent: &[Attribute],
) -> Vec<AttributeTransition> {
    let present = present
        .iter()
        .filter(|attribute| attribute.attribute_id != CURRENT_HP_ATTRIBUTE_ID)
        .map(|attribute| (attribute.attribute_id, attribute.value))
        .collect::<BTreeMap<_, _>>();
    let absent = absent
        .iter()
        .filter(|attribute| attribute.attribute_id != CURRENT_HP_ATTRIBUTE_ID)
        .map(|attribute| (attribute.attribute_id, attribute.value))
        .collect::<BTreeMap<_, _>>();
    present
        .keys()
        .chain(absent.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|attribute_id| {
            let present_value = present.get(&attribute_id).copied();
            let absent_value = absent.get(&attribute_id).copied();
            (present_value != absent_value).then_some(AttributeTransition {
                attribute_id,
                present_value,
                absent_value,
            })
        })
        .collect()
}

fn status_co_transitions(
    present: &[Status],
    absent: &[Status],
    candidate: Status,
) -> (Vec<Status>, Vec<Status>) {
    let present = present.iter().copied().collect::<BTreeSet<_>>();
    let absent = absent.iter().copied().collect::<BTreeSet<_>>();
    let present_only = present
        .difference(&absent)
        .copied()
        .filter(|status| *status != candidate)
        .collect();
    let absent_only = absent.difference(&present).copied().collect();
    (present_only, absent_only)
}

fn build_near_target_reports(
    near: BTreeMap<VariantKey, NearTargetAccum>,
    example_limit: usize,
) -> Vec<NearTargetEffectReport> {
    let mut by_effect = BTreeMap::<i64, Vec<(Status, NearTargetAccum)>>::new();
    for (variant, mut stats) in near {
        stats.examples.sort_by(|left, right| {
            left.transition_distance
                .cmp(&right.transition_distance)
                .then_with(|| left.ability_id.cmp(&right.ability_id))
                .then_with(|| left.present_sequences.cmp(&right.present_sequences))
        });
        stats.examples.truncate(example_limit);
        by_effect
            .entry(variant.status.effect_id)
            .or_default()
            .push((variant.status, stats));
    }
    by_effect
        .into_iter()
        .map(|(effect_id, variants)| {
            let candidate_absent_near_pairs = variants
                .iter()
                .map(|(_, stats)| stats.candidate_absent_near_pairs)
                .sum();
            let divergent_output_pairs = variants
                .iter()
                .map(|(_, stats)| stats.divergent_output_pairs)
                .sum();
            let minimum_transition_distance = variants
                .iter()
                .filter_map(|(_, stats)| stats.minimum_transition_distance)
                .min();
            NearTargetEffectReport {
                locus: Locus::Target,
                effect_id,
                candidate_absent_near_pairs,
                divergent_output_pairs,
                minimum_transition_distance,
                variants: variants
                    .into_iter()
                    .map(|(status, stats)| NearTargetVariantReport {
                        status,
                        candidate_present_groups: stats.candidate_present_groups,
                        candidate_absent_near_pairs: stats.candidate_absent_near_pairs,
                        sample_comparisons: stats.sample_comparisons,
                        deterministic_pairs: stats.deterministic_pairs,
                        equal_output_pairs: stats.equal_output_pairs,
                        divergent_output_pairs: stats.divergent_output_pairs,
                        nondeterministic_pairs: stats.nondeterministic_pairs,
                        minimum_transition_distance: stats.minimum_transition_distance,
                        transition_distance_counts: stats
                            .transition_distance_counts
                            .into_iter()
                            .map(|(transition_distance, pairs)| NearDistanceCount {
                                transition_distance,
                                pairs,
                            })
                            .collect(),
                        examples: stats.examples,
                    })
                    .collect(),
                formula_authority: false,
                runtime_authority: false,
                provider_rdps_credit_allowed: false,
            }
        })
        .collect()
}

fn build_near_source_reports(
    values: BTreeMap<VariantKey, NearSourceAccum>,
    selected_source_attribute_ids: &BTreeSet<i32>,
    example_limit: usize,
) -> Vec<NearSourceEffectReport> {
    let mut by_effect = BTreeMap::<i64, Vec<(Status, NearSourceAccum)>>::new();
    for (variant, mut stats) in values {
        stats.examples.sort_by(|left, right| {
            right
                .outputs_equal
                .cmp(&left.outputs_equal)
                .then_with(|| left.transition_distance.cmp(&right.transition_distance))
                .then_with(|| left.ability_id.cmp(&right.ability_id))
                .then_with(|| left.present_sequences.cmp(&right.present_sequences))
        });
        stats.examples.truncate(example_limit);
        by_effect
            .entry(variant.status.effect_id)
            .or_default()
            .push((variant.status, stats));
    }
    by_effect
        .into_iter()
        .map(|(effect_id, variants)| {
            let candidate_absent_near_pairs = variants
                .iter()
                .map(|(_, stats)| stats.candidate_absent_near_pairs)
                .sum();
            let divergent_output_pairs = variants
                .iter()
                .map(|(_, stats)| stats.divergent_output_pairs)
                .sum();
            let minimum_transition_distance = variants
                .iter()
                .filter_map(|(_, stats)| stats.minimum_transition_distance)
                .min();
            NearSourceEffectReport {
                locus: Locus::Source,
                effect_id,
                selected_source_attribute_ids: selected_source_attribute_ids
                    .iter()
                    .copied()
                    .collect(),
                candidate_absent_near_pairs,
                divergent_output_pairs,
                minimum_transition_distance,
                variants: variants
                    .into_iter()
                    .map(|(status, stats)| NearSourceVariantReport {
                        status,
                        candidate_present_groups: stats.candidate_present_groups,
                        present_groups_without_effect_absent_identity_state: stats
                            .present_groups_without_effect_absent_identity_state,
                        effect_absent_identity_state_candidates: stats
                            .effect_absent_identity_state_candidates,
                        rejected_without_source_attribute_transition: stats
                            .rejected_without_source_attribute_transition,
                        rejected_with_unselected_source_attribute_transition: stats
                            .rejected_with_unselected_source_attribute_transition,
                        rejected_with_excess_source_status_co_transitions: stats
                            .rejected_with_excess_source_status_co_transitions,
                        rejected_source_attribute_transition_sets: stats
                            .rejected_source_attribute_transition_sets
                            .into_iter()
                            .map(
                                |(attribute_ids, candidates)| SourceAttributeTransitionSetCount {
                                    attribute_ids,
                                    candidates,
                                },
                            )
                            .collect(),
                        candidate_absent_near_pairs: stats.candidate_absent_near_pairs,
                        sample_comparisons: stats.sample_comparisons,
                        deterministic_pairs: stats.deterministic_pairs,
                        equal_output_pairs: stats.equal_output_pairs,
                        divergent_output_pairs: stats.divergent_output_pairs,
                        nondeterministic_pairs: stats.nondeterministic_pairs,
                        minimum_transition_distance: stats.minimum_transition_distance,
                        transition_distance_counts: stats
                            .transition_distance_counts
                            .into_iter()
                            .map(|(transition_distance, pairs)| NearDistanceCount {
                                transition_distance,
                                pairs,
                            })
                            .collect(),
                        examples: stats.examples,
                    })
                    .collect(),
                formula_authority: false,
                runtime_authority: false,
                provider_rdps_credit_allowed: false,
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn compare_locus(
    cohort: &PartitionedCohort,
    groups: &HashMap<ExactKey, Bucket>,
    present_key: &ExactKey,
    present: &Bucket,
    locus: Locus,
    removals: &[(Status, Option<u32>)],
    _mode: Mode,
    example_limit: usize,
    results: &mut BTreeMap<VariantKey, ModeAccum>,
) {
    for (status, absent_state_id) in removals {
        let variant = VariantKey {
            locus,
            status: *status,
        };
        let stats = results.entry(variant).or_default();
        let blade_sweep_candidate_applicable = cohort.game_build.as_deref() == Some("24687926")
            && locus == Locus::Target
            && status.effect_id == BLADE_SWEEP_EFFECT_ID;
        if blade_sweep_candidate_applicable {
            stats
                .blade_sweep_candidate_projection
                .get_or_insert_with(BladeSweepCandidateAccum::default);
        }
        stats.present_groups += 1;
        stats.present_samples += present.sample_count;
        let Some(absent_state_id) = absent_state_id else {
            stats.absent_status_state_unobserved_groups += 1;
            continue;
        };
        let mut absent_key = present_key.clone();
        match locus {
            Locus::Source => absent_key.source_status_state_id = *absent_state_id,
            Locus::Target => absent_key.target_status_state_id = *absent_state_id,
        }
        retain_referenced_provider_attribute_states(
            &mut absent_key.status_provider_attribute_states,
            &cohort.status_states[absent_key.source_status_state_id as usize],
            &cohort.status_states[absent_key.target_status_state_id as usize],
        );
        let Some(absent) = groups.get(&absent_key) else {
            stats.absent_identity_group_unobserved_groups += 1;
            continue;
        };
        stats.controlled_groups += 1;
        let comparisons = present.sample_count.saturating_mul(absent.sample_count);
        stats.sample_comparisons = stats.sample_comparisons.saturating_add(comparisons);
        if present.outcomes.len() != 1 || absent.outcomes.len() != 1 {
            stats.nondeterministic_groups += 1;
            if stats.nondeterministic_examples.len() < example_limit {
                let present_sample = present
                    .representative_sample
                    .as_ref()
                    .expect("every populated bucket has a representative sample");
                let absent_sample = absent
                    .representative_sample
                    .as_ref()
                    .expect("every populated bucket has a representative sample");
                stats
                    .nondeterministic_examples
                    .push(NondeterministicExample {
                        rlog: cohort.rlogs[present_sample.identity.rlog_id as usize].clone(),
                        session_id: cohort.sessions[present_sample.identity.session_id as usize]
                            .clone(),
                        run_ordinal: present_sample.identity.run_ordinal,
                        source_entity_uuid: present_sample.identity.source_entity_uuid,
                        target_entity_uuid: present_sample.identity.target_entity_uuid,
                        ability_id: present_sample.identity.ability_id,
                        status: *status,
                        locus,
                        provider_relationship: provider_relationship(
                            *status,
                            &present_sample.identity,
                        ),
                        present_sample_count: present.sample_count,
                        absent_sample_count: absent.sample_count,
                        present_unique_outcomes: present.outcomes.len(),
                        absent_unique_outcomes: absent.outcomes.len(),
                        present_outcomes: bounded_outcome_counts(present, example_limit),
                        absent_outcomes: bounded_outcome_counts(absent, example_limit),
                        present_sequences: present.sequences.iter().copied().collect(),
                        absent_sequences: absent.sequences.iter().copied().collect(),
                        present_formula_context: formula_context(cohort, present_sample),
                        absent_formula_context: formula_context(cohort, absent_sample),
                    });
            }
            continue;
        }
        stats.deterministic_groups += 1;
        let present_outcome = present.outcomes.keys().next().expect("one present outcome");
        let absent_outcome = absent.outcomes.keys().next().expect("one absent outcome");
        if present_outcome == absent_outcome {
            stats.equal_output_groups += 1;
            continue;
        }
        stats.divergent_output_groups += 1;
        let present_sample = present
            .representative_sample
            .as_ref()
            .expect("every populated bucket has a representative sample");
        let absent_sample = absent
            .representative_sample
            .as_ref()
            .expect("every populated bucket has a representative sample");
        if blade_sweep_candidate_applicable {
            evaluate_blade_sweep_candidate(
                cohort,
                present_outcome,
                absent_outcome,
                absent_sample,
                example_limit,
                stats
                    .blade_sweep_candidate_projection
                    .as_mut()
                    .expect("applicable candidate accumulator exists"),
            );
        }
        let provider_relationship = provider_relationship(*status, &present_sample.identity);
        let relationship = stats
            .divergent_provider_relationship_groups
            .entry(provider_relationship)
            .or_default();
        relationship.0 = relationship.0.saturating_add(1);
        relationship.1 = relationship.1.saturating_add(comparisons);
        increment(
            &mut stats.amount_differences,
            present_outcome.amount.saturating_sub(absent_outcome.amount),
            comparisons,
        );
        if let (Some(present_value), Some(absent_value)) =
            (present_outcome.normal_value, absent_outcome.normal_value)
        {
            increment(
                &mut stats.normal_value_differences,
                present_value.saturating_sub(absent_value),
                comparisons,
            );
        }
        if absent_outcome.amount != 0 {
            let ratio = i128::from(present_outcome.amount)
                .saturating_mul(10_000)
                .checked_div(i128::from(absent_outcome.amount))
                .and_then(|value| i64::try_from(value).ok());
            if let Some(ratio) = ratio {
                increment(&mut stats.amount_ratio_basis_points, ratio, comparisons);
            }
        }
        if stats.divergent_examples.len() < example_limit {
            stats.divergent_examples.push(Example {
                rlog: cohort.rlogs[present_sample.identity.rlog_id as usize].clone(),
                session_id: cohort.sessions[present_sample.identity.session_id as usize].clone(),
                run_ordinal: present_sample.identity.run_ordinal,
                source_entity_uuid: present_sample.identity.source_entity_uuid,
                target_entity_uuid: present_sample.identity.target_entity_uuid,
                ability_id: present_sample.identity.ability_id,
                status: *status,
                locus,
                provider_relationship,
                present_outcome: present_outcome.clone(),
                absent_outcome: absent_outcome.clone(),
                present_sequences: present.sequences.iter().copied().collect(),
                absent_sequences: absent.sequences.iter().copied().collect(),
                present_formula_context: formula_context(cohort, present_sample),
                absent_formula_context: formula_context(cohort, absent_sample),
            });
        }
    }
}

fn bounded_outcome_counts(bucket: &Bucket, limit: usize) -> Vec<OutcomeCount> {
    let mut outcomes = bucket
        .outcomes
        .iter()
        .map(|(outcome, samples)| OutcomeCount {
            outcome: outcome.clone(),
            samples: *samples,
        })
        .collect::<Vec<_>>();
    outcomes.sort_by(|left, right| {
        right
            .samples
            .cmp(&left.samples)
            .then_with(|| left.outcome.cmp(&right.outcome))
    });
    outcomes.truncate(limit);
    outcomes
}

fn evaluate_blade_sweep_candidate(
    cohort: &PartitionedCohort,
    present_outcome: &Outcome,
    absent_outcome: &Outcome,
    absent_sample: &PartitionSample,
    example_limit: usize,
    accum: &mut BladeSweepCandidateAccum,
) {
    accum.controlled_divergent_groups = accum.controlled_divergent_groups.saturating_add(1);
    let target_attributes =
        &cohort.attribute_states[absent_sample.target_attribute_state_id as usize];
    let Some(raw_defense) = target_attributes
        .iter()
        .find(|row| row.attribute_id == PHYSICAL_DEFENSE_ATTRIBUTE_ID)
        .map(|row| row.value)
    else {
        accum.groups_missing_target_physical_defense = accum
            .groups_missing_target_physical_defense
            .saturating_add(1);
        return;
    };
    if raw_defense < 0 || absent_outcome.amount < 0 || present_outcome.amount < 0 {
        accum.groups_with_invalid_nonnegative_inputs = accum
            .groups_with_invalid_nonnegative_inputs
            .saturating_add(1);
        return;
    }
    accum.groups_with_target_physical_defense =
        accum.groups_with_target_physical_defense.saturating_add(1);
    let defense = i128::from(raw_defense);
    let absent_damage = i128::from(absent_outcome.amount);
    let present_damage = i128::from(present_outcome.amount);
    let constant = i128::from(TARGET_DEFENSE_CURVE_CONSTANT);
    let denominator = constant + defense;
    let base_minimum = ceil_div_i128(absent_damage.saturating_mul(denominator), constant);
    let base_maximum = ceil_div_i128(
        absent_damage.saturating_add(1).saturating_mul(denominator),
        constant,
    )
    .saturating_sub(1);
    let scaled_defense = defense.saturating_mul(i128::from(
        BASIS_POINT_SCALE - BLADE_SWEEP_ARMOR_PENETRATION_BASIS_POINTS,
    ));
    let scale = i128::from(BASIS_POINT_SCALE);
    let mut example_variants = Vec::new();
    for rounding in candidate_roundings() {
        let effective_defense = match rounding {
            EffectiveDefenseRounding::Floor => scaled_defense / scale,
            EffectiveDefenseRounding::Ceil => ceil_div_i128(scaled_defense, scale),
            EffectiveDefenseRounding::RoundHalfUp => {
                scaled_defense.saturating_add(scale / 2) / scale
            }
        };
        let effective_denominator = constant + effective_defense;
        let predicted_minimum = base_minimum.saturating_mul(constant) / effective_denominator;
        let predicted_maximum = base_maximum.saturating_mul(constant) / effective_denominator;
        let compatible = present_damage >= predicted_minimum && present_damage <= predicted_maximum;
        let variant = accum.variants.entry(rounding).or_default();
        if compatible {
            variant.compatible_groups = variant.compatible_groups.saturating_add(1);
        } else {
            variant.rejected_groups = variant.rejected_groups.saturating_add(1);
        }
        example_variants.push(CandidateVariantExample {
            rounding,
            effective_target_physical_defense_raw: i64::try_from(effective_defense)
                .unwrap_or(i64::MAX),
            predicted_present_damage_minimum: predicted_minimum.to_string(),
            predicted_present_damage_maximum: predicted_maximum.to_string(),
            observed_present_damage_compatible: compatible,
        });
    }
    if accum.examples.len() < example_limit {
        accum.examples.push(BladeSweepCandidateExample {
            target_physical_defense_raw: raw_defense,
            absent_damage: absent_outcome.amount,
            present_damage: present_outcome.amount,
            compatible_base_minimum: base_minimum.to_string(),
            compatible_base_maximum: base_maximum.to_string(),
            variants: example_variants,
        });
    }
}

fn evaluate_all_element_damage_candidate(
    transitions: &[AttributeTransition],
    present_outcome: &Outcome,
    absent_outcome: &Outcome,
    example_limit: usize,
    accum: &mut AllElementDamageCandidateAccum,
) {
    accum.deterministic_pairs = accum.deterministic_pairs.saturating_add(1);
    if present_outcome != absent_outcome {
        accum.deterministic_divergent_pairs = accum.deterministic_divergent_pairs.saturating_add(1);
    }
    let Some(transition) = transitions
        .iter()
        .find(|row| row.attribute_id == ALL_ELEMENT_CURRENT_ATTRIBUTE_ID)
    else {
        accum.pairs_missing_current_attribute_transition = accum
            .pairs_missing_current_attribute_transition
            .saturating_add(1);
        return;
    };
    let (Some(present_bonus), Some(absent_bonus)) =
        (transition.present_value, transition.absent_value)
    else {
        accum.pairs_with_invalid_inputs = accum.pairs_with_invalid_inputs.saturating_add(1);
        return;
    };
    let scale = i128::from(BASIS_POINT_SCALE);
    let present_factor = scale + i128::from(present_bonus);
    let absent_factor = scale + i128::from(absent_bonus);
    if present_factor <= 0
        || absent_factor <= 0
        || !is_ordinary_unshielded_damage(present_outcome)
        || !is_ordinary_unshielded_damage(absent_outcome)
    {
        accum.pairs_with_invalid_inputs = accum.pairs_with_invalid_inputs.saturating_add(1);
        return;
    }
    accum.pairs_with_current_attribute_transition = accum
        .pairs_with_current_attribute_transition
        .saturating_add(1);

    let mut example_variants = Vec::new();
    for rounding in fixed_point_damage_roundings() {
        let absent_interval = fixed_point_preimage_interval(
            i128::from(absent_outcome.amount),
            absent_factor,
            scale,
            rounding,
        );
        let present_interval = fixed_point_preimage_interval(
            i128::from(present_outcome.amount),
            present_factor,
            scale,
            rounding,
        );
        let intersection_minimum = absent_interval.0.max(present_interval.0);
        let intersection_maximum = absent_interval.1.min(present_interval.1);
        let compatible = intersection_minimum <= intersection_maximum;
        let variant = accum.variants.entry(rounding).or_default();
        if compatible {
            variant.compatible_groups = variant.compatible_groups.saturating_add(1);
        } else {
            variant.rejected_groups = variant.rejected_groups.saturating_add(1);
        }
        example_variants.push(AllElementDamageCandidateVariantExample {
            rounding,
            absent_subtotal_minimum: absent_interval.0.to_string(),
            absent_subtotal_maximum: absent_interval.1.to_string(),
            present_subtotal_minimum: present_interval.0.to_string(),
            present_subtotal_maximum: present_interval.1.to_string(),
            compatible_subtotal_minimum: compatible.then(|| intersection_minimum.to_string()),
            compatible_subtotal_maximum: compatible.then(|| intersection_maximum.to_string()),
            compatible,
        });
    }
    if accum.examples.len() < example_limit {
        accum.examples.push(AllElementDamageCandidateExample {
            absent_bonus_basis_points: absent_bonus,
            present_bonus_basis_points: present_bonus,
            absent_damage: absent_outcome.amount,
            present_damage: present_outcome.amount,
            variants: example_variants,
        });
    }
}

fn is_ordinary_unshielded_damage(outcome: &Outcome) -> bool {
    outcome.amount >= 0
        && outcome.normal_value == Some(outcome.amount)
        && outcome.lucky_value.is_none_or(|value| value == 0)
        && outcome.shield_loss.is_none_or(|value| value == 0)
        && outcome.hp_loss == Some(outcome.amount)
        && outcome
            .actual_amount
            .is_none_or(|value| value == outcome.amount)
}

fn fixed_point_damage_roundings() -> [FixedPointDamageRounding; 2] {
    [
        FixedPointDamageRounding::Floor,
        FixedPointDamageRounding::NearestHalfUp,
    ]
}

fn fixed_point_preimage_interval(
    observed: i128,
    factor: i128,
    denominator: i128,
    rounding: FixedPointDamageRounding,
) -> (i128, i128) {
    debug_assert!(observed >= 0);
    debug_assert!(factor > 0);
    debug_assert!(denominator > 0);
    match rounding {
        FixedPointDamageRounding::Floor => (
            ceil_div_i128(observed * denominator, factor),
            ceil_div_i128((observed + 1) * denominator, factor) - 1,
        ),
        FixedPointDamageRounding::NearestHalfUp => {
            let doubled_factor = factor * 2;
            let lower_numerator = (observed * denominator * 2 - denominator).max(0);
            let upper_numerator = (observed + 1) * denominator * 2 - denominator;
            (
                ceil_div_i128(lower_numerator, doubled_factor),
                ceil_div_i128(upper_numerator, doubled_factor) - 1,
            )
        }
    }
}

fn finalize_all_element_damage_candidate(
    mut accum: AllElementDamageCandidateAccum,
) -> AllElementDamageCandidateProjection {
    AllElementDamageCandidateProjection {
        model_id: "effect-2110125-source-all-element-current-final-multiplier-candidate",
        effect_id: FATAL_SPIRAL_EFFECT_ID,
        current_attribute_id: ALL_ELEMENT_CURRENT_ATTRIBUTE_ID,
        fixed_point_denominator: BASIS_POINT_SCALE,
        deterministic_pairs: accum.deterministic_pairs,
        deterministic_divergent_pairs: accum.deterministic_divergent_pairs,
        pairs_with_current_attribute_transition: accum.pairs_with_current_attribute_transition,
        pairs_missing_current_attribute_transition: accum
            .pairs_missing_current_attribute_transition,
        pairs_with_invalid_inputs: accum.pairs_with_invalid_inputs,
        variants: fixed_point_damage_roundings()
            .into_iter()
            .map(|rounding| {
                let stats = accum.variants.remove(&rounding).unwrap_or_default();
                AllElementCandidateVariantStats {
                    rounding,
                    compatible_pairs: stats.compatible_groups,
                    rejected_pairs: stats.rejected_groups,
                }
            })
            .collect(),
        examples: accum.examples,
        candidate_selected: false,
        exact_damage_stage_binding_proven: false,
        exact_operation_order_proven: false,
        exact_integer_rounding_proven: false,
        conservation_proven: false,
        formula_authority: false,
        runtime_authority: false,
        ui_display_authority: false,
        provider_rdps_credit_allowed: false,
    }
}

fn candidate_roundings() -> [EffectiveDefenseRounding; 3] {
    [
        EffectiveDefenseRounding::Floor,
        EffectiveDefenseRounding::Ceil,
        EffectiveDefenseRounding::RoundHalfUp,
    ]
}

fn ceil_div_i128(numerator: i128, denominator: i128) -> i128 {
    numerator.saturating_add(denominator.saturating_sub(1)) / denominator
}

fn formula_context(cohort: &PartitionedCohort, sample: &PartitionSample) -> FormulaContext {
    FormulaContext {
        representative_sequence: sample.sequence,
        observed_micros: sample.observed_micros,
        wire_capture_sequence: sample.wire_capture_sequence,
        normalized_packet_input_sha256: packet_fingerprint_text(
            &sample.identity.packet_input_fingerprint,
        ),
        normalized_packet_inputs: sample.normalized_packet_inputs.clone(),
        source_attribute_state_id: sample.source_attribute_state_id,
        source_attributes: cohort.attribute_states[sample.source_attribute_state_id as usize]
            .clone(),
        direct_source_attribute_state_id: sample.direct_source_attribute_state_id,
        direct_source_attributes: sample
            .direct_source_attribute_state_id
            .map(|id| cohort.attribute_states[id as usize].clone())
            .unwrap_or_default(),
        target_attribute_state_id: sample.target_attribute_state_id,
        target_attributes: cohort.attribute_states[sample.target_attribute_state_id as usize]
            .clone(),
        source_status_state_id: sample.source_status_state_id,
        source_statuses: cohort.status_states[sample.source_status_state_id as usize].clone(),
        target_status_state_id: sample.target_status_state_id,
        target_statuses: cohort.status_states[sample.target_status_state_id as usize].clone(),
        status_provider_attributes: sample
            .status_provider_attribute_states
            .iter()
            .map(|provider| FormulaProviderContext {
                provider_entity_uuid: provider.provider_entity_uuid,
                attribute_state_observed: provider.attribute_state_id.is_some(),
                attribute_state_id: provider.attribute_state_id,
                attributes: provider
                    .attribute_state_id
                    .map(|id| cohort.attribute_states[id as usize].clone())
                    .unwrap_or_default(),
            })
            .collect(),
    }
}

fn packet_fingerprint_text(fingerprint: &[u8; 32]) -> String {
    let mut text = String::with_capacity(71);
    text.push_str("sha256:");
    for byte in fingerprint {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn provider_relationship(status: Status, identity: &Identity) -> ProviderRelationship {
    let Some(provider) = status.source_entity_uuid else {
        return ProviderRelationship::MissingProvider;
    };
    if provider == identity.source_entity_uuid {
        ProviderRelationship::CreditedDamageSource
    } else if identity.direct_source_entity_uuid == Some(provider) {
        ProviderRelationship::DirectDamageSource
    } else if provider == identity.target_entity_uuid {
        ProviderRelationship::DamageTarget
    } else {
        ProviderRelationship::ThirdParty
    }
}

fn outcome(sample: &Sample) -> Outcome {
    Outcome {
        normal_value: sample.normal_value,
        lucky_value: sample.lucky_value,
        amount: sample.amount,
        actual_amount: sample.actual_amount,
        hp_loss: sample.hp_loss,
        shield_loss: sample.shield_loss,
    }
}

fn increment(values: &mut BTreeMap<i64, u64>, value: i64, count: u64) {
    let entry = values.entry(value).or_default();
    *entry = entry.saturating_add(count);
}

fn build_reports(
    observations: BTreeMap<VariantKey, ObservationStats>,
    mut exact: BTreeMap<VariantKey, ModeAccum>,
    mut relaxed: BTreeMap<VariantKey, ModeAccum>,
    example_limit: usize,
    histogram_limit: usize,
) -> Vec<EffectReport> {
    let mut grouped = BTreeMap::<(Locus, i64), Vec<VariantReport>>::new();
    for (variant, observation) in observations {
        grouped
            .entry((variant.locus, variant.status.effect_id))
            .or_default()
            .push(VariantReport {
                status: variant.status,
                observation,
                exact_recorded_inputs: finalize_mode(
                    exact.remove(&variant).unwrap_or_default(),
                    example_limit,
                    histogram_limit,
                ),
                target_current_hp_excluded_diagnostic: finalize_mode(
                    relaxed.remove(&variant).unwrap_or_default(),
                    example_limit,
                    histogram_limit,
                ),
            });
    }
    grouped
        .into_iter()
        .map(|((locus, effect_id), mut variants)| {
            variants.sort_by(|left, right| {
                right
                    .observation
                    .observed_samples
                    .cmp(&left.observation.observed_samples)
                    .then_with(|| left.status.cmp(&right.status))
            });
            let mut observation = ObservationStats::default();
            let mut exact = ModeStats::default();
            let mut relaxed = ModeStats::default();
            for variant in &variants {
                observation.observed_status_states += variant.observation.observed_status_states;
                observation.observed_samples += variant.observation.observed_samples;
                merge_mode(&mut exact, &variant.exact_recorded_inputs, example_limit);
                merge_mode(
                    &mut relaxed,
                    &variant.target_current_hp_excluded_diagnostic,
                    example_limit,
                );
            }
            normalize_merged_mode(&mut exact, histogram_limit);
            normalize_merged_mode(&mut relaxed, histogram_limit);
            EffectReport {
                locus,
                effect_id,
                observation,
                exact_recorded_inputs: exact,
                target_current_hp_excluded_diagnostic: relaxed,
                variants,
            }
        })
        .collect()
}

fn finalize_mode(accum: ModeAccum, example_limit: usize, histogram_limit: usize) -> ModeStats {
    ModeStats {
        present_groups: accum.present_groups,
        present_samples: accum.present_samples,
        absent_status_state_unobserved_groups: accum.absent_status_state_unobserved_groups,
        absent_identity_group_unobserved_groups: accum.absent_identity_group_unobserved_groups,
        controlled_groups: accum.controlled_groups,
        sample_comparisons: accum.sample_comparisons,
        deterministic_groups: accum.deterministic_groups,
        equal_output_groups: accum.equal_output_groups,
        divergent_output_groups: accum.divergent_output_groups,
        nondeterministic_groups: accum.nondeterministic_groups,
        amount_differences: difference_counts(accum.amount_differences, histogram_limit),
        normal_value_differences: difference_counts(
            accum.normal_value_differences,
            histogram_limit,
        ),
        amount_ratio_basis_points: difference_counts(
            accum.amount_ratio_basis_points,
            histogram_limit,
        ),
        divergent_provider_relationship_groups: provider_relationship_counts(
            accum.divergent_provider_relationship_groups,
        ),
        nondeterministic_examples: accum
            .nondeterministic_examples
            .into_iter()
            .take(example_limit)
            .collect(),
        divergent_examples: accum
            .divergent_examples
            .into_iter()
            .take(example_limit)
            .collect(),
        blade_sweep_candidate_projection: accum
            .blade_sweep_candidate_projection
            .map(|value| finalize_blade_sweep_candidate(value, example_limit)),
    }
}

fn finalize_blade_sweep_candidate(
    mut accum: BladeSweepCandidateAccum,
    example_limit: usize,
) -> BladeSweepCandidateProjection {
    BladeSweepCandidateProjection {
        model_id: "effect-2110092-pre-mitigation-650bp-defense-reduction-candidate",
        effect_id: BLADE_SWEEP_EFFECT_ID,
        armor_penetration_basis_points: BLADE_SWEEP_ARMOR_PENETRATION_BASIS_POINTS,
        defense_curve_constant: TARGET_DEFENSE_CURVE_CONSTANT,
        controlled_divergent_groups: accum.controlled_divergent_groups,
        groups_with_target_physical_defense: accum.groups_with_target_physical_defense,
        groups_missing_target_physical_defense: accum.groups_missing_target_physical_defense,
        groups_with_invalid_nonnegative_inputs: accum.groups_with_invalid_nonnegative_inputs,
        variants: candidate_roundings()
            .into_iter()
            .map(|rounding| {
                let stats = accum.variants.remove(&rounding).unwrap_or_default();
                CandidateVariantStats {
                    rounding,
                    compatible_groups: stats.compatible_groups,
                    rejected_groups: stats.rejected_groups,
                }
            })
            .collect(),
        examples: accum.examples.into_iter().take(example_limit).collect(),
        candidate_selected: false,
        exact_damage_projection_proven: false,
        exact_operation_order_proven: false,
        exact_integer_rounding_proven: false,
        formula_authority: false,
        runtime_authority: false,
        ui_display_authority: false,
        provider_rdps_credit_allowed: false,
    }
}

fn provider_relationship_counts(
    values: BTreeMap<ProviderRelationship, (u64, u64)>,
) -> Vec<ProviderRelationshipCount> {
    values
        .into_iter()
        .map(
            |(relationship, (groups, sample_comparisons))| ProviderRelationshipCount {
                relationship,
                groups,
                sample_comparisons,
            },
        )
        .collect()
}

fn difference_counts(values: BTreeMap<i64, u64>, limit: usize) -> Vec<DifferenceCount> {
    let mut values = values
        .into_iter()
        .map(|(value, comparisons)| DifferenceCount { value, comparisons })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .comparisons
            .cmp(&left.comparisons)
            .then_with(|| left.value.abs().cmp(&right.value.abs()))
            .then_with(|| left.value.cmp(&right.value))
    });
    values.truncate(limit);
    values
}

fn merge_mode(target: &mut ModeStats, source: &ModeStats, example_limit: usize) {
    target.present_groups += source.present_groups;
    target.present_samples += source.present_samples;
    target.absent_status_state_unobserved_groups += source.absent_status_state_unobserved_groups;
    target.absent_identity_group_unobserved_groups +=
        source.absent_identity_group_unobserved_groups;
    target.controlled_groups += source.controlled_groups;
    target.sample_comparisons += source.sample_comparisons;
    target.deterministic_groups += source.deterministic_groups;
    target.equal_output_groups += source.equal_output_groups;
    target.divergent_output_groups += source.divergent_output_groups;
    target.nondeterministic_groups += source.nondeterministic_groups;
    merge_difference_counts(&mut target.amount_differences, &source.amount_differences);
    merge_difference_counts(
        &mut target.normal_value_differences,
        &source.normal_value_differences,
    );
    merge_difference_counts(
        &mut target.amount_ratio_basis_points,
        &source.amount_ratio_basis_points,
    );
    merge_provider_relationship_counts(
        &mut target.divergent_provider_relationship_groups,
        &source.divergent_provider_relationship_groups,
    );
    let remaining = example_limit.saturating_sub(target.nondeterministic_examples.len());
    target.nondeterministic_examples.extend(
        source
            .nondeterministic_examples
            .iter()
            .take(remaining)
            .cloned(),
    );
    let remaining = example_limit.saturating_sub(target.divergent_examples.len());
    target
        .divergent_examples
        .extend(source.divergent_examples.iter().take(remaining).cloned());
    merge_blade_sweep_candidate(
        &mut target.blade_sweep_candidate_projection,
        source.blade_sweep_candidate_projection.as_ref(),
        example_limit,
    );
}

fn merge_blade_sweep_candidate(
    target: &mut Option<BladeSweepCandidateProjection>,
    source: Option<&BladeSweepCandidateProjection>,
    example_limit: usize,
) {
    let Some(source) = source else {
        return;
    };
    let target = target.get_or_insert_with(|| BladeSweepCandidateProjection {
        model_id: source.model_id,
        effect_id: source.effect_id,
        armor_penetration_basis_points: source.armor_penetration_basis_points,
        defense_curve_constant: source.defense_curve_constant,
        controlled_divergent_groups: 0,
        groups_with_target_physical_defense: 0,
        groups_missing_target_physical_defense: 0,
        groups_with_invalid_nonnegative_inputs: 0,
        variants: candidate_roundings()
            .into_iter()
            .map(|rounding| CandidateVariantStats {
                rounding,
                compatible_groups: 0,
                rejected_groups: 0,
            })
            .collect(),
        examples: Vec::new(),
        candidate_selected: false,
        exact_damage_projection_proven: false,
        exact_operation_order_proven: false,
        exact_integer_rounding_proven: false,
        formula_authority: false,
        runtime_authority: false,
        ui_display_authority: false,
        provider_rdps_credit_allowed: false,
    });
    target.controlled_divergent_groups = target
        .controlled_divergent_groups
        .saturating_add(source.controlled_divergent_groups);
    target.groups_with_target_physical_defense = target
        .groups_with_target_physical_defense
        .saturating_add(source.groups_with_target_physical_defense);
    target.groups_missing_target_physical_defense = target
        .groups_missing_target_physical_defense
        .saturating_add(source.groups_missing_target_physical_defense);
    target.groups_with_invalid_nonnegative_inputs = target
        .groups_with_invalid_nonnegative_inputs
        .saturating_add(source.groups_with_invalid_nonnegative_inputs);
    for source_variant in &source.variants {
        let target_variant = target
            .variants
            .iter_mut()
            .find(|row| row.rounding == source_variant.rounding)
            .expect("every candidate rounding is initialized");
        target_variant.compatible_groups = target_variant
            .compatible_groups
            .saturating_add(source_variant.compatible_groups);
        target_variant.rejected_groups = target_variant
            .rejected_groups
            .saturating_add(source_variant.rejected_groups);
    }
    let remaining = example_limit.saturating_sub(target.examples.len());
    target
        .examples
        .extend(source.examples.iter().take(remaining).cloned());
}

fn merge_provider_relationship_counts(
    target: &mut Vec<ProviderRelationshipCount>,
    source: &[ProviderRelationshipCount],
) {
    let mut values = target
        .drain(..)
        .map(|row| (row.relationship, (row.groups, row.sample_comparisons)))
        .collect::<BTreeMap<_, _>>();
    for row in source {
        let entry = values.entry(row.relationship).or_default();
        entry.0 = entry.0.saturating_add(row.groups);
        entry.1 = entry.1.saturating_add(row.sample_comparisons);
    }
    *target = provider_relationship_counts(values);
}

fn merge_difference_counts(target: &mut Vec<DifferenceCount>, source: &[DifferenceCount]) {
    let mut values = target
        .drain(..)
        .map(|row| (row.value, row.comparisons))
        .collect::<BTreeMap<_, _>>();
    for row in source {
        let entry = values.entry(row.value).or_default();
        *entry = entry.saturating_add(row.comparisons);
    }
    *target = values
        .into_iter()
        .map(|(value, comparisons)| DifferenceCount { value, comparisons })
        .collect();
}

fn normalize_merged_mode(mode: &mut ModeStats, histogram_limit: usize) {
    normalize_difference_counts(&mut mode.amount_differences, histogram_limit);
    normalize_difference_counts(&mut mode.normal_value_differences, histogram_limit);
    normalize_difference_counts(&mut mode.amount_ratio_basis_points, histogram_limit);
}

fn normalize_difference_counts(values: &mut Vec<DifferenceCount>, limit: usize) {
    values.sort_by(|left, right| {
        right
            .comparisons
            .cmp(&left.comparisons)
            .then_with(|| left.value.abs().cmp(&right.value.abs()))
            .then_with(|| left.value.cmp(&right.value))
    });
    values.truncate(limit);
}

fn parse_args() -> Result<Arguments, String> {
    let mut cohort = None;
    let mut baseline_proof = None;
    let mut output = None;
    let mut effect_ids = BTreeSet::new();
    let mut source_character_ids = BTreeSet::new();
    let mut source_entity_uuids = BTreeSet::new();
    let mut example_limit = 8usize;
    let mut histogram_limit = 32usize;
    let mut memory_limit_mib = 512usize;
    let mut source_transition_attribute_ids = BTreeSet::new();
    let mut cross_entity_formula_state_diagnostic = false;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--cohort" => cohort = Some(PathBuf::from(value)),
            "--baseline-proof" => baseline_proof = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--effect" => {
                effect_ids.insert(value.parse().map_err(|_| {
                    "invalid --effect; expected an exact numeric effect ID".to_owned()
                })?);
            }
            "--source-character-id" => {
                let value = value.trim();
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(
                        "invalid --source-character-id; expected an exact numeric stable character ID"
                            .to_owned(),
                    );
                }
                source_character_ids.insert(value.to_owned());
            }
            "--source-entity-uuid" => {
                let value = value.parse().map_err(|_| {
                    "invalid --source-entity-uuid; expected an exact positive numeric entity UUID"
                        .to_owned()
                })?;
                if value <= 0 {
                    return Err(
                        "invalid --source-entity-uuid; expected an exact positive numeric entity UUID"
                            .to_owned(),
                    );
                }
                source_entity_uuids.insert(value);
            }
            "--example-limit" => {
                example_limit = value
                    .parse()
                    .map_err(|_| "invalid --example-limit".to_owned())?
            }
            "--histogram-limit" => {
                histogram_limit = value
                    .parse()
                    .map_err(|_| "invalid --histogram-limit".to_owned())?
            }
            "--memory-limit-mib" => {
                memory_limit_mib = value
                    .parse()
                    .map_err(|_| "invalid --memory-limit-mib".to_owned())?;
                if memory_limit_mib == 0 {
                    return Err("--memory-limit-mib must be greater than zero".to_owned());
                }
            }
            "--source-transition-attribute" => {
                source_transition_attribute_ids.insert(value.parse().map_err(|_| {
                    "invalid --source-transition-attribute; expected an exact numeric attribute ID"
                        .to_owned()
                })?);
            }
            "--cross-entity-formula-state-diagnostic" => {
                cross_entity_formula_state_diagnostic = value.parse().map_err(|_| {
                    "invalid --cross-entity-formula-state-diagnostic; expected true or false"
                        .to_owned()
                })?;
            }
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    Ok(Arguments {
        cohort: cohort.ok_or_else(|| "missing --cohort".to_owned())?,
        baseline_proof,
        output: output.ok_or_else(|| "missing --output".to_owned())?,
        effect_ids,
        source_character_ids,
        source_entity_uuids,
        example_limit,
        histogram_limit,
        memory_limit_mib,
        source_transition_attribute_ids,
        cross_entity_formula_state_diagnostic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_cohort_schema_requires_provider_and_direct_source_context_generations() {
        assert!(!formula_cohort_schema_is_supported(None));
        assert!(!formula_cohort_schema_is_supported(Some(39)));
        assert!(formula_cohort_schema_is_supported(Some(40)));
        assert!(formula_cohort_schema_is_supported(Some(41)));
        assert!(formula_cohort_schema_is_supported(Some(42)));
        assert!(formula_cohort_schema_is_supported(Some(43)));
        assert!(formula_cohort_schema_is_supported(Some(44)));
        assert!(formula_cohort_schema_is_supported(Some(45)));
        assert!(formula_cohort_schema_is_supported(Some(46)));
        assert!(formula_cohort_schema_is_supported(Some(47)));
        assert!(formula_cohort_schema_is_supported(Some(48)));
        assert!(!formula_cohort_schema_is_supported(Some(49)));
    }

    #[test]
    fn source_character_filter_is_exact_and_fail_closed_for_missing_identity() {
        let selected = BTreeSet::from(["3296036".to_owned()]);
        let matching = ActorIdentity {
            entity_type_id: 1,
            monster_id: None,
            character_id: Some("3296036".to_owned()),
            class_id: Some(3),
            specialization_id: Some(1),
            level: Some(60),
        };
        let other = ActorIdentity {
            character_id: Some("2474661".to_owned()),
            ..matching.clone()
        };

        assert!(source_character_matches(Some(&matching), &selected));
        assert!(!source_character_matches(Some(&other), &selected));
        assert!(!source_character_matches(None, &selected));
        assert!(source_character_matches(None, &BTreeSet::new()));
    }

    #[test]
    fn configured_memory_limit_fails_closed_on_a_current_working_set_overage() {
        assert!(enforce_observed_working_set_limit(512, 512, "fixture").is_ok());
        let error = enforce_observed_working_set_limit(513, 512, "fixture")
            .expect_err("current working-set overage must fail closed");
        assert!(error.contains("fixture"));
        assert!(error.contains("513"));
        assert!(error.contains("512"));
    }

    #[test]
    fn streaming_loader_partitions_a_small_fixture_without_materializing_samples() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rlogs-counterfactual-streaming-fixture-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        let cohort_path = root.join("cohort.json");
        let output_path = root.join("proof.json");
        let packet =
            serde_json::to_value(DamagePacketDetail::default()).expect("serialize default packet");
        let sample = |sequence: u64, amount: i64| {
            serde_json::json!({
                "rlog": "fixture.rlog",
                "session_id": "fixture-session",
                "run_ordinal": 1,
                "sequence": sequence,
                "observed_micros": sequence * 10,
                "wire_capture_sequence": sequence,
                "scene_id": 12023,
                "source_entity_uuid": 10,
                "direct_source_entity_uuid": null,
                "target_entity_uuid": 20,
                "source_actor_identity": {
                    "entity_type_id": 1,
                    "monster_id": null,
                    "character_id": "source-character",
                    "class_id": 4,
                    "specialization_id": 2,
                    "level": 80
                },
                "direct_source_actor_identity": null,
                "target_actor_identity": {
                    "entity_type_id": 2,
                    "monster_id": 9001,
                    "character_id": null,
                    "class_id": null,
                    "specialization_id": null,
                    "level": 80
                },
                "ability_id": 30,
                "passive_uuid": null,
                "hit_event_id": 1,
                "amount": amount,
                "actual_amount": null,
                "normal_value": amount,
                "lucky_value": null,
                "hp_loss": amount,
                "shield_loss": null,
                "damage_source": 2,
                "damage_type": 1,
                "critical": false,
                "lucky": false,
                "packet": packet.clone(),
                "source_attribute_state_id": 0,
                "target_attribute_state_id": 0,
                "source_status_state_id": 0,
                "target_status_state_id": 0,
                "status_provider_attribute_states": []
            })
        };
        let document = serde_json::json!({
            "schema_version": 43,
            "game_build": "24687926",
            "inputs": ["fixture.rlog"],
            "attribute_states": [[]],
            "status_states": [[]],
            "samples": [sample(1, 100), sample(2, 101)]
        });
        fs::write(
            &cohort_path,
            serde_json::to_vec(&document).expect("serialize fixture"),
        )
        .expect("write fixture");
        let arguments = Arguments {
            cohort: cohort_path,
            baseline_proof: None,
            output: output_path,
            effect_ids: BTreeSet::new(),
            source_character_ids: BTreeSet::new(),
            source_entity_uuids: BTreeSet::new(),
            example_limit: 4,
            histogram_limit: 4,
            memory_limit_mib: 4_096,
            source_transition_attribute_ids: BTreeSet::new(),
            cross_entity_formula_state_diagnostic: true,
        };
        let partitioned = load_partitioned_cohort(&arguments).expect("stream fixture");
        assert_eq!(partitioned.scanned_sample_count, 2);
        assert_eq!(partitioned.sample_count, 2);
        assert_eq!(partitioned.partition_paths.len(), MIN_PARTITIONS);
        assert_eq!(
            partitioned.cross_entity_partition_paths.len(),
            MIN_PARTITIONS
        );
        assert_eq!(partitioned.rlogs, ["fixture.rlog"]);
        assert_eq!(partitioned.sessions, ["fixture-session"]);
        assert!(partitioned.largest_partition_bytes > 0);
        let retained = partitioned
            .partition_paths
            .iter()
            .find_map(|path| {
                let line = fs::read_to_string(path).ok()?;
                (!line.is_empty()).then(|| {
                    serde_json::from_str::<PartitionSample>(line.lines().next().unwrap())
                        .expect("decode retained partition sample")
                })
            })
            .expect("non-empty partition");
        assert_eq!(retained.identity.scene_id, Some(12_023));
        assert_eq!(
            retained
                .identity
                .source_actor_identity
                .as_ref()
                .and_then(|identity| identity.character_id.as_deref()),
            Some("source-character")
        );
        validate_state_references(&partitioned).expect("valid partition references");
        let work_dir = partitioned.work_dir.clone();
        drop(partitioned);
        assert!(!work_dir.exists());
        fs::remove_dir_all(root).expect("remove fixture root");
    }

    fn status(effect_id: i64) -> Status {
        Status {
            effect_id,
            source_entity_uuid: Some(7),
            stacks: Some(1),
            level: Some(2),
            origin_source_type_id: Some(3),
            origin_source_config_id: Some(4),
        }
    }

    #[test]
    fn removal_points_to_the_exact_observed_absent_state() {
        let candidate = status(21);
        let retained = status(22);
        let states = vec![vec![retained], vec![candidate, retained]];
        let removals = status_removals(&states, &BTreeSet::new());
        assert_eq!(removals[1], vec![(candidate, Some(0)), (retained, None)]);
    }

    #[test]
    fn exact_effect_filter_keeps_only_the_requested_numeric_id() {
        let candidate = status(21);
        let retained = status(22);
        let states = vec![vec![retained], vec![candidate, retained]];
        let removals = status_removals(&states, &BTreeSet::from([21]));
        assert!(removals[0].is_empty());
        assert_eq!(removals[1], vec![(candidate, Some(0))]);
    }

    #[test]
    fn current_hp_normalization_preserves_every_other_attribute() {
        let states = vec![
            vec![
                Attribute {
                    attribute_id: CURRENT_HP_ATTRIBUTE_ID,
                    value: 100,
                },
                Attribute {
                    attribute_id: 99,
                    value: 5,
                },
            ],
            vec![
                Attribute {
                    attribute_id: CURRENT_HP_ATTRIBUTE_ID,
                    value: 90,
                },
                Attribute {
                    attribute_id: 99,
                    value: 5,
                },
            ],
            vec![Attribute {
                attribute_id: 99,
                value: 6,
            }],
        ];
        let ids = current_hp_excluded_attribute_ids(&states);
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
    }

    #[test]
    fn provider_state_key_drops_only_the_removed_status_exclusive_provider() {
        let candidate = Status {
            source_entity_uuid: Some(7),
            ..status(21)
        };
        let retained_same_provider = Status {
            source_entity_uuid: Some(8),
            ..status(22)
        };
        let retained_target_provider = Status {
            source_entity_uuid: Some(20),
            ..status(23)
        };
        let mut providers = vec![
            ProviderAttributeStateRef {
                provider_entity_uuid: 7,
                attribute_state_id: Some(1),
            },
            ProviderAttributeStateRef {
                provider_entity_uuid: 8,
                attribute_state_id: Some(2),
            },
            ProviderAttributeStateRef {
                provider_entity_uuid: 20,
                attribute_state_id: Some(3),
            },
        ];

        retain_referenced_provider_attribute_states(
            &mut providers,
            &[retained_same_provider],
            &[retained_target_provider],
        );

        assert_eq!(
            providers,
            vec![
                ProviderAttributeStateRef {
                    provider_entity_uuid: 8,
                    attribute_state_id: Some(2),
                },
                ProviderAttributeStateRef {
                    provider_entity_uuid: 20,
                    attribute_state_id: Some(3),
                },
            ]
        );
        assert_eq!(candidate.source_entity_uuid, Some(7));
    }

    #[test]
    fn provider_state_key_normalizes_target_current_hp_state_with_target_key() {
        let sample = PartitionSample {
            identity: Identity {
                rlog_id: 0,
                session_id: 0,
                run_ordinal: 1,
                scene_id: Some(1),
                source_entity_uuid: 10,
                direct_source_entity_uuid: None,
                target_entity_uuid: 20,
                source_actor_identity: None,
                direct_source_actor_identity: None,
                target_actor_identity: None,
                ability_id: 30,
                passive_uuid: None,
                hit_event_id: Some(1),
                damage_source: Some(2),
                damage_type: Some(1),
                critical: Some(false),
                lucky: Some(false),
                packet_input_fingerprint: [0; 32],
            },
            sequence: 1,
            observed_micros: 1,
            wire_capture_sequence: Some(1),
            normalized_packet_inputs: DamagePacketDetail::default(),
            outcome: Outcome {
                normal_value: Some(100),
                lucky_value: None,
                amount: 100,
                actual_amount: None,
                hp_loss: Some(100),
                shield_loss: None,
            },
            source_attribute_state_id: 0,
            direct_source_attribute_state_id: None,
            target_attribute_state_id: 3,
            source_status_state_id: 0,
            target_status_state_id: 0,
            status_provider_attribute_states: vec![
                ProviderAttributeStateRef {
                    provider_entity_uuid: 10,
                    attribute_state_id: Some(2),
                },
                ProviderAttributeStateRef {
                    provider_entity_uuid: 20,
                    attribute_state_id: Some(3),
                },
            ],
        };
        let normalized = normalized_provider_attribute_states(&sample, &[0, 1, 2, 9]);

        assert_eq!(normalized[0].attribute_state_id, Some(2));
        assert_eq!(normalized[1].attribute_state_id, Some(9));
    }

    #[test]
    fn normalized_packet_context_preserves_formula_inputs_and_removes_outputs() {
        let packet = DamagePacketDetail {
            dead: Some(true),
            normal_value: Some(100),
            lucky_value: Some(200),
            owner_level: Some(30),
            owner_stage: Some(2),
            property: Some(7),
            skill_effect_uuid: Some(300),
            skill_effect_total_damage: Some(400),
            skill_effect_group_index: Some(5),
            skill_effect_component_index: Some(2),
            skill_effect_component_count: Some(7),
            ..DamagePacketDetail::default()
        };
        let normalized = normalized_packet_inputs(&packet);
        assert_eq!(normalized.owner_level, Some(30));
        assert_eq!(normalized.owner_stage, Some(2));
        assert_eq!(normalized.property, Some(7));
        assert_eq!(normalized.skill_effect_group_index, None);
        assert_eq!(normalized.skill_effect_component_index, None);
        assert_eq!(normalized.skill_effect_component_count, None);
        assert_eq!(normalized.dead, None);
        assert_eq!(normalized.normal_value, None);
        assert_eq!(normalized.lucky_value, None);
        assert_eq!(normalized.skill_effect_uuid, None);
        assert_eq!(normalized.skill_effect_total_damage, None);

        let mut other_outcome = packet.clone();
        other_outcome.dead = Some(false);
        other_outcome.normal_value = Some(999);
        other_outcome.skill_effect_total_damage = Some(888);
        assert_eq!(
            packet_input_fingerprint(&packet).unwrap(),
            packet_input_fingerprint(&other_outcome).unwrap()
        );
    }

    #[test]
    fn comparison_partition_ignores_both_attribute_and_status_states() {
        let identity = Identity {
            rlog_id: 1,
            session_id: 2,
            run_ordinal: 3,
            scene_id: Some(30),
            source_entity_uuid: 4,
            direct_source_entity_uuid: Some(5),
            target_entity_uuid: 6,
            source_actor_identity: None,
            direct_source_actor_identity: None,
            target_actor_identity: None,
            ability_id: 7,
            passive_uuid: Some(8),
            hit_event_id: Some(9),
            damage_source: Some(10),
            damage_type: Some(11),
            critical: Some(false),
            lucky: Some(true),
            packet_input_fingerprint: [12; 32],
        };
        let present = PartitionSample {
            identity,
            sequence: 14,
            observed_micros: 15,
            wire_capture_sequence: Some(16),
            normalized_packet_inputs: DamagePacketDetail::default(),
            outcome: Outcome {
                normal_value: None,
                lucky_value: None,
                amount: 17,
                actual_amount: None,
                hp_loss: Some(17),
                shield_loss: None,
            },
            source_attribute_state_id: 18,
            direct_source_attribute_state_id: None,
            target_attribute_state_id: 19,
            source_status_state_id: 20,
            target_status_state_id: 21,
            status_provider_attribute_states: vec![ProviderAttributeStateRef {
                provider_entity_uuid: 25,
                attribute_state_id: Some(18),
            }],
        };
        let mut absent = present.clone();
        absent.source_attribute_state_id = 27;
        absent.target_attribute_state_id = 22;
        absent.source_status_state_id = 23;
        absent.target_status_state_id = 24;
        absent.status_provider_attribute_states[0].attribute_state_id = Some(26);
        assert_eq!(partition_index(&present, 64), partition_index(&absent, 64));
        assert!(!has_required_direct_source_attribute_state(&present));

        let exact_key = |sample: &PartitionSample| ExactKey {
            identity: sample.identity.clone(),
            source_attribute_state_id: sample.source_attribute_state_id,
            direct_source_attribute_state_id: sample.direct_source_attribute_state_id,
            target_attribute_state_id: sample.target_attribute_state_id,
            source_status_state_id: sample.source_status_state_id,
            target_status_state_id: sample.target_status_state_id,
            status_provider_attribute_states: sample.status_provider_attribute_states.clone(),
        };
        assert_ne!(exact_key(&present), exact_key(&absent));

        let mut direct_source_transition = present.clone();
        direct_source_transition.direct_source_attribute_state_id = Some(28);
        assert!(has_required_direct_source_attribute_state(
            &direct_source_transition
        ));
        assert_ne!(exact_key(&present), exact_key(&direct_source_transition));
    }

    #[test]
    fn near_target_attribute_transitions_exclude_only_current_hp() {
        let transitions = target_attribute_transitions(
            &[
                Attribute {
                    attribute_id: CURRENT_HP_ATTRIBUTE_ID,
                    value: 100,
                },
                Attribute {
                    attribute_id: PHYSICAL_DEFENSE_ATTRIBUTE_ID,
                    value: 500,
                },
            ],
            &[
                Attribute {
                    attribute_id: CURRENT_HP_ATTRIBUTE_ID,
                    value: 90,
                },
                Attribute {
                    attribute_id: PHYSICAL_DEFENSE_ATTRIBUTE_ID,
                    value: 450,
                },
            ],
        );
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].attribute_id, PHYSICAL_DEFENSE_ATTRIBUTE_ID);
        assert_eq!(transitions[0].present_value, Some(500));
        assert_eq!(transitions[0].absent_value, Some(450));
    }

    #[test]
    fn source_attribute_transitions_preserve_current_hp_and_exact_numeric_ids() {
        let transitions = attribute_transitions(
            &[
                Attribute {
                    attribute_id: CURRENT_HP_ATTRIBUTE_ID,
                    value: 100,
                },
                Attribute {
                    attribute_id: 11_030,
                    value: 12_000,
                },
            ],
            &[
                Attribute {
                    attribute_id: CURRENT_HP_ATTRIBUTE_ID,
                    value: 90,
                },
                Attribute {
                    attribute_id: 11_030,
                    value: 9_000,
                },
            ],
        );
        assert_eq!(transitions.len(), 2);
        assert_eq!(transitions[0].attribute_id, 11_030);
        assert_eq!(transitions[1].attribute_id, CURRENT_HP_ATTRIBUTE_ID);
    }

    #[test]
    fn near_target_status_transitions_preserve_every_co_transition() {
        let candidate = status(21);
        let retained = status(22);
        let co_present = status(23);
        let co_absent = status(24);
        let (present_only, absent_only) = status_co_transitions(
            &[candidate, retained, co_present],
            &[retained, co_absent],
            candidate,
        );
        assert_eq!(present_only, [co_present]);
        assert_eq!(absent_only, [co_absent]);
    }

    #[test]
    fn cross_entity_formula_state_diagnostic_pairs_only_exact_observed_state() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rlogs-cross-entity-counterfactual-fixture-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        let partition_path = root.join("cross.ndjson");
        let actor = |character_id: &str| ActorIdentity {
            entity_type_id: 1,
            monster_id: None,
            character_id: Some(character_id.to_owned()),
            class_id: Some(4),
            specialization_id: Some(2),
            level: Some(80),
        };
        let identity = |rlog_id, session_id, source_entity_uuid, character_id: &str| Identity {
            rlog_id,
            session_id,
            run_ordinal: rlog_id + 1,
            scene_id: Some(12_000 + i32::try_from(rlog_id).unwrap()),
            source_entity_uuid,
            direct_source_entity_uuid: None,
            target_entity_uuid: 1_000 + source_entity_uuid,
            source_actor_identity: Some(actor(character_id)),
            direct_source_actor_identity: None,
            target_actor_identity: None,
            ability_id: 30,
            passive_uuid: None,
            hit_event_id: Some(1),
            damage_source: Some(2),
            damage_type: Some(1),
            critical: Some(false),
            lucky: Some(false),
            packet_input_fingerprint: [9; 32],
        };
        let sample = |identity: Identity,
                      sequence,
                      amount,
                      source_attribute_state_id,
                      source_status_state_id,
                      status_provider_attribute_states| PartitionSample {
            identity,
            sequence,
            observed_micros: sequence * 10,
            wire_capture_sequence: Some(sequence),
            normalized_packet_inputs: DamagePacketDetail::default(),
            outcome: Outcome {
                normal_value: Some(amount),
                lucky_value: None,
                amount,
                actual_amount: None,
                hp_loss: Some(amount),
                shield_loss: None,
            },
            source_attribute_state_id,
            direct_source_attribute_state_id: None,
            target_attribute_state_id: 0,
            source_status_state_id,
            target_status_state_id: 0,
            status_provider_attribute_states,
        };
        let present = sample(
            identity(0, 0, 10, "character-a"),
            1,
            110,
            0,
            1,
            vec![ProviderAttributeStateRef {
                provider_entity_uuid: 10,
                attribute_state_id: Some(0),
            }],
        );
        let absent = sample(identity(1, 1, 20, "character-b"), 2, 100, 0, 0, Vec::new());
        assert_eq!(
            cross_entity_packet_identity(&present.identity),
            cross_entity_packet_identity(&absent.identity),
            "capture, scene, entity UUID, and character ID are provenance rather than formula inputs"
        );
        let mut transition_present_identity = identity(2, 2, 30, "character-c");
        transition_present_identity.ability_id = 31;
        transition_present_identity.packet_input_fingerprint = [8; 32];
        let mut transition_present = sample(
            transition_present_identity,
            3,
            120,
            1,
            2,
            vec![
                ProviderAttributeStateRef {
                    provider_entity_uuid: 30,
                    attribute_state_id: Some(1),
                },
                ProviderAttributeStateRef {
                    provider_entity_uuid: 1_030,
                    attribute_state_id: Some(0),
                },
            ],
        );
        transition_present.target_status_state_id = 3;
        let mut transition_absent_identity = identity(3, 3, 40, "character-d");
        transition_absent_identity.ability_id = 31;
        transition_absent_identity.packet_input_fingerprint = [8; 32];
        let mut transition_absent = sample(
            transition_absent_identity,
            4,
            100,
            0,
            4,
            vec![ProviderAttributeStateRef {
                provider_entity_uuid: 40,
                attribute_state_id: Some(0),
            }],
        );
        transition_absent.target_status_state_id = 0;
        let mut review_present_identity = identity(4, 4, 50, "character-e");
        review_present_identity.ability_id = 32;
        review_present_identity.packet_input_fingerprint = [7; 32];
        let review_present = sample(
            review_present_identity,
            5,
            130,
            1,
            5,
            vec![ProviderAttributeStateRef {
                provider_entity_uuid: 50,
                attribute_state_id: Some(1),
            }],
        );
        let mut review_absent_identity = identity(5, 5, 60, "character-f");
        review_absent_identity.ability_id = 32;
        review_absent_identity.packet_input_fingerprint = [7; 32];
        let review_absent = sample(review_absent_identity, 6, 100, 0, 0, Vec::new());
        let mut encoded = serde_json::to_vec(&present).expect("serialize present sample");
        encoded.push(b'\n');
        encoded.extend(serde_json::to_vec(&absent).expect("serialize absent sample"));
        encoded.push(b'\n');
        encoded.extend(
            serde_json::to_vec(&transition_present).expect("serialize transition present sample"),
        );
        encoded.push(b'\n');
        encoded.extend(
            serde_json::to_vec(&transition_absent).expect("serialize transition absent sample"),
        );
        encoded.push(b'\n');
        encoded
            .extend(serde_json::to_vec(&review_present).expect("serialize review present sample"));
        encoded.push(b'\n');
        encoded.extend(serde_json::to_vec(&review_absent).expect("serialize review absent sample"));
        encoded.push(b'\n');
        fs::write(&partition_path, encoded).expect("write cross partition");
        let candidate = Status {
            effect_id: 21,
            source_entity_uuid: Some(10),
            stacks: Some(1),
            level: Some(2),
            origin_source_type_id: Some(3),
            origin_source_config_id: Some(4),
        };
        let transition_candidate = Status {
            source_entity_uuid: Some(30),
            ..candidate
        };
        let target_co_status = Status {
            effect_id: 99,
            source_entity_uuid: Some(1_030),
            stacks: Some(1),
            level: Some(1),
            origin_source_type_id: Some(8),
            origin_source_config_id: Some(9),
        };
        let source_co_present_status = Status {
            effect_id: 97,
            source_entity_uuid: Some(30),
            stacks: Some(1),
            level: Some(1),
            origin_source_type_id: Some(10),
            origin_source_config_id: Some(11),
        };
        let source_co_absent_status = Status {
            effect_id: 98,
            source_entity_uuid: Some(40),
            ..source_co_present_status
        };
        let review_candidate = Status {
            source_entity_uuid: Some(50),
            ..candidate
        };
        let review_co_statuses = (101..=105)
            .map(|effect_id| Status {
                effect_id,
                source_entity_uuid: Some(50),
                stacks: Some(1),
                level: Some(1),
                origin_source_type_id: Some(12),
                origin_source_config_id: Some(effect_id),
            })
            .collect::<Vec<_>>();
        let mut review_status_state = vec![review_candidate];
        review_status_state.extend(review_co_statuses);
        let cohort = PartitionedCohort {
            game_build: Some("24687926".to_owned()),
            source_inputs: vec![
                "a.rlog".to_owned(),
                "b.rlog".to_owned(),
                "c.rlog".to_owned(),
                "d.rlog".to_owned(),
            ],
            input_bytes: 0,
            input_sha256: String::new(),
            attribute_states: vec![
                vec![Attribute {
                    attribute_id: 11_030,
                    value: 10_000,
                }],
                vec![Attribute {
                    attribute_id: 11_030,
                    value: 11_000,
                }],
            ],
            status_states: vec![
                Vec::new(),
                vec![candidate],
                vec![transition_candidate, source_co_present_status],
                vec![target_co_status],
                vec![source_co_absent_status],
                review_status_state,
            ],
            source_status_usage: vec![3, 1, 1, 0, 1, 1],
            target_status_usage: vec![5, 0, 0, 1, 0, 0],
            rlogs: vec![
                "a.rlog".to_owned(),
                "b.rlog".to_owned(),
                "c.rlog".to_owned(),
                "d.rlog".to_owned(),
                "e.rlog".to_owned(),
                "f.rlog".to_owned(),
            ],
            sessions: vec![
                "a".to_owned(),
                "b".to_owned(),
                "c".to_owned(),
                "d".to_owned(),
                "e".to_owned(),
                "f".to_owned(),
            ],
            scanned_sample_count: 6,
            sample_count: 6,
            partition_paths: Vec::new(),
            cross_entity_partition_paths: vec![partition_path],
            work_dir: root.clone(),
            largest_partition_bytes: 0,
            largest_cross_entity_partition_bytes: 1,
            configured_memory_limit_bytes: u64::MAX,
        };
        let reports = build_cross_entity_reports(
            analyze_cross_entity_formula_state_mode(&cohort, &BTreeSet::from([21]), 4)
                .expect("analyze fixture"),
        );
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].controlled_groups, 1);
        assert_eq!(reports[0].divergent_output_groups, 1);
        let exact_variant = reports[0]
            .variants
            .iter()
            .find(|variant| variant.controlled_groups == 1)
            .expect("one exact controlled variant");
        assert_eq!(exact_variant.sample_comparisons, 1);
        assert_eq!(
            exact_variant.status.provider_relationship,
            ProviderRelationship::CreditedDamageSource
        );
        assert!(exact_variant.status.provider_attribute_state_observed);
        assert!(!reports[0].formula_authority);
        let transition_reports = build_cross_entity_source_transition_reports(
            analyze_cross_entity_source_transition_mode(
                &cohort,
                &BTreeSet::from([21]),
                &BTreeSet::from([11_030]),
                &[0, 1],
                false,
                false,
                4,
            )
            .expect("analyze transition fixture"),
            &BTreeSet::from([11_030]),
        );
        assert_eq!(transition_reports.len(), 1);
        assert_eq!(transition_reports[0].controlled_pairs, 0);
        let target_status_transition_reports = build_cross_entity_source_transition_reports(
            analyze_cross_entity_source_transition_mode(
                &cohort,
                &BTreeSet::from([21]),
                &BTreeSet::from([11_030]),
                &[0, 1],
                false,
                true,
                4,
            )
            .expect("analyze target-status transition fixture"),
            &BTreeSet::from([11_030]),
        );
        assert_eq!(target_status_transition_reports[0].controlled_pairs, 0);
        let source_and_target_status_transition_reports =
            build_cross_entity_source_transition_reports(
                analyze_cross_entity_source_transition_mode(
                    &cohort,
                    &BTreeSet::from([21]),
                    &BTreeSet::from([11_030]),
                    &[0, 1],
                    true,
                    true,
                    4,
                )
                .expect("analyze source-and-target-status transition fixture"),
                &BTreeSet::from([11_030]),
            );
        assert_eq!(
            source_and_target_status_transition_reports[0].controlled_pairs,
            1
        );
        assert_eq!(
            source_and_target_status_transition_reports[0].divergent_output_pairs,
            1
        );
        let controlled_variant = source_and_target_status_transition_reports[0]
            .variants
            .iter()
            .find(|variant| variant.controlled_pairs == 1)
            .expect("one controlled transition variant");
        assert_eq!(
            controlled_variant.examples[0].source_attribute_transitions[0].attribute_id,
            11_030
        );
        assert_eq!(
            controlled_variant.examples[0].target_status_transition_distance,
            1
        );
        assert_eq!(
            controlled_variant.examples[0].source_status_transition_distance,
            2
        );
        assert_eq!(
            controlled_variant.source_status_transition_distance_counts[0].transition_distance,
            2
        );
        assert_eq!(
            controlled_variant.source_status_transition_distance_counts[0].pairs,
            1
        );
        assert_eq!(
            controlled_variant.source_status_transition_review_band_pairs,
            1
        );
        assert_eq!(
            controlled_variant
                .source_status_transition_review_band_pairs_without_source_attribute_transition,
            0
        );
        assert_eq!(
            controlled_variant
                .source_status_transition_review_band_pairs_with_unselected_source_attribute_transition,
            0
        );
        assert_eq!(
            controlled_variant
                .source_status_transition_review_band_pairs_with_selected_source_attribute_transition,
            1
        );
        assert_eq!(
            controlled_variant.source_status_transition_review_band_pairs_with_selected_source_attribute_and_target_status_transition_within_limit,
            1
        );
        assert!(!source_and_target_status_transition_reports[0].formula_authority);
        drop(cohort);
        assert!(!root.exists());
    }

    #[test]
    fn blade_sweep_candidate_rejects_incompatible_effective_defense_rounding() {
        let cohort = PartitionedCohort {
            game_build: Some("24687926".to_owned()),
            source_inputs: Vec::new(),
            input_bytes: 0,
            input_sha256: String::new(),
            attribute_states: vec![vec![Attribute {
                attribute_id: PHYSICAL_DEFENSE_ATTRIBUTE_ID,
                value: 5907,
            }]],
            status_states: Vec::new(),
            source_status_usage: Vec::new(),
            target_status_usage: Vec::new(),
            rlogs: Vec::new(),
            sessions: Vec::new(),
            scanned_sample_count: 0,
            sample_count: 0,
            partition_paths: Vec::new(),
            cross_entity_partition_paths: Vec::new(),
            work_dir: std::env::temp_dir().join(format!(
                "rlogs-counterfactual-candidate-test-nonexistent-{}",
                std::process::id()
            )),
            largest_partition_bytes: 0,
            largest_cross_entity_partition_bytes: 0,
            configured_memory_limit_bytes: u64::MAX,
        };
        let absent_sample = PartitionSample {
            identity: Identity {
                rlog_id: 0,
                session_id: 0,
                run_ordinal: 1,
                scene_id: None,
                source_entity_uuid: 2,
                direct_source_entity_uuid: None,
                target_entity_uuid: 3,
                source_actor_identity: None,
                direct_source_actor_identity: None,
                target_actor_identity: None,
                ability_id: 4,
                passive_uuid: None,
                hit_event_id: Some(1),
                damage_source: None,
                damage_type: None,
                critical: None,
                lucky: None,
                packet_input_fingerprint: [0; 32],
            },
            sequence: 1,
            observed_micros: 2,
            wire_capture_sequence: Some(3),
            normalized_packet_inputs: DamagePacketDetail::default(),
            outcome: Outcome {
                normal_value: Some(84356),
                lucky_value: None,
                amount: 84356,
                actual_amount: None,
                hp_loss: Some(84356),
                shield_loss: None,
            },
            source_attribute_state_id: 0,
            direct_source_attribute_state_id: None,
            target_attribute_state_id: 0,
            source_status_state_id: 0,
            target_status_state_id: 0,
            status_provider_attribute_states: Vec::new(),
        };
        let present_outcome = Outcome {
            amount: 85533,
            normal_value: Some(85533),
            ..absent_sample.outcome.clone()
        };
        let mut accum = BladeSweepCandidateAccum::default();
        evaluate_blade_sweep_candidate(
            &cohort,
            &present_outcome,
            &absent_sample.outcome,
            &absent_sample,
            8,
            &mut accum,
        );
        assert_eq!(accum.controlled_divergent_groups, 1);
        assert_eq!(accum.groups_with_target_physical_defense, 1);
        assert_eq!(accum.examples[0].compatible_base_minimum, "107006");
        assert_eq!(accum.examples[0].compatible_base_maximum, "107006");
        assert_eq!(
            accum
                .variants
                .get(&EffectiveDefenseRounding::Floor)
                .unwrap()
                .compatible_groups,
            1
        );
        assert_eq!(
            accum
                .variants
                .get(&EffectiveDefenseRounding::Ceil)
                .unwrap()
                .rejected_groups,
            1
        );
        assert_eq!(
            accum
                .variants
                .get(&EffectiveDefenseRounding::RoundHalfUp)
                .unwrap()
                .compatible_groups,
            1
        );
    }

    #[test]
    fn all_element_fixed_point_preimages_recover_shared_subtotal() {
        let absent_factor = i128::from(BASIS_POINT_SCALE + 316);
        let present_factor = i128::from(BASIS_POINT_SCALE + 1_316);
        for rounding in fixed_point_damage_roundings() {
            let absent = fixed_point_preimage_interval(
                103_160,
                absent_factor,
                i128::from(BASIS_POINT_SCALE),
                rounding,
            );
            let present = fixed_point_preimage_interval(
                113_160,
                present_factor,
                i128::from(BASIS_POINT_SCALE),
                rounding,
            );
            assert!(absent.0 <= 100_000 && 100_000 <= absent.1);
            assert!(present.0 <= 100_000 && 100_000 <= present.1);
            assert!(absent.0.max(present.0) <= absent.1.min(present.1));
        }
    }

    #[test]
    fn all_element_candidate_records_compatible_and_rejected_pairs() {
        let transitions = vec![AttributeTransition {
            attribute_id: ALL_ELEMENT_CURRENT_ATTRIBUTE_ID,
            present_value: Some(1_316),
            absent_value: Some(316),
        }];
        let outcome = |amount| Outcome {
            normal_value: Some(amount),
            lucky_value: None,
            amount,
            actual_amount: Some(amount),
            hp_loss: Some(amount),
            shield_loss: None,
        };
        let mut compatible = AllElementDamageCandidateAccum::default();
        evaluate_all_element_damage_candidate(
            &transitions,
            &outcome(113_160),
            &outcome(103_160),
            4,
            &mut compatible,
        );
        assert_eq!(compatible.deterministic_pairs, 1);
        assert_eq!(compatible.deterministic_divergent_pairs, 1);
        assert_eq!(compatible.pairs_with_current_attribute_transition, 1);
        for rounding in fixed_point_damage_roundings() {
            assert_eq!(
                compatible
                    .variants
                    .get(&rounding)
                    .unwrap()
                    .compatible_groups,
                1
            );
        }

        let mut rejected = AllElementDamageCandidateAccum::default();
        evaluate_all_element_damage_candidate(
            &transitions,
            &outcome(1),
            &outcome(103_160),
            4,
            &mut rejected,
        );
        for rounding in fixed_point_damage_roundings() {
            assert_eq!(rejected.variants.get(&rounding).unwrap().rejected_groups, 1);
        }
    }

    #[test]
    fn all_element_candidate_keeps_missing_attribute_transition_fail_closed() {
        let outcome = Outcome {
            normal_value: Some(100),
            lucky_value: None,
            amount: 100,
            actual_amount: Some(100),
            hp_loss: Some(100),
            shield_loss: None,
        };
        let mut accum = AllElementDamageCandidateAccum::default();
        evaluate_all_element_damage_candidate(&[], &outcome, &outcome, 4, &mut accum);
        assert_eq!(accum.deterministic_pairs, 1);
        assert_eq!(accum.pairs_missing_current_attribute_transition, 1);
        assert!(accum.variants.is_empty());
    }

    #[test]
    fn all_element_candidate_rejects_non_hp_or_clamped_damage() {
        let transitions = vec![AttributeTransition {
            attribute_id: ALL_ELEMENT_CURRENT_ATTRIBUTE_ID,
            present_value: Some(1_316),
            absent_value: Some(316),
        }];
        let outcome = Outcome {
            normal_value: Some(100),
            lucky_value: None,
            amount: 100,
            actual_amount: Some(99),
            hp_loss: Some(99),
            shield_loss: None,
        };
        let mut accum = AllElementDamageCandidateAccum::default();
        evaluate_all_element_damage_candidate(&transitions, &outcome, &outcome, 4, &mut accum);
        assert_eq!(accum.pairs_with_invalid_inputs, 1);
        assert!(accum.variants.is_empty());
    }

    #[test]
    fn exact_source_entity_filter_does_not_require_remote_character_identity() {
        let selected = BTreeSet::from([162_179_383_936]);
        assert!(source_entity_matches(162_179_383_936, &selected));
        assert!(!source_entity_matches(216_009_015_936, &selected));
        assert!(source_entity_matches(216_009_015_936, &BTreeSet::new()));
    }
}
