use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorKind, CanonicalEvent, EntityAttribute, EntityAttributeUpdateKind, EntityAttributeValue,
    EvidenceSource, RunState, StatusOrigin, StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::{
    BPSR_FIXED_POINT_SCALE, CriticalDamageFactorInterpretation,
    exact_external_critical_chance_fraction, exact_external_lucky_chance_fraction,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 19;
const MIN_STATUS_ATTRIBUTE_PROOF_SCHEMA_VERSION: u16 = 28;
const MAX_STATUS_ATTRIBUTE_PROOF_SCHEMA_VERSION: u16 = 29;
const INSPIRATION_EFFECT_ID: i64 = 2_202_041;
const INSPIRATION_PARENT_EFFECT_ID: i64 = 2_202_040;
const INSPIRATION_ORIGIN_SOURCE_TYPE_ID: i32 = 1;
const CRITICAL_CHANCE_ATTRIBUTE_ID: i32 = 11_710;
const CRITICAL_CHANCE_ADD_ATTRIBUTE_ID: i32 = 11_712;
const LUCKY_CHANCE_ATTRIBUTE_ID: i32 = 11_780;
const LUCKY_CHANCE_ADD_ATTRIBUTE_ID: i32 = 11_782;
const CRITICAL_DAMAGE_ATTRIBUTE_ID: i32 = 12_510;
const LUCKY_DAMAGE_ATTRIBUTE_ID: i32 = 12_530;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WireKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WindowKey {
    session_ordinal: u32,
    run_ordinal: u32,
    target_entity_uuid: i64,
    provider_entity_uuid: i64,
    instance_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MagnitudeKey {
    session_ordinal: u32,
    run_ordinal: u32,
    target_entity_uuid: i64,
    provider_entity_uuid: i64,
    instance_id: i64,
    level: i32,
    origin_source_type_id: i32,
    origin_source_config_id: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ChanceState {
    critical_chance_raw: Option<ObservedAttribute>,
    lucky_chance_raw: Option<ObservedAttribute>,
    critical_damage_raw: Option<ObservedAttribute>,
    lucky_damage_raw: Option<ObservedAttribute>,
}

#[derive(Debug, Clone, Copy)]
struct ObservedAttribute {
    value: i64,
    sequence: u64,
    observed_micros: u64,
    wire: Option<WireKey>,
}

#[derive(Debug, Clone)]
struct ActiveWindow {
    provider_actor_id: u64,
    level: Option<i32>,
    origin: Option<StatusOrigin>,
    applied_observed_micros: u64,
    expires_at_observed_micros: Option<u64>,
}

#[derive(Debug, Clone)]
struct WindowSnapshot {
    key: WindowKey,
    provider_actor_id: u64,
    level: Option<i32>,
    origin: Option<StatusOrigin>,
    applied_observed_micros: u64,
}

#[derive(Debug, Clone)]
struct CandidateHit {
    session_ordinal: u32,
    session_id: String,
    protocol_pack_digest: String,
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    source_actor_id: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    amount: i64,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    critical: bool,
    lucky: bool,
    damage_wire: Option<WireKey>,
    chance_state: ChanceState,
    active_windows: Vec<WindowSnapshot>,
}

#[derive(Debug, Default)]
struct MagnitudeAccumulator {
    critical_raw_deltas: BTreeSet<i64>,
    lucky_raw_deltas: BTreeSet<i64>,
}

#[derive(Debug, Clone, Copy)]
struct ProvenMagnitude {
    critical_raw_delta: i64,
    lucky_raw_delta: i64,
}

#[derive(Debug, Deserialize)]
struct StatusAttributeProof {
    schema_version: u16,
    expected_deployment_id: Option<String>,
    expected_game_build: Option<String>,
    selected_effect_ids: Vec<i64>,
    selected_attribute_ids: Vec<i32>,
    sessions: Vec<ProofSession>,
    wire_additive_equation_systems: Vec<WireEquationSystem>,
    #[serde(default)]
    reversible_static_coefficient_proofs: Vec<ReversibleStaticCoefficientProof>,
    #[serde(default)]
    matched_lifecycle_coefficient_proofs: Vec<MatchedLifecycleCoefficientProof>,
}

#[derive(Debug, Deserialize)]
struct ProofSession {
    rlog: String,
    session_id: String,
    bytes: u64,
    sha256: String,
    deployment_id: String,
    game_build: String,
    protocol_pack_digest: String,
}

#[derive(Debug, Deserialize)]
struct WireEquationSystem {
    attribute_id: i32,
    equations: Vec<WireEquation>,
}

#[derive(Debug, Deserialize)]
struct WireEquation {
    terms: Vec<WireTerm>,
    raw_attribute_delta: i64,
    examples: Vec<WireEquationExample>,
}

#[derive(Debug, Deserialize)]
struct WireTerm {
    effect_id: i64,
    origin: Option<ProofOrigin>,
    level: Option<i32>,
    signed_presence_delta: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct ProofOrigin {
    source_type_id: i32,
    source_config_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ProofFingerprint {
    effect_id: i64,
    origin: Option<ProofOrigin>,
    level: Option<i32>,
    part_id: Option<i32>,
    stacks: Option<u32>,
    count: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ReversibleStaticCoefficientProof {
    attribute_id: i32,
    fingerprint: ProofFingerprint,
    status: String,
    proven_coefficient_units: Option<i64>,
    normalized_coefficient_counts: BTreeMap<i64, u64>,
    apply_occurrences: u64,
    remove_occurrences: u64,
    independent_run_contexts: usize,
    cross_actor_occurrences: u64,
}

#[derive(Debug, Deserialize)]
struct MatchedLifecycleCoefficientProof {
    attribute_id: i32,
    fingerprint: ProofFingerprint,
    status: String,
    proven_coefficient_units: Option<i64>,
    exact_coefficient_counts: BTreeMap<i64, u64>,
    exact_pair_count: u64,
    contradictory_pair_count: u64,
    ambiguous_instance_count: u64,
    application_only_instance_count: u64,
    removal_only_instance_count: u64,
    independent_run_contexts: usize,
    cross_actor_exact_pairs: u64,
    #[serde(default)]
    examples: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct WireEquationExample {
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    status_instances: Vec<WireStatusInstance>,
}

#[derive(Debug, Deserialize)]
struct WireStatusInstance {
    effect_id: i64,
    instance_id: Option<i64>,
    state: String,
    source_entity_uuid: Option<i64>,
}

#[derive(Debug, Serialize)]
struct InputDescriptor {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    deployment_id: String,
    game_build: String,
    protocol_pack_digests: Vec<String>,
    effect_id: i64,
    transition_proof: InputDescriptor,
    transition_proof_schema_version: u16,
    damage_formula_surface: InputDescriptor,
    damage_formula_surface_schema_version: u16,
    critical_factor_proof: InputDescriptor,
    critical_factor_proof_schema_version: u16,
    rlogs: Vec<InputDescriptor>,
    counts: AuditCounts,
    exact_removal_magnitudes: Vec<MagnitudeReport>,
    level_lifecycle_evidence: Vec<LevelLifecycleEvidence>,
    formula_input_snapshot_coverage: FormulaInputSnapshotCoverage,
    integer_stage_counterfactual_coverage: IntegerStageCounterfactualCoverage,
    combined_packet_evidence: CombinedPacketEvidence,
    contribution_buckets: Vec<ContributionBucket>,
    conservation: ConservationReport,
    examples: Vec<ContributionExample>,
}

#[derive(Debug, Serialize)]
struct LevelLifecycleEvidence {
    level: i32,
    exact_instance_raw_delta: i64,
    attributes: Vec<AttributeLifecycleEvidence>,
    reversible_static_transform_proven: bool,
    blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AttributeLifecycleEvidence {
    attribute_id: i32,
    matching_reversible_rows: usize,
    reversible_statuses: Vec<String>,
    reversible_proven_coefficient_units: Option<i64>,
    normalized_coefficient_counts: BTreeMap<i64, u64>,
    apply_occurrences: u64,
    remove_occurrences: u64,
    independent_run_contexts: usize,
    cross_actor_occurrences: u64,
    matching_lifecycle_rows: usize,
    matched_statuses: Vec<String>,
    matched_proven_coefficient_units: Option<i64>,
    exact_coefficient_counts: BTreeMap<i64, u64>,
    exact_pair_count: u64,
    contradictory_pair_count: u64,
    ambiguous_instance_count: u64,
    application_only_instance_count: u64,
    removal_only_instance_count: u64,
    matched_independent_run_contexts: usize,
    cross_actor_exact_pairs: u64,
    matched_examples: Vec<serde_json::Value>,
    coefficient_consistent_with_instance_magnitude: bool,
    reversible_static_gate_passed: bool,
    matched_lifecycle_gate_passed: bool,
}

#[derive(Debug, Default, Serialize)]
struct CombinedPacketEvidence {
    candidates: u64,
    candidates_with_any_exact_external_player_window_magnitude: u64,
    normal_value_only: u64,
    lucky_value_only: u64,
    both_values: u64,
    neither_value: u64,
    amount_matches_normal_value: u64,
    amount_matches_lucky_value: u64,
    reported_critical_true: u64,
    reported_critical_false: u64,
    reported_critical_absent: u64,
    packet_shapes: Vec<CombinedPacketShape>,
    examples: Vec<CombinedPacketExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CombinedPacketShapeKey {
    value_source: &'static str,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
}

#[derive(Debug, Serialize)]
struct CombinedPacketShape {
    value_source: &'static str,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    events: u64,
}

#[derive(Debug, Serialize)]
struct CombinedPacketExample {
    session_id: String,
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    amount: i64,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    current_critical_chance_raw: Option<i64>,
    current_lucky_chance_raw: Option<i64>,
    critical_damage_raw: Option<i64>,
    exact_external_player_window_magnitudes: Vec<CombinedWindowMagnitude>,
}

#[derive(Debug, Serialize)]
struct CombinedWindowMagnitude {
    provider_entity_uuid: i64,
    instance_id: i64,
    level: i32,
    origin_source_type_id: i32,
    origin_source_config_id: i64,
    critical_raw_delta: i64,
    lucky_raw_delta: i64,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_use: &'static str,
    occurrence_authority: &'static str,
    magnitude_authority: &'static str,
    critical_path: &'static str,
    lucky_path: &'static str,
    combined_path: &'static str,
    buffering_bound: &'static str,
    remote_player_packets_required: bool,
    remote_player_packets_treated_as_zero: bool,
    remote_player_packets_synthesized: bool,
    unobserved_effect_levels_interpolated: bool,
    unobserved_or_mismatched_parent_origins_attributed: bool,
    historical_or_current_snapshot_substitution_allowed: bool,
    last_observed_local_attribute_is_event_time_formula_authority: bool,
    snapshot_age_threshold_is_formula_authority: bool,
    formula_input_snapshot_authority: bool,
    integer_stage_candidate_family_authority: bool,
    integer_stage_counterfactual_authority: bool,
    damage_formula_surface_runtime_authority: bool,
    ambiguous_damage_surface_rows_inferred: bool,
    owner_stage_array_selection_is_formula_authority: bool,
    missing_hit_event_id_may_be_synthesized: bool,
    unique_ability_damage_surface_candidate_authority: bool,
    damage_script_preimage_breakdown_authority: bool,
    damage_surface_identity_groups_include_exact_stage_inputs: bool,
    damage_surface_identity_groups_include_exact_stage_input_freshness: bool,
    stage_input_freshness_breakdown_authority: bool,
    critical_damage_raw_interpretation_authority: bool,
    partial_effect_credit_may_be_displayed_as_complete_effect_rdps: bool,
    formula_authority: bool,
    full_effect_composition_authority: bool,
    canonical_conservation_replay_authority: bool,
    provider_rdps_credit_authorized: bool,
    runtime_promotion_allowed: bool,
    ui_display_allowed: bool,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct FormulaInputSnapshotCoverage {
    scope: &'static str,
    exact_single_provider_candidate_events: u64,
    paths: Vec<FormulaPathSnapshotCoverage>,
    attributes: Vec<AttributeSnapshotCoverage>,
    oldest_observed_examples: Vec<FormulaInputSnapshotExample>,
    missing_examples: Vec<MissingFormulaInputExample>,
    event_time_snapshot_authority: bool,
}

#[derive(Debug, Serialize)]
struct FormulaPathSnapshotCoverage {
    path: &'static str,
    candidate_events: u64,
    complete_input_sets: u64,
    all_inputs_wire_provenance: u64,
    all_inputs_observed_not_after_damage: u64,
    maximum_oldest_input_age_sequences: Option<u64>,
    maximum_oldest_input_age_micros: Option<u64>,
}

#[derive(Debug, Serialize)]
struct AttributeSnapshotCoverage {
    attribute_id: i32,
    required_events: u64,
    present_events: u64,
    missing_events: u64,
    wire_provenance_events: u64,
    non_wire_or_unknown_provenance_events: u64,
    observed_not_after_damage_events: u64,
    observed_after_damage_events: u64,
    same_wire_as_damage_events: u64,
    maximum_age_sequences: Option<u64>,
    maximum_age_micros: Option<u64>,
}

#[derive(Debug, Serialize)]
struct FormulaInputSnapshotExample {
    path: &'static str,
    session_id: String,
    run_ordinal: u32,
    damage_sequence: u64,
    damage_observed_micros: u64,
    attribute_id: i32,
    value: i64,
    attribute_sequence: u64,
    attribute_observed_micros: u64,
    age_sequences: u64,
    age_micros: u64,
    wire_provenance: bool,
    same_wire_as_damage: bool,
}

#[derive(Debug, Serialize)]
struct MissingFormulaInputExample {
    path: &'static str,
    session_id: String,
    run_ordinal: u32,
    damage_sequence: u64,
    attribute_id: i32,
}

#[derive(Debug, Serialize)]
struct IntegerStageCounterfactualCoverage {
    scope: &'static str,
    candidate_family: Vec<&'static str>,
    exact_single_provider_candidate_events: u64,
    lucky_only_events_without_critical_stage: u64,
    critical_stage_events: u64,
    events_with_complete_stage_inputs: u64,
    events_without_complete_stage_inputs: u64,
    events_with_at_least_one_compatible_candidate: u64,
    events_without_compatible_candidates: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
    paths: Vec<IntegerStagePathCoverage>,
    exact_examples: Vec<IntegerStageCounterfactualExample>,
    unresolved_examples: Vec<IntegerStageCounterfactualExample>,
    critical_factor_event_records: Vec<CriticalFactorEventRecord>,
    damage_surface_join: IntegerStageDamageSurfaceJoin,
    critical_factor_interpretation_breakdown: Vec<CriticalFactorInterpretationBreakdown>,
    critical_factor_interpretation_breakdown_authority: bool,
    candidate_family_authority: bool,
    counterfactual_authority: bool,
}

#[derive(Debug, Default, Serialize)]
struct IntegerStagePathCoverage {
    path: &'static str,
    events: u64,
    complete_stage_inputs: u64,
    compatible_candidate_events: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
}

#[derive(Debug, Serialize)]
struct IntegerStageCounterfactualExample {
    path: &'static str,
    session_id: String,
    run_ordinal: u32,
    damage_sequence: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    observed_damage: i64,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    critical_damage_raw: Option<i64>,
    lucky_damage_raw: Option<i64>,
    candidates: Vec<IntegerStageCandidate>,
    exact_noncritical_counterfactual: Option<i64>,
    exact_critical_bonus: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct IntegerStageCandidate {
    formula: &'static str,
    critical_factor_interpretation: &'static str,
    first_rounding: &'static str,
    second_rounding: Option<&'static str>,
    evaluation_status: &'static str,
    compatible_with_observed_damage: bool,
    latent_base_min: Option<i64>,
    latent_base_max: Option<i64>,
    counterfactual_min: Option<i64>,
    counterfactual_max: Option<i64>,
}

#[derive(Debug, Serialize)]
struct CriticalFactorInputObservation {
    value: i64,
    attribute_sequence: u64,
    attribute_observed_micros: u64,
    age_sequences: Option<u64>,
    age_micros: Option<u64>,
    wire_provenance: bool,
    same_wire_as_damage: bool,
}

#[derive(Debug, Serialize)]
struct CriticalFactorEventRecord {
    protocol_pack_digest: String,
    session_id: String,
    run_ordinal: u32,
    damage_sequence: u64,
    damage_observed_micros: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    type_flags: Option<i32>,
    reported_critical: Option<bool>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    observed_damage: i64,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    path: &'static str,
    critical_damage: Option<CriticalFactorInputObservation>,
    lucky_damage: Option<CriticalFactorInputObservation>,
    provider_entity_uuid: i64,
    provider_instance_id: i64,
    provider_level: i32,
    provider_origin_source_type_id: i32,
    provider_origin_source_config_id: i64,
    provider_critical_raw_delta: i64,
    provider_lucky_raw_delta: i64,
    damage_surface_resolution: &'static str,
    damage_surface_candidates: Vec<IntegerStageDamageSurfaceCandidate>,
    candidate_arithmetic: Vec<IntegerStageCandidate>,
    event_time_local_state_authority: bool,
    attack_preimage_complete: bool,
    mitigation_preimage_complete: bool,
    formula_authority: bool,
}

#[derive(Debug, Serialize)]
struct CriticalFactorInterpretationBreakdown {
    path: &'static str,
    compatibility: &'static str,
    counterfactual_relation: &'static str,
    events: u64,
    formula_authority: bool,
}

#[derive(Debug, Clone, Copy)]
enum StageRounding {
    Floor,
    HalfUp,
}

impl StageRounding {
    const ALL: [Self; 2] = [Self::Floor, Self::HalfUp];

    fn label(self) -> &'static str {
        match self {
            Self::Floor => "floor",
            Self::HalfUp => "nearest_half_up",
        }
    }
}

#[derive(Debug)]
struct DamageFormulaSurface {
    schema_version: u16,
    rows_by_key: BTreeMap<(i64, i32), Vec<DamageFormulaSurfaceRow>>,
    rows_by_ability: BTreeMap<i64, Vec<DamageFormulaSurfaceRow>>,
}

#[derive(Debug, Clone)]
struct DamageFormulaSurfaceRow {
    damage_id: String,
    damage_script: Option<String>,
    pve_damage_ratio: Vec<i64>,
    pve_fixed_parameter: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IntegerStageIdentityKey {
    path: &'static str,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    type_flags: Option<i32>,
    reported_critical: Option<bool>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    critical_damage_raw: Option<i64>,
    lucky_damage_raw: Option<i64>,
    stage_input_observation_state: &'static str,
    oldest_stage_input_age_sequences: Option<u64>,
    oldest_stage_input_age_micros: Option<u64>,
    stage_inputs_all_wire_provenance: bool,
    stage_inputs_all_same_wire_as_damage: bool,
}

#[derive(Debug, Clone, Copy)]
struct StageInputFreshness {
    observation_state: &'static str,
    oldest_age_sequences: Option<u64>,
    oldest_age_micros: Option<u64>,
    all_wire_provenance: bool,
    all_same_wire_as_damage: bool,
}

#[derive(Debug, Default)]
struct IntegerStageIdentityAccumulator {
    events: u64,
    complete_stage_inputs: u64,
    compatible_candidate_events: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
    events_without_compatible_candidates: u64,
    observed_damage_sum: i128,
    critical_damage_raw_values: BTreeSet<i64>,
    lucky_damage_raw_values: BTreeSet<i64>,
}

#[derive(Debug, Serialize)]
struct IntegerStageDamageSurfaceJoin {
    surface_runtime_formula_authority: bool,
    identity_groups: usize,
    events: u64,
    events_with_exactly_one_surface_row: u64,
    events_with_ambiguous_surface_rows: u64,
    events_without_surface_row: u64,
    events_with_resolved_damage_script: u64,
    events_without_resolved_damage_script: u64,
    events_with_unique_ability_surface_candidate_when_hit_event_absent: u64,
    events_with_unique_ability_surface_candidate_and_resolved_damage_script_when_hit_event_absent:
        u64,
    events_without_exact_or_unique_ability_surface_candidate: u64,
    unique_ability_surface_candidate_authority: bool,
    damage_script_preimage_breakdown_authority: bool,
    damage_script_preimage_breakdown: Vec<DamageScriptPreimageBreakdown>,
    stage_input_freshness_breakdown_authority: bool,
    stage_input_freshness_breakdown: Vec<StageInputFreshnessBreakdown>,
    groups: Vec<IntegerStageIdentityGroup>,
}

#[derive(Debug, Default)]
struct DamageScriptPreimageAccumulator {
    identity_groups: usize,
    events: u64,
    complete_stage_inputs: u64,
    compatible_candidate_events: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
    events_without_compatible_candidates: u64,
}

#[derive(Debug, Serialize)]
struct DamageScriptPreimageBreakdown {
    surface_binding: &'static str,
    damage_script: String,
    identity_groups: usize,
    events: u64,
    complete_stage_inputs: u64,
    compatible_candidate_events: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
    events_without_compatible_candidates: u64,
    formula_authority: bool,
}

#[derive(Debug, Serialize)]
struct StageInputFreshnessBreakdown {
    path: &'static str,
    observation_state: &'static str,
    oldest_age_bucket: &'static str,
    all_wire_provenance: bool,
    all_same_wire_as_damage: bool,
    identity_groups: usize,
    events: u64,
    complete_stage_inputs: u64,
    compatible_candidate_events: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
    events_without_compatible_candidates: u64,
    formula_authority: bool,
}

#[derive(Debug, Serialize)]
struct IntegerStageIdentityGroup {
    path: &'static str,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    type_flags: Option<i32>,
    reported_critical: Option<bool>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    events: u64,
    complete_stage_inputs: u64,
    compatible_candidate_events: u64,
    exact_stage_independent_events: u64,
    unresolved_stage_or_rounding_events: u64,
    events_without_compatible_candidates: u64,
    observed_damage_sum: String,
    critical_damage_raw_values: Vec<i64>,
    lucky_damage_raw_values: Vec<i64>,
    stage_input_observation_state: &'static str,
    oldest_stage_input_age_sequences: Option<u64>,
    oldest_stage_input_age_micros: Option<u64>,
    stage_inputs_all_wire_provenance: bool,
    stage_inputs_all_same_wire_as_damage: bool,
    damage_surface_resolution: &'static str,
    damage_surface_candidates: Vec<IntegerStageDamageSurfaceCandidate>,
    unique_ability_damage_surface_resolution: &'static str,
    unique_ability_damage_surface_candidates: Vec<IntegerStageDamageSurfaceCandidate>,
}

#[derive(Debug, Serialize)]
struct IntegerStageDamageSurfaceCandidate {
    damage_id: String,
    damage_script: Option<String>,
    pve_damage_ratio: Vec<i64>,
    pve_fixed_parameter: Vec<i64>,
    selected_pve_damage_ratio: Option<i64>,
    selected_pve_fixed_parameter: Option<i64>,
    owner_stage_selection_authority: bool,
}

#[derive(Debug, Default)]
struct FormulaPathSnapshotAccumulator {
    candidate_events: u64,
    complete_input_sets: u64,
    all_inputs_wire_provenance: u64,
    all_inputs_observed_not_after_damage: u64,
    maximum_oldest_input_age_sequences: Option<u64>,
    maximum_oldest_input_age_micros: Option<u64>,
}

#[derive(Debug, Default)]
struct AttributeSnapshotAccumulator {
    required_events: u64,
    present_events: u64,
    missing_events: u64,
    wire_provenance_events: u64,
    non_wire_or_unknown_provenance_events: u64,
    observed_not_after_damage_events: u64,
    observed_after_damage_events: u64,
    same_wire_as_damage_events: u64,
    maximum_age_sequences: Option<u64>,
    maximum_age_micros: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
struct AuditCounts {
    proof_removal_keys: u64,
    proof_complete_magnitude_keys: u64,
    proof_conflicting_magnitude_keys: u64,
    status_windows_started: u64,
    status_windows_ended: u64,
    status_windows_expired: u64,
    status_activation_rows_with_exact_packet_duration: u64,
    status_activation_rows_without_exact_packet_duration: u64,
    status_activation_rows_without_exact_level: u64,
    status_activation_rows_without_exact_parent_origin: u64,
    external_player_windows_with_proc_candidates: u64,
    damage_events: u64,
    damage_events_inside_external_window: u64,
    candidate_critical_events: u64,
    candidate_lucky_events: u64,
    candidate_combined_events: u64,
    combined_candidates_retained_unattributed: u64,
    candidate_events_on_transition_wire: u64,
    candidates_with_exact_window_magnitude: u64,
    candidates_without_exact_window_magnitude: u64,
    candidates_with_multiple_exact_provider_windows: u64,
    candidates_without_single_exact_player_provider_window: u64,
    candidates_missing_critical_chance: u64,
    candidates_missing_lucky_chance: u64,
    candidates_missing_critical_damage: u64,
    candidates_blocked_critical_damage_interpretation: u64,
    rejected_provider_delta_exceeds_current_chance: u64,
    rejected_arithmetic_overflow: u64,
    emitted_critical_contributions: u64,
    emitted_lucky_contributions: u64,
    emitted_combined_contributions: u64,
}

#[derive(Debug, Serialize)]
struct MagnitudeReport {
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    provider_entity_uuid: i64,
    instance_id: i64,
    level: i32,
    origin_source_type_id: i32,
    origin_source_config_id: i64,
    critical_raw_delta: i64,
    lucky_raw_delta: i64,
}

#[derive(Debug, Serialize)]
struct ConservationReport {
    emitted_contribution_events: u64,
    contribution_bucket_events: u64,
    unique_emitted_damage_events: u64,
    duplicate_emitted_damage_events: u64,
    contribution_buckets_match_emitted_events: bool,
    every_emitted_damage_event_has_exactly_one_provider_window: bool,
    ordinary_damage_totals_mutated: bool,
    exact_rational_bucket_arithmetic_is_authoritative_only_within_diagnostic_hypothesis: bool,
    floating_point_total_is_authoritative: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ContributionBucket {
    path: &'static str,
    numerator: String,
    denominator: String,
    events: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BucketKey {
    path: &'static str,
    denominator: i128,
}

#[derive(Debug, Default)]
struct BucketAccumulator {
    numerator: i128,
    events: u64,
}

#[derive(Debug, Serialize)]
struct ContributionExample {
    session_id: String,
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    path: &'static str,
    source_actor_id: u64,
    source_entity_uuid: i64,
    provider_actor_id: u64,
    provider_entity_uuid: i64,
    provider_window_applied_observed_micros: u64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    observed_damage: i64,
    critical: bool,
    lucky: bool,
    current_chance_raw: i64,
    provider_chance_raw_delta: i64,
    critical_damage_raw: Option<i64>,
    numerator: String,
    denominator: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Inspiration proc attribution proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let transition_proof = input_descriptor(&arguments.proof)?;
    let damage_formula_surface = input_descriptor(&arguments.damage_surface)?;
    let critical_factor_proof = input_descriptor(&arguments.critical_factor_proof)?;
    let critical_factor: Value = serde_json::from_reader(BufReader::new(File::open(
        &arguments.critical_factor_proof,
    )?))?;
    validate_critical_factor_proof(
        &critical_factor,
        &arguments.deployment_id,
        &arguments.game_build,
    )?;
    let damage_surface =
        load_damage_formula_surface(&arguments.damage_surface, &arguments.game_build)?;
    let rlogs = arguments
        .rlogs
        .iter()
        .map(|path| input_descriptor(path))
        .collect::<Result<Vec<_>, _>>()?;
    let proof: StatusAttributeProof =
        serde_json::from_reader(BufReader::new(File::open(&arguments.proof)?))?;
    let mut protocol_pack_digests = BTreeSet::new();
    let mut scanned_sessions = Vec::new();
    for (path, descriptor) in arguments.rlogs.iter().zip(&rlogs) {
        let header_reader =
            RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        let deployment_id = header_reader.header().region.identity.deployment_id.clone();
        let game_build = header_reader.header().region.client_build.clone();
        let protocol_pack_digest = header_reader.header().region.protocol_pack_digest.clone();
        if deployment_id != arguments.deployment_id {
            return Err(format!(
                "RLOG deployment {deployment_id} does not match requested {}",
                arguments.deployment_id
            )
            .into());
        }
        if game_build != arguments.game_build {
            return Err(format!(
                "RLOG client build {game_build} does not match requested {}",
                arguments.game_build
            )
            .into());
        }
        protocol_pack_digests.insert(protocol_pack_digest.clone());
        let Some((session_ordinal, session)) =
            proof.sessions.iter().enumerate().find(|(_, session)| {
                session.bytes == descriptor.bytes
                    && session.sha256.eq_ignore_ascii_case(&descriptor.sha256)
            })
        else {
            return Err(format!(
                "{} is absent from the exact transition-proof session inventory",
                path.display()
            )
            .into());
        };
        scanned_sessions.push((
            u32::try_from(session_ordinal).map_err(|_| "too many transition-proof sessions")?,
            session.session_id.clone(),
            protocol_pack_digest,
        ));
    }
    validate_transition_proof(&proof, &arguments, &rlogs, &protocol_pack_digests)?;
    let (magnitudes, conflicting_magnitude_keys) = extract_magnitudes(&proof);
    let level_lifecycle_evidence = level_lifecycle_evidence(&proof, &magnitudes)?;
    let mut player_entities = HashSet::<(u32, u32, i64)>::new();
    let mut candidates = Vec::<CandidateHit>::new();
    let mut counts = AuditCounts {
        proof_removal_keys: u64::try_from(magnitudes.len() + conflicting_magnitude_keys)
            .unwrap_or(u64::MAX),
        proof_complete_magnitude_keys: u64::try_from(magnitudes.len()).unwrap_or(u64::MAX),
        proof_conflicting_magnitude_keys: u64::try_from(conflicting_magnitude_keys)
            .unwrap_or(u64::MAX),
        ..AuditCounts::default()
    };

    for (path, (session_ordinal, session_id, protocol_pack_digest)) in
        arguments.rlogs.iter().zip(scanned_sessions.iter())
    {
        let transition_wires = selected_effect_transition_wires(path)?;
        let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        let mut run_ordinal = 0_u32;
        let mut states = HashMap::<i64, ChanceState>::new();
        let mut active_windows = HashMap::<WindowKey, ActiveWindow>::new();
        while let Some(envelope) = reader.next_event()? {
            if envelope.region.identity.deployment_id != arguments.deployment_id
                || envelope.region.client_build != arguments.game_build
            {
                return Err(format!(
                    "event sequence {} escaped requested deployment/build {}/{}",
                    envelope.sequence, arguments.deployment_id, arguments.game_build
                )
                .into());
            }
            let CanonicalEvent::Timeline(timeline) = &envelope.event else {
                continue;
            };
            let active_before_expiration = active_windows.len();
            active_windows.retain(|_, window| {
                window
                    .expires_at_observed_micros
                    .is_none_or(|expiration| expiration > envelope.time.observed_micros)
            });
            counts.status_windows_expired = counts.status_windows_expired.saturating_add(
                u64::try_from(active_before_expiration.saturating_sub(active_windows.len()))
                    .unwrap_or(u64::MAX),
            );
            match &timeline.kind {
                TimelineEventKind::RunBoundary { state, .. } => match state {
                    RunState::Entered => {
                        run_ordinal = run_ordinal.saturating_add(1);
                        states.clear();
                        active_windows.clear();
                    }
                    RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                    _ => {}
                },
                TimelineEventKind::Actor(actor) => {
                    if actor.kind == ActorKind::Player {
                        player_entities.insert((
                            *session_ordinal,
                            run_ordinal,
                            actor.actor.entity_uuid.0,
                        ));
                    }
                }
                TimelineEventKind::EntityAttributes(attributes) => {
                    observe_attributes(
                        &mut states,
                        attributes,
                        envelope.sequence,
                        envelope.time.observed_micros,
                        wire_key(&envelope.provenance.source),
                    );
                }
                TimelineEventKind::Status(status) if status.effect.0 == INSPIRATION_EFFECT_ID => {
                    let Some(instance_id) = status.instance_id.map(|value| value.0) else {
                        continue;
                    };
                    let Some(provider) = status.source else {
                        continue;
                    };
                    let key = WindowKey {
                        session_ordinal: *session_ordinal,
                        run_ordinal,
                        target_entity_uuid: status.target.entity_uuid.0,
                        provider_entity_uuid: provider.entity_uuid.0,
                        instance_id,
                    };
                    match status.state {
                        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                            if status.level.is_none() {
                                counts.status_activation_rows_without_exact_level = counts
                                    .status_activation_rows_without_exact_level
                                    .saturating_add(1);
                            }
                            if !matches!(
                                status.origin,
                                Some(StatusOrigin {
                                    source_type_id: INSPIRATION_ORIGIN_SOURCE_TYPE_ID,
                                    source_config_id: INSPIRATION_PARENT_EFFECT_ID,
                                })
                            ) {
                                counts.status_activation_rows_without_exact_parent_origin = counts
                                    .status_activation_rows_without_exact_parent_origin
                                    .saturating_add(1);
                            }
                            let Some(duration_millis) = status.duration_millis else {
                                counts.status_activation_rows_without_exact_packet_duration =
                                    counts
                                        .status_activation_rows_without_exact_packet_duration
                                        .saturating_add(1);
                                active_windows.remove(&key);
                                continue;
                            };
                            counts.status_activation_rows_with_exact_packet_duration = counts
                                .status_activation_rows_with_exact_packet_duration
                                .saturating_add(1);
                            let Some(expires_at_observed_micros) =
                                duration_millis.checked_mul(1_000).and_then(|duration| {
                                    envelope.time.observed_micros.checked_add(duration)
                                })
                            else {
                                counts.rejected_arithmetic_overflow =
                                    counts.rejected_arithmetic_overflow.saturating_add(1);
                                active_windows.remove(&key);
                                continue;
                            };
                            if active_windows
                                .insert(
                                    key,
                                    ActiveWindow {
                                        provider_actor_id: provider.actor_id.0,
                                        level: status.level,
                                        origin: status.origin,
                                        applied_observed_micros: envelope.time.observed_micros,
                                        expires_at_observed_micros: Some(
                                            expires_at_observed_micros,
                                        ),
                                    },
                                )
                                .is_none()
                            {
                                counts.status_windows_started =
                                    counts.status_windows_started.saturating_add(1);
                            }
                        }
                        StatusState::Removed | StatusState::Consumed => {
                            if active_windows.remove(&key).is_some() {
                                counts.status_windows_ended =
                                    counts.status_windows_ended.saturating_add(1);
                            }
                        }
                    }
                }
                TimelineEventKind::Damage(damage) => {
                    counts.damage_events = counts.damage_events.saturating_add(1);
                    if damage.amount <= 0 {
                        continue;
                    }
                    let source_entity_uuid = damage.source.entity_uuid.0;
                    let windows = active_windows
                        .iter()
                        .filter(|(key, _)| {
                            key.run_ordinal == run_ordinal
                                && key.target_entity_uuid == source_entity_uuid
                                && key.provider_entity_uuid != source_entity_uuid
                        })
                        .map(|(key, window)| WindowSnapshot {
                            key: *key,
                            provider_actor_id: window.provider_actor_id,
                            level: window.level,
                            origin: window.origin,
                            applied_observed_micros: window.applied_observed_micros,
                        })
                        .collect::<Vec<_>>();
                    if windows.is_empty() {
                        continue;
                    }
                    counts.damage_events_inside_external_window = counts
                        .damage_events_inside_external_window
                        .saturating_add(1);
                    let critical = damage.flags.critical == Some(true);
                    let lucky = damage.flags.lucky == Some(true);
                    match (critical, lucky) {
                        (true, true) => {
                            counts.candidate_combined_events =
                                counts.candidate_combined_events.saturating_add(1)
                        }
                        (true, false) => {
                            counts.candidate_critical_events =
                                counts.candidate_critical_events.saturating_add(1)
                        }
                        (false, true) => {
                            counts.candidate_lucky_events =
                                counts.candidate_lucky_events.saturating_add(1)
                        }
                        (false, false) => continue,
                    }
                    if wire_key(&envelope.provenance.source)
                        .is_some_and(|wire| transition_wires.contains(&(run_ordinal, wire)))
                    {
                        counts.candidate_events_on_transition_wire =
                            counts.candidate_events_on_transition_wire.saturating_add(1);
                        continue;
                    }
                    candidates.push(CandidateHit {
                        session_ordinal: *session_ordinal,
                        session_id: session_id.clone(),
                        protocol_pack_digest: protocol_pack_digest.clone(),
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        run_ordinal,
                        source_actor_id: damage.source.actor_id.0,
                        source_entity_uuid,
                        target_entity_uuid: damage.target.entity_uuid.0,
                        ability_id: damage.ability.map(|value| value.0),
                        amount: damage.amount,
                        normal_value: damage.packet.normal_value,
                        lucky_value: damage.packet.lucky_value,
                        reported_critical: damage.packet.reported_critical,
                        type_flags: damage.packet.type_flags,
                        hit_event_id: damage.hit_event_id,
                        damage_source: damage.damage_source,
                        damage_type: damage.damage_type,
                        owner_level: damage.packet.owner_level,
                        owner_stage: damage.packet.owner_stage,
                        normal_hit: damage.packet.normal_hit,
                        property: damage.packet.property,
                        passive_uuid: damage.packet.passive_uuid,
                        rainbow: damage.packet.rainbow,
                        damage_mode: damage.packet.damage_mode,
                        skill_effect_uuid: damage.packet.skill_effect_uuid,
                        skill_effect_group_index: damage.packet.skill_effect_group_index,
                        skill_effect_component_index: damage.packet.skill_effect_component_index,
                        skill_effect_component_count: damage.packet.skill_effect_component_count,
                        critical,
                        lucky,
                        damage_wire: wire_key(&envelope.provenance.source),
                        chance_state: states.get(&source_entity_uuid).copied().unwrap_or_default(),
                        active_windows: windows,
                    });
                }
                _ => {}
            }
        }
    }

    counts.external_player_windows_with_proc_candidates =
        active_player_window_count(&candidates, &player_entities);
    let combined_packet_evidence = combined_packet_evidence(
        &candidates,
        &magnitudes,
        &player_entities,
        arguments.example_limit,
    );
    let formula_input_snapshot_coverage = formula_input_snapshot_coverage(
        &candidates,
        &magnitudes,
        &player_entities,
        arguments.example_limit,
    );
    let integer_stage_counterfactual_coverage = integer_stage_counterfactual_coverage(
        &candidates,
        &magnitudes,
        &player_entities,
        &damage_surface,
        arguments.example_limit,
    );
    let mut buckets = BTreeMap::<BucketKey, BucketAccumulator>::new();
    let mut examples = Vec::new();
    let mut emitted_damage_events = HashSet::<(u32, u32, u64)>::new();
    let mut duplicate_emitted_damage_events = 0_u64;
    for hit in &candidates {
        let player_windows = hit
            .active_windows
            .iter()
            .filter(|window| {
                player_entities.contains(&(
                    window.key.session_ordinal,
                    window.key.run_ordinal,
                    window.key.provider_entity_uuid,
                ))
            })
            .collect::<Vec<_>>();
        let exact_windows = player_windows
            .iter()
            .filter_map(|window| {
                let level = window.level?;
                let origin = window.origin?;
                let magnitude = magnitudes.get(&MagnitudeKey {
                    session_ordinal: window.key.session_ordinal,
                    run_ordinal: window.key.run_ordinal,
                    target_entity_uuid: window.key.target_entity_uuid,
                    provider_entity_uuid: window.key.provider_entity_uuid,
                    instance_id: window.key.instance_id,
                    level,
                    origin_source_type_id: origin.source_type_id,
                    origin_source_config_id: origin.source_config_id,
                })?;
                Some(((**window).clone(), *magnitude))
            })
            .collect::<Vec<_>>();
        if exact_windows.len() > 1 {
            counts.candidates_with_multiple_exact_provider_windows = counts
                .candidates_with_multiple_exact_provider_windows
                .saturating_add(1);
        }
        if player_windows.len() != 1 || exact_windows.len() != 1 {
            counts.candidates_without_single_exact_player_provider_window = counts
                .candidates_without_single_exact_player_provider_window
                .saturating_add(1);
            if !player_windows.is_empty() && exact_windows.is_empty() {
                counts.candidates_without_exact_window_magnitude = counts
                    .candidates_without_exact_window_magnitude
                    .saturating_add(1);
            }
            if hit.critical && hit.lucky {
                counts.combined_candidates_retained_unattributed = counts
                    .combined_candidates_retained_unattributed
                    .saturating_add(1);
            }
            continue;
        }
        counts.candidates_with_exact_window_magnitude = counts
            .candidates_with_exact_window_magnitude
            .saturating_add(1);
        let (window, magnitude) = &exact_windows[0];
        if hit.critical && hit.lucky {
            match combined_fraction(hit, *magnitude) {
                CombinedFractionOutcome::MissingCriticalChance => {
                    counts.candidates_missing_critical_chance =
                        counts.candidates_missing_critical_chance.saturating_add(1)
                }
                CombinedFractionOutcome::MissingLuckyChance => {
                    counts.candidates_missing_lucky_chance =
                        counts.candidates_missing_lucky_chance.saturating_add(1)
                }
                CombinedFractionOutcome::MissingCriticalDamage => {
                    counts.candidates_missing_critical_damage =
                        counts.candidates_missing_critical_damage.saturating_add(1)
                }
                CombinedFractionOutcome::CriticalDamageInterpretationUnresolved => {
                    counts.candidates_blocked_critical_damage_interpretation = counts
                        .candidates_blocked_critical_damage_interpretation
                        .saturating_add(1)
                }
                CombinedFractionOutcome::ProviderDeltaExceedsChance => {
                    counts.rejected_provider_delta_exceeds_current_chance = counts
                        .rejected_provider_delta_exceeds_current_chance
                        .saturating_add(1)
                }
            }
            if !emitted_damage_events.contains(&(
                hit.session_ordinal,
                hit.run_ordinal,
                hit.sequence,
            )) {
                counts.combined_candidates_retained_unattributed = counts
                    .combined_candidates_retained_unattributed
                    .saturating_add(1);
            }
            continue;
        }
        if hit.critical {
            match critical_fraction(hit, magnitude.critical_raw_delta) {
                FractionOutcome::Exact(numerator, denominator, current_chance) => {
                    if add_fraction(&mut buckets, "critical_proc_bonus", numerator, denominator)
                        .is_err()
                    {
                        counts.rejected_arithmetic_overflow =
                            counts.rejected_arithmetic_overflow.saturating_add(1);
                        continue;
                    }
                    counts.emitted_critical_contributions =
                        counts.emitted_critical_contributions.saturating_add(1);
                    if !emitted_damage_events.insert((
                        hit.session_ordinal,
                        hit.run_ordinal,
                        hit.sequence,
                    )) {
                        duplicate_emitted_damage_events =
                            duplicate_emitted_damage_events.saturating_add(1);
                    }
                    push_example(
                        &mut examples,
                        arguments.example_limit,
                        hit,
                        window,
                        "critical_proc_bonus",
                        current_chance,
                        magnitude.critical_raw_delta,
                        numerator,
                        denominator,
                    );
                }
                FractionOutcome::MissingChance => {
                    counts.candidates_missing_critical_chance =
                        counts.candidates_missing_critical_chance.saturating_add(1)
                }
                FractionOutcome::MissingCriticalDamage => {
                    counts.candidates_missing_critical_damage =
                        counts.candidates_missing_critical_damage.saturating_add(1)
                }
                FractionOutcome::ProviderDeltaExceedsChance => {
                    counts.rejected_provider_delta_exceeds_current_chance = counts
                        .rejected_provider_delta_exceeds_current_chance
                        .saturating_add(1)
                }
                FractionOutcome::ArithmeticOverflow => {
                    counts.rejected_arithmetic_overflow =
                        counts.rejected_arithmetic_overflow.saturating_add(1)
                }
            }
        }
        if hit.lucky {
            match lucky_fraction(hit, magnitude.lucky_raw_delta) {
                FractionOutcome::Exact(numerator, denominator, current_chance) => {
                    if add_fraction(
                        &mut buckets,
                        "lucky_proc_occurrence",
                        numerator,
                        denominator,
                    )
                    .is_err()
                    {
                        counts.rejected_arithmetic_overflow =
                            counts.rejected_arithmetic_overflow.saturating_add(1);
                        continue;
                    }
                    counts.emitted_lucky_contributions =
                        counts.emitted_lucky_contributions.saturating_add(1);
                    if !emitted_damage_events.insert((
                        hit.session_ordinal,
                        hit.run_ordinal,
                        hit.sequence,
                    )) {
                        duplicate_emitted_damage_events =
                            duplicate_emitted_damage_events.saturating_add(1);
                    }
                    push_example(
                        &mut examples,
                        arguments.example_limit,
                        hit,
                        window,
                        "lucky_proc_occurrence",
                        current_chance,
                        magnitude.lucky_raw_delta,
                        numerator,
                        denominator,
                    );
                }
                FractionOutcome::MissingChance => {
                    counts.candidates_missing_lucky_chance =
                        counts.candidates_missing_lucky_chance.saturating_add(1)
                }
                FractionOutcome::MissingCriticalDamage => {
                    counts.candidates_missing_critical_damage =
                        counts.candidates_missing_critical_damage.saturating_add(1)
                }
                FractionOutcome::ProviderDeltaExceedsChance => {
                    counts.rejected_provider_delta_exceeds_current_chance = counts
                        .rejected_provider_delta_exceeds_current_chance
                        .saturating_add(1)
                }
                FractionOutcome::ArithmeticOverflow => {
                    counts.rejected_arithmetic_overflow =
                        counts.rejected_arithmetic_overflow.saturating_add(1)
                }
            }
        }
    }

    if formula_input_snapshot_coverage.exact_single_provider_candidate_events
        != counts.candidates_with_exact_window_magnitude
    {
        return Err("formula-input snapshot coverage escaped the exact candidate subset".into());
    }
    let contribution_buckets = buckets
        .into_iter()
        .map(|(key, value)| ContributionBucket {
            path: key.path,
            numerator: value.numerator.to_string(),
            denominator: key.denominator.to_string(),
            events: value.events,
        })
        .collect::<Vec<_>>();
    let contribution_bucket_events = contribution_buckets
        .iter()
        .map(|bucket| bucket.events)
        .fold(0_u64, u64::saturating_add);
    let emitted_contribution_events = counts
        .emitted_critical_contributions
        .saturating_add(counts.emitted_lucky_contributions)
        .saturating_add(counts.emitted_combined_contributions);
    let unique_emitted_damage_events =
        u64::try_from(emitted_damage_events.len()).unwrap_or(u64::MAX);
    let conservation = ConservationReport {
        emitted_contribution_events,
        contribution_bucket_events,
        unique_emitted_damage_events,
        duplicate_emitted_damage_events,
        contribution_buckets_match_emitted_events: contribution_bucket_events
            == emitted_contribution_events,
        every_emitted_damage_event_has_exactly_one_provider_window: duplicate_emitted_damage_events
            == 0
            && unique_emitted_damage_events == emitted_contribution_events,
        ordinary_damage_totals_mutated: false,
        exact_rational_bucket_arithmetic_is_authoritative_only_within_diagnostic_hypothesis: true,
        floating_point_total_is_authoritative: false,
    };
    let exact_removal_magnitudes = magnitudes
        .into_iter()
        .map(|(key, value)| MagnitudeReport {
            session_id: proof
                .sessions
                .get(key.session_ordinal as usize)
                .map(|session| session.session_id.clone())
                .unwrap_or_default(),
            run_ordinal: key.run_ordinal,
            target_entity_uuid: key.target_entity_uuid,
            provider_entity_uuid: key.provider_entity_uuid,
            instance_id: key.instance_id,
            level: key.level,
            origin_source_type_id: key.origin_source_type_id,
            origin_source_config_id: key.origin_source_config_id,
            critical_raw_delta: value.critical_raw_delta,
            lucky_raw_delta: value.lucky_raw_delta,
        })
        .collect();
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-inspiration-proc-attribution-proof",
        policy: AuditPolicy {
            runtime_use: "offline_research_only_not_loaded_by_live_meter",
            occurrence_authority: "current_build_canonical_packet_damage_flags_and_values",
            magnitude_authority: "exact_single_effect_removal_equations_for_raw_add_attributes_11712_and_11782",
            critical_path: "critical-only packet rows use the exact current-build additive-bonus interpretation of attribute 12510; provider chance share remains component-scoped",
            lucky_path: "noncritical_lucky_uses_full_lucky_row; critical_lucky_is_retained_unattributed_until_stage_order_is_proven",
            combined_path: "joint_lucky_and_critical_candidates_retained_per_event; no combined contribution emitted while_critical_damage_interpretation_is_unresolved",
            buffering_bound: "one_exact_inspiration_instance_window; expiration_comes_only_from_duration_millis_on_that_exact_locally_observed_status_packet",
            remote_player_packets_required: false,
            remote_player_packets_treated_as_zero: false,
            remote_player_packets_synthesized: false,
            unobserved_effect_levels_interpolated: false,
            unobserved_or_mismatched_parent_origins_attributed: false,
            historical_or_current_snapshot_substitution_allowed: false,
            last_observed_local_attribute_is_event_time_formula_authority: false,
            snapshot_age_threshold_is_formula_authority: false,
            formula_input_snapshot_authority: false,
            integer_stage_candidate_family_authority: false,
            integer_stage_counterfactual_authority: false,
            damage_formula_surface_runtime_authority: false,
            ambiguous_damage_surface_rows_inferred: false,
            owner_stage_array_selection_is_formula_authority: false,
            missing_hit_event_id_may_be_synthesized: false,
            unique_ability_damage_surface_candidate_authority: false,
            damage_script_preimage_breakdown_authority: false,
            damage_surface_identity_groups_include_exact_stage_inputs: true,
            damage_surface_identity_groups_include_exact_stage_input_freshness: true,
            stage_input_freshness_breakdown_authority: false,
            critical_damage_raw_interpretation_authority: true,
            partial_effect_credit_may_be_displayed_as_complete_effect_rdps: false,
            formula_authority: true,
            full_effect_composition_authority: false,
            canonical_conservation_replay_authority: false,
            provider_rdps_credit_authorized: false,
            runtime_promotion_allowed: false,
            ui_display_allowed: false,
            unresolved_evidence_is_hidden: false,
        },
        deployment_id: arguments.deployment_id.clone(),
        game_build: arguments.game_build.clone(),
        protocol_pack_digests: protocol_pack_digests.into_iter().collect(),
        effect_id: INSPIRATION_EFFECT_ID,
        transition_proof,
        transition_proof_schema_version: proof.schema_version,
        damage_formula_surface,
        damage_formula_surface_schema_version: damage_surface.schema_version,
        critical_factor_proof,
        critical_factor_proof_schema_version: critical_factor
            .get("schema_version")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or_default(),
        rlogs,
        counts,
        exact_removal_magnitudes,
        level_lifecycle_evidence,
        formula_input_snapshot_coverage,
        integer_stage_counterfactual_coverage,
        combined_packet_evidence,
        contribution_buckets,
        conservation,
        examples,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn load_damage_formula_surface(
    path: &Path,
    expected_game_build: &str,
) -> Result<DamageFormulaSurface, Box<dyn std::error::Error>> {
    let surface: Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let schema_version = surface
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or("damage formula surface has no supported schema version")?;
    let game_build = surface
        .get("game_build")
        .and_then(Value::as_str)
        .ok_or("damage formula surface is missing game_build")?;
    if !(1..=2).contains(&schema_version)
        || surface.get("generated_by").and_then(Value::as_str)
            != Some("rlogs-bpsr-damage-attr-semantic-surface")
        || game_build != expected_game_build
        || surface
            .pointer("/policy/runtime_formula_authority")
            .and_then(Value::as_bool)
            != Some(false)
        || surface
            .pointer("/policy/exact_build_table_required")
            .and_then(Value::as_bool)
            != Some(true)
        || surface
            .pointer("/policy/unresolved_rows_hidden")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err("damage formula surface identity or fail-closed policy is invalid".into());
    }
    if schema_version >= 2
        && surface
            .pointer("/policy/damage_script_field_rule")
            .and_then(Value::as_str)
            != Some(
                "damage_script mirrors decoded DamageScript exactly and is grouping evidence, not server formula authority",
            )
    {
        return Err("damage formula surface has no exact decoded DamageScript policy".into());
    }
    let rows = surface
        .get("rows")
        .and_then(Value::as_object)
        .ok_or("damage formula surface is missing rows")?;
    let lookup = surface
        .get("linked_hit_event_candidate_lookup")
        .and_then(Value::as_object)
        .ok_or("damage formula surface is missing linked hit-event lookup")?;
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
            let damage_script = if schema_version >= 2 {
                let value = row
                    .get("damage_script")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        format!("damage formula surface row {damage_id} has no DamageScript")
                    })?;
                (!value.is_empty()).then(|| value.to_owned())
            } else {
                None
            };
            candidates.push(DamageFormulaSurfaceRow {
                damage_id,
                damage_script,
                pve_damage_ratio: surface_array_values(
                    row.get("int_array_pool_1_candidates_by_offset")
                        .and_then(|value| value.get("28")),
                )?,
                pve_fixed_parameter: surface_array_values(
                    row.get("int_array_pool_1_candidates_by_offset")
                        .and_then(|value| value.get("32")),
                )?,
            });
        }
        rows_by_key.insert((ability_id, hit_event_id), candidates);
    }
    let mut rows_by_ability_and_damage_id =
        BTreeMap::<i64, BTreeMap<String, DamageFormulaSurfaceRow>>::new();
    for ((ability_id, _), candidates) in &rows_by_key {
        let ability_rows = rows_by_ability_and_damage_id
            .entry(*ability_id)
            .or_default();
        for candidate in candidates {
            ability_rows
                .entry(candidate.damage_id.clone())
                .or_insert_with(|| candidate.clone());
        }
    }
    let rows_by_ability = rows_by_ability_and_damage_id
        .into_iter()
        .map(|(ability_id, rows)| (ability_id, rows.into_values().collect()))
        .collect();
    Ok(DamageFormulaSurface {
        schema_version,
        rows_by_key,
        rows_by_ability,
    })
}

fn validate_critical_factor_proof(
    proof: &Value,
    expected_deployment_id: &str,
    expected_game_build: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let valid = proof.get("schema_version").and_then(Value::as_u64) == Some(1)
        && proof.get("deployment_id").and_then(Value::as_str) == Some(expected_deployment_id)
        && proof.get("game_build").and_then(Value::as_str) == Some(expected_game_build)
        && proof.get("proof_state").and_then(Value::as_str)
            == Some("critical-only-attribute-12510-additive-bonus-factor-proven")
        && proof
            .pointer("/interpretation/attribute_id")
            .and_then(Value::as_i64)
            == Some(i64::from(CRITICAL_DAMAGE_ATTRIBUTE_ID))
        && proof
            .pointer("/interpretation/authoritative_interpretation")
            .and_then(Value::as_str)
            == Some("additive_bonus")
        && proof
            .pointer("/policy/critical_only_factor_interpretation_authority")
            .and_then(Value::as_bool)
            == Some(true)
        && proof
            .pointer("/policy/ordinary_damage_totals_unchanged")
            .and_then(Value::as_bool)
            == Some(true)
        && proof
            .pointer("/policy/current_character_snapshot_substitution_allowed")
            .and_then(Value::as_bool)
            == Some(false)
        && proof
            .pointer("/policy/combined_critical_lucky_order_authority")
            .and_then(Value::as_bool)
            == Some(false)
        && proof
            .pointer("/runtime_decision/critical_only_team_luck_component_promotion_allowed")
            .and_then(Value::as_bool)
            == Some(true)
        && proof
            .pointer("/runtime_decision/combined_critical_lucky_promotion_allowed")
            .and_then(Value::as_bool)
            == Some(false);
    if !valid {
        return Err("critical-factor proof is not exact-build additive-bonus authority".into());
    }
    Ok(())
}

fn surface_array_values(value: Option<&Value>) -> Result<Vec<i64>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "damage formula surface array candidate is missing values".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "damage formula surface array value is not an integer".to_owned())
        })
        .collect()
}

fn input_descriptor(path: &Path) -> Result<InputDescriptor, Box<dyn std::error::Error>> {
    Ok(InputDescriptor {
        path: path.to_string_lossy().replace('\\', "/"),
        bytes: fs::metadata(path)?.len(),
        sha256: sha256_file(path)?,
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
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_transition_proof(
    proof: &StatusAttributeProof,
    arguments: &Arguments,
    rlogs: &[InputDescriptor],
    protocol_pack_digests: &BTreeSet<String>,
) -> Result<(), String> {
    if !(MIN_STATUS_ATTRIBUTE_PROOF_SCHEMA_VERSION..=MAX_STATUS_ATTRIBUTE_PROOF_SCHEMA_VERSION)
        .contains(&proof.schema_version)
    {
        return Err(format!(
            "transition proof schema {} is outside supported schemas {}..={}",
            proof.schema_version,
            MIN_STATUS_ATTRIBUTE_PROOF_SCHEMA_VERSION,
            MAX_STATUS_ATTRIBUTE_PROOF_SCHEMA_VERSION
        ));
    }
    if proof.expected_deployment_id.as_deref() != Some(arguments.deployment_id.as_str())
        || proof.expected_game_build.as_deref() != Some(arguments.game_build.as_str())
    {
        return Err("transition proof is not locked to the requested deployment/build".to_owned());
    }
    if proof.selected_effect_ids != [INSPIRATION_EFFECT_ID]
        || ![
            CRITICAL_CHANCE_ATTRIBUTE_ID,
            CRITICAL_CHANCE_ADD_ATTRIBUTE_ID,
            LUCKY_CHANCE_ATTRIBUTE_ID,
            LUCKY_CHANCE_ADD_ATTRIBUTE_ID,
            CRITICAL_DAMAGE_ATTRIBUTE_ID,
        ]
        .into_iter()
        .all(|attribute_id| proof.selected_attribute_ids.contains(&attribute_id))
    {
        return Err(
            "transition proof does not have the exact Inspiration effect and required attributes"
                .to_owned(),
        );
    }
    if proof.sessions.len() != rlogs.len() {
        return Err(format!(
            "transition proof session count {} does not match the {} exact RLOG inputs",
            proof.sessions.len(),
            rlogs.len()
        ));
    }
    let mut matched = BTreeSet::new();
    for session in &proof.sessions {
        let Some((index, _)) = rlogs.iter().enumerate().find(|(_, rlog)| {
            session.bytes == rlog.bytes && session.sha256.eq_ignore_ascii_case(&rlog.sha256)
        }) else {
            return Err(
                "transition proof session identity does not match any exact RLOG input".to_owned(),
            );
        };
        if !matched.insert(index)
            || session.deployment_id != arguments.deployment_id
            || session.game_build != arguments.game_build
            || !protocol_pack_digests.contains(&session.protocol_pack_digest)
            || session.rlog.trim().is_empty()
            || session.session_id.trim().is_empty()
        {
            return Err(
                "transition proof session identity does not match the exact RLOG bytes and header"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn extract_magnitudes(
    proof: &StatusAttributeProof,
) -> (BTreeMap<MagnitudeKey, ProvenMagnitude>, usize) {
    let mut accumulators = BTreeMap::<MagnitudeKey, MagnitudeAccumulator>::new();
    let session_ordinals = proof
        .sessions
        .iter()
        .enumerate()
        .filter_map(|(index, session)| {
            u32::try_from(index)
                .ok()
                .map(|ordinal| (session.session_id.as_str(), ordinal))
        })
        .collect::<HashMap<_, _>>();
    for system in &proof.wire_additive_equation_systems {
        if !matches!(
            system.attribute_id,
            CRITICAL_CHANCE_ADD_ATTRIBUTE_ID | LUCKY_CHANCE_ADD_ATTRIBUTE_ID
        ) {
            continue;
        }
        for equation in &system.equations {
            if equation.raw_attribute_delta >= 0
                || equation.terms.len() != 1
                || equation.terms[0].effect_id != INSPIRATION_EFFECT_ID
                || equation.terms[0].signed_presence_delta != -1
            {
                continue;
            }
            let Some(level) = equation.terms[0].level.filter(|level| *level > 0) else {
                continue;
            };
            let Some(origin) = equation.terms[0].origin.filter(|origin| {
                origin.source_type_id == INSPIRATION_ORIGIN_SOURCE_TYPE_ID
                    && origin.source_config_id == INSPIRATION_PARENT_EFFECT_ID
            }) else {
                continue;
            };
            for example in &equation.examples {
                let Some(session_ordinal) = session_ordinals.get(example.session_id.as_str())
                else {
                    continue;
                };
                let Some(status) = example.status_instances.iter().find(|status| {
                    status.effect_id == INSPIRATION_EFFECT_ID && status.state == "removed"
                }) else {
                    continue;
                };
                let Some(instance_id) = status.instance_id else {
                    continue;
                };
                let Some(provider_entity_uuid) = status.source_entity_uuid else {
                    continue;
                };
                let entry = accumulators
                    .entry(MagnitudeKey {
                        session_ordinal: *session_ordinal,
                        run_ordinal: example.run_ordinal,
                        target_entity_uuid: example.target_entity_uuid,
                        provider_entity_uuid,
                        instance_id,
                        level,
                        origin_source_type_id: origin.source_type_id,
                        origin_source_config_id: origin.source_config_id,
                    })
                    .or_default();
                let magnitude = equation.raw_attribute_delta.saturating_neg();
                if system.attribute_id == CRITICAL_CHANCE_ADD_ATTRIBUTE_ID {
                    entry.critical_raw_deltas.insert(magnitude);
                } else {
                    entry.lucky_raw_deltas.insert(magnitude);
                }
            }
        }
    }
    let mut proven = BTreeMap::new();
    let mut conflicting = 0_usize;
    for (key, value) in accumulators {
        if value.critical_raw_deltas.len() == 1 && value.lucky_raw_deltas.len() == 1 {
            proven.insert(
                key,
                ProvenMagnitude {
                    critical_raw_delta: *value.critical_raw_deltas.first().expect("length checked"),
                    lucky_raw_delta: *value.lucky_raw_deltas.first().expect("length checked"),
                },
            );
        } else {
            conflicting = conflicting.saturating_add(1);
        }
    }
    (proven, conflicting)
}

fn level_lifecycle_evidence(
    proof: &StatusAttributeProof,
    magnitudes: &BTreeMap<MagnitudeKey, ProvenMagnitude>,
) -> Result<Vec<LevelLifecycleEvidence>, String> {
    let levels = magnitudes
        .keys()
        .map(|key| key.level)
        .collect::<BTreeSet<_>>();
    let mut reports = Vec::new();
    for level in levels {
        let raw_deltas = magnitudes
            .iter()
            .filter(|(key, _)| key.level == level)
            .flat_map(|(_, magnitude)| [magnitude.critical_raw_delta, magnitude.lucky_raw_delta])
            .collect::<BTreeSet<_>>();
        let Some(exact_instance_raw_delta) = raw_deltas
            .iter()
            .next()
            .copied()
            .filter(|_| raw_deltas.len() == 1)
        else {
            return Err(format!(
                "effect level {level} does not have one exact Critical/Lucky instance magnitude"
            ));
        };
        let attributes = [
            CRITICAL_CHANCE_ADD_ATTRIBUTE_ID,
            LUCKY_CHANCE_ADD_ATTRIBUTE_ID,
        ]
        .into_iter()
        .map(|attribute_id| {
            attribute_lifecycle_evidence(proof, attribute_id, level, exact_instance_raw_delta)
        })
        .collect::<Vec<_>>();
        let reversible_static_transform_proven = attributes.iter().all(|attribute| {
            attribute.reversible_static_gate_passed
                && attribute.matched_lifecycle_gate_passed
                && attribute.coefficient_consistent_with_instance_magnitude
        });
        let blockers = attributes
            .iter()
            .flat_map(|attribute| {
                let mut blockers = Vec::new();
                if !attribute.reversible_static_gate_passed {
                    blockers.push(format!(
                        "attribute:{}:reversible-static:{}",
                        attribute.attribute_id,
                        joined_statuses(&attribute.reversible_statuses)
                    ));
                }
                if !attribute.matched_lifecycle_gate_passed {
                    blockers.push(format!(
                        "attribute:{}:matched-lifecycle:{}",
                        attribute.attribute_id,
                        joined_statuses(&attribute.matched_statuses)
                    ));
                }
                if !attribute.coefficient_consistent_with_instance_magnitude {
                    blockers.push(format!(
                        "attribute:{}:coefficient-does-not-match-exact-instance-magnitude",
                        attribute.attribute_id
                    ));
                }
                blockers
            })
            .collect::<Vec<_>>();
        reports.push(LevelLifecycleEvidence {
            level,
            exact_instance_raw_delta,
            attributes,
            reversible_static_transform_proven,
            blockers,
        });
    }
    Ok(reports)
}

fn joined_statuses(statuses: &[String]) -> String {
    if statuses.is_empty() {
        "missing".to_owned()
    } else {
        statuses.join("+")
    }
}

fn attribute_lifecycle_evidence(
    proof: &StatusAttributeProof,
    attribute_id: i32,
    level: i32,
    exact_instance_raw_delta: i64,
) -> AttributeLifecycleEvidence {
    let fingerprint_matches = |fingerprint: &ProofFingerprint| {
        fingerprint.effect_id == INSPIRATION_EFFECT_ID
            && fingerprint.origin
                == Some(ProofOrigin {
                    source_type_id: INSPIRATION_ORIGIN_SOURCE_TYPE_ID,
                    source_config_id: INSPIRATION_PARENT_EFFECT_ID,
                })
            && fingerprint.level == Some(level)
            && fingerprint.part_id.is_none()
            && fingerprint.stacks == Some(1)
            && fingerprint.count == Some(-1)
    };
    let reversible_rows = proof
        .reversible_static_coefficient_proofs
        .iter()
        .filter(|row| row.attribute_id == attribute_id && fingerprint_matches(&row.fingerprint))
        .collect::<Vec<_>>();
    let lifecycle_rows = proof
        .matched_lifecycle_coefficient_proofs
        .iter()
        .filter(|row| row.attribute_id == attribute_id && fingerprint_matches(&row.fingerprint))
        .collect::<Vec<_>>();
    let mut reversible_statuses = reversible_rows
        .iter()
        .map(|row| row.status.clone())
        .collect::<Vec<_>>();
    reversible_statuses.sort();
    reversible_statuses.dedup();
    let mut matched_statuses = lifecycle_rows
        .iter()
        .map(|row| row.status.clone())
        .collect::<Vec<_>>();
    matched_statuses.sort();
    matched_statuses.dedup();
    let normalized_coefficient_counts = summed_counts(
        reversible_rows
            .iter()
            .map(|row| &row.normalized_coefficient_counts),
    );
    let exact_coefficient_counts = summed_counts(
        lifecycle_rows
            .iter()
            .map(|row| &row.exact_coefficient_counts),
    );
    let observed_coefficients = normalized_coefficient_counts
        .keys()
        .chain(exact_coefficient_counts.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let reversible_static_gate_passed = reversible_rows.len() == 1
        && reversible_rows[0].status == "proven_reversible_static_coefficient"
        && reversible_rows[0].proven_coefficient_units == Some(exact_instance_raw_delta);
    let matched_lifecycle_gate_passed = lifecycle_rows.len() == 1
        && lifecycle_rows[0].status == "proven_matched_lifecycle_coefficient"
        && lifecycle_rows[0].proven_coefficient_units == Some(exact_instance_raw_delta);
    AttributeLifecycleEvidence {
        attribute_id,
        matching_reversible_rows: reversible_rows.len(),
        reversible_statuses,
        reversible_proven_coefficient_units: (reversible_rows.len() == 1)
            .then(|| reversible_rows[0].proven_coefficient_units)
            .flatten(),
        normalized_coefficient_counts,
        apply_occurrences: reversible_rows
            .iter()
            .map(|row| row.apply_occurrences)
            .fold(0, u64::saturating_add),
        remove_occurrences: reversible_rows
            .iter()
            .map(|row| row.remove_occurrences)
            .fold(0, u64::saturating_add),
        independent_run_contexts: reversible_rows
            .first()
            .filter(|_| reversible_rows.len() == 1)
            .map_or(0, |row| row.independent_run_contexts),
        cross_actor_occurrences: reversible_rows
            .iter()
            .map(|row| row.cross_actor_occurrences)
            .fold(0, u64::saturating_add),
        matching_lifecycle_rows: lifecycle_rows.len(),
        matched_statuses,
        matched_proven_coefficient_units: (lifecycle_rows.len() == 1)
            .then(|| lifecycle_rows[0].proven_coefficient_units)
            .flatten(),
        exact_coefficient_counts,
        exact_pair_count: lifecycle_rows
            .iter()
            .map(|row| row.exact_pair_count)
            .fold(0, u64::saturating_add),
        contradictory_pair_count: lifecycle_rows
            .iter()
            .map(|row| row.contradictory_pair_count)
            .fold(0, u64::saturating_add),
        ambiguous_instance_count: lifecycle_rows
            .iter()
            .map(|row| row.ambiguous_instance_count)
            .fold(0, u64::saturating_add),
        application_only_instance_count: lifecycle_rows
            .iter()
            .map(|row| row.application_only_instance_count)
            .fold(0, u64::saturating_add),
        removal_only_instance_count: lifecycle_rows
            .iter()
            .map(|row| row.removal_only_instance_count)
            .fold(0, u64::saturating_add),
        matched_independent_run_contexts: lifecycle_rows
            .first()
            .filter(|_| lifecycle_rows.len() == 1)
            .map_or(0, |row| row.independent_run_contexts),
        cross_actor_exact_pairs: lifecycle_rows
            .iter()
            .map(|row| row.cross_actor_exact_pairs)
            .fold(0, u64::saturating_add),
        matched_examples: lifecycle_rows
            .iter()
            .flat_map(|row| row.examples.iter().cloned())
            .collect(),
        coefficient_consistent_with_instance_magnitude: !observed_coefficients.is_empty()
            && observed_coefficients == BTreeSet::from([exact_instance_raw_delta]),
        reversible_static_gate_passed,
        matched_lifecycle_gate_passed,
    }
}

fn summed_counts<'a>(maps: impl Iterator<Item = &'a BTreeMap<i64, u64>>) -> BTreeMap<i64, u64> {
    let mut counts = BTreeMap::new();
    for map in maps {
        for (coefficient, count) in map {
            let entry = counts.entry(*coefficient).or_insert(0_u64);
            *entry = entry.saturating_add(*count);
        }
    }
    counts
}

fn selected_effect_transition_wires(
    path: &Path,
) -> Result<HashSet<(u32, WireKey)>, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut run_ordinal = 0_u32;
    let mut wires = HashSet::new();
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
            TimelineEventKind::Status(status) if status.effect.0 == INSPIRATION_EFFECT_ID => {
                if let Some(wire) = wire_key(&envelope.provenance.source) {
                    wires.insert((run_ordinal, wire));
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

fn observe_attributes(
    states: &mut HashMap<i64, ChanceState>,
    event: &rlogs_events::EntityAttributeEvent,
    sequence: u64,
    observed_micros: u64,
    wire: Option<WireKey>,
) {
    let mut state = if event.update_kind == EntityAttributeUpdateKind::Snapshot {
        ChanceState::default()
    } else {
        states
            .get(&event.actor.entity_uuid.0)
            .copied()
            .unwrap_or_default()
    };
    for attribute in &event.attributes {
        let Some(value) = decode_attribute(attribute) else {
            continue;
        };
        let observation = ObservedAttribute {
            value,
            sequence,
            observed_micros,
            wire,
        };
        match attribute.attribute_id {
            CRITICAL_CHANCE_ATTRIBUTE_ID => state.critical_chance_raw = Some(observation),
            LUCKY_CHANCE_ATTRIBUTE_ID => state.lucky_chance_raw = Some(observation),
            CRITICAL_DAMAGE_ATTRIBUTE_ID => state.critical_damage_raw = Some(observation),
            LUCKY_DAMAGE_ATTRIBUTE_ID => state.lucky_damage_raw = Some(observation),
            _ => {}
        }
    }
    states.insert(event.actor.entity_uuid.0, state);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FractionOutcome {
    Exact(i128, i128, i64),
    MissingChance,
    MissingCriticalDamage,
    ProviderDeltaExceedsChance,
    ArithmeticOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CombinedFractionOutcome {
    MissingCriticalChance,
    MissingLuckyChance,
    MissingCriticalDamage,
    CriticalDamageInterpretationUnresolved,
    ProviderDeltaExceedsChance,
}

fn combined_fraction(hit: &CandidateHit, magnitude: ProvenMagnitude) -> CombinedFractionOutcome {
    let Some(critical_chance) = hit
        .chance_state
        .critical_chance_raw
        .map(|observation| observation.value)
        .filter(|value| *value > 0)
    else {
        return CombinedFractionOutcome::MissingCriticalChance;
    };
    let Some(lucky_chance) = hit
        .chance_state
        .lucky_chance_raw
        .map(|observation| observation.value)
        .filter(|value| *value > 0)
    else {
        return CombinedFractionOutcome::MissingLuckyChance;
    };
    let Some(_) = hit
        .chance_state
        .critical_damage_raw
        .map(|observation| observation.value)
        .filter(|value| *value > 0)
    else {
        return CombinedFractionOutcome::MissingCriticalDamage;
    };
    if magnitude.critical_raw_delta <= 0
        || magnitude.critical_raw_delta > critical_chance
        || magnitude.lucky_raw_delta <= 0
        || magnitude.lucky_raw_delta > lucky_chance
    {
        return CombinedFractionOutcome::ProviderDeltaExceedsChance;
    }

    CombinedFractionOutcome::CriticalDamageInterpretationUnresolved
}

fn critical_fraction(hit: &CandidateHit, provider_delta: i64) -> FractionOutcome {
    let Some(current_chance) = hit
        .chance_state
        .critical_chance_raw
        .map(|observation| observation.value)
        .filter(|value| *value > 0)
    else {
        return FractionOutcome::MissingChance;
    };
    let Some(critical_damage_raw) = hit
        .chance_state
        .critical_damage_raw
        .map(|observation| observation.value)
        .filter(|value| *value > 0)
    else {
        return FractionOutcome::MissingCriticalDamage;
    };
    if provider_delta <= 0 || provider_delta > current_chance {
        return FractionOutcome::ProviderDeltaExceedsChance;
    }
    let Some((numerator, denominator)) = exact_external_critical_chance_fraction(
        hit.amount,
        current_chance,
        provider_delta,
        critical_damage_raw,
        CriticalDamageFactorInterpretation::AdditiveBonus,
    ) else {
        return FractionOutcome::ArithmeticOverflow;
    };
    FractionOutcome::Exact(numerator, denominator, current_chance)
}

fn lucky_fraction(hit: &CandidateHit, provider_delta: i64) -> FractionOutcome {
    let Some(current_chance) = hit
        .chance_state
        .lucky_chance_raw
        .map(|observation| observation.value)
        .filter(|value| *value > 0)
    else {
        return FractionOutcome::MissingChance;
    };
    if provider_delta <= 0 || provider_delta > current_chance {
        return FractionOutcome::ProviderDeltaExceedsChance;
    }
    if hit.critical {
        return FractionOutcome::ArithmeticOverflow;
    }
    let Some((numerator, denominator)) =
        exact_external_lucky_chance_fraction(hit.amount, current_chance, provider_delta)
    else {
        return FractionOutcome::ArithmeticOverflow;
    };
    FractionOutcome::Exact(numerator, denominator, current_chance)
}

fn add_fraction(
    buckets: &mut BTreeMap<BucketKey, BucketAccumulator>,
    path: &'static str,
    numerator: i128,
    denominator: i128,
) -> Result<(), ()> {
    let entry = buckets.entry(BucketKey { path, denominator }).or_default();
    entry.numerator = entry.numerator.checked_add(numerator).ok_or(())?;
    entry.events = entry.events.saturating_add(1);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_example(
    examples: &mut Vec<ContributionExample>,
    example_limit: usize,
    hit: &CandidateHit,
    window: &WindowSnapshot,
    path: &'static str,
    current_chance_raw: i64,
    provider_chance_raw_delta: i64,
    numerator: i128,
    denominator: i128,
) {
    if examples.len() >= example_limit {
        return;
    }
    examples.push(ContributionExample {
        session_id: hit.session_id.clone(),
        sequence: hit.sequence,
        observed_micros: hit.observed_micros,
        run_ordinal: hit.run_ordinal,
        path,
        source_actor_id: hit.source_actor_id,
        source_entity_uuid: hit.source_entity_uuid,
        provider_actor_id: window.provider_actor_id,
        provider_entity_uuid: window.key.provider_entity_uuid,
        provider_window_applied_observed_micros: window.applied_observed_micros,
        target_entity_uuid: hit.target_entity_uuid,
        ability_id: hit.ability_id,
        observed_damage: hit.amount,
        critical: hit.critical,
        lucky: hit.lucky,
        current_chance_raw,
        provider_chance_raw_delta,
        critical_damage_raw: hit
            .chance_state
            .critical_damage_raw
            .map(|observation| observation.value),
        numerator: numerator.to_string(),
        denominator: denominator.to_string(),
    });
}

fn formula_input_snapshot_coverage(
    candidates: &[CandidateHit],
    magnitudes: &BTreeMap<MagnitudeKey, ProvenMagnitude>,
    players: &HashSet<(u32, u32, i64)>,
    example_limit: usize,
) -> FormulaInputSnapshotCoverage {
    let mut paths = BTreeMap::<&'static str, FormulaPathSnapshotAccumulator>::new();
    let mut attributes = BTreeMap::<i32, AttributeSnapshotAccumulator>::from([
        (
            CRITICAL_CHANCE_ATTRIBUTE_ID,
            AttributeSnapshotAccumulator::default(),
        ),
        (
            LUCKY_CHANCE_ATTRIBUTE_ID,
            AttributeSnapshotAccumulator::default(),
        ),
        (
            CRITICAL_DAMAGE_ATTRIBUTE_ID,
            AttributeSnapshotAccumulator::default(),
        ),
        (
            LUCKY_DAMAGE_ATTRIBUTE_ID,
            AttributeSnapshotAccumulator::default(),
        ),
    ]);
    let mut exact_single_provider_candidate_events = 0_u64;
    let mut observed_examples = Vec::new();
    let mut missing_examples = Vec::new();
    for hit in candidates {
        let player_windows = hit
            .active_windows
            .iter()
            .filter(|window| {
                players.contains(&(
                    window.key.session_ordinal,
                    window.key.run_ordinal,
                    window.key.provider_entity_uuid,
                ))
            })
            .collect::<Vec<_>>();
        if player_windows.len() != 1 {
            continue;
        }
        let window = player_windows[0];
        let (Some(level), Some(origin)) = (window.level, window.origin) else {
            continue;
        };
        if !magnitudes.contains_key(&MagnitudeKey {
            session_ordinal: window.key.session_ordinal,
            run_ordinal: window.key.run_ordinal,
            target_entity_uuid: window.key.target_entity_uuid,
            provider_entity_uuid: window.key.provider_entity_uuid,
            instance_id: window.key.instance_id,
            level,
            origin_source_type_id: origin.source_type_id,
            origin_source_config_id: origin.source_config_id,
        }) {
            continue;
        }
        exact_single_provider_candidate_events =
            exact_single_provider_candidate_events.saturating_add(1);
        let (path, required_inputs) = match (hit.critical, hit.lucky) {
            (true, true) => (
                "combined_lucky_occurrence_and_critical_bonus",
                vec![
                    (
                        CRITICAL_CHANCE_ATTRIBUTE_ID,
                        hit.chance_state.critical_chance_raw,
                    ),
                    (LUCKY_CHANCE_ATTRIBUTE_ID, hit.chance_state.lucky_chance_raw),
                    (
                        CRITICAL_DAMAGE_ATTRIBUTE_ID,
                        hit.chance_state.critical_damage_raw,
                    ),
                    (LUCKY_DAMAGE_ATTRIBUTE_ID, hit.chance_state.lucky_damage_raw),
                ],
            ),
            (true, false) => (
                "critical_proc_bonus",
                vec![
                    (
                        CRITICAL_CHANCE_ATTRIBUTE_ID,
                        hit.chance_state.critical_chance_raw,
                    ),
                    (
                        CRITICAL_DAMAGE_ATTRIBUTE_ID,
                        hit.chance_state.critical_damage_raw,
                    ),
                ],
            ),
            (false, true) => (
                "lucky_proc_occurrence",
                vec![(LUCKY_CHANCE_ATTRIBUTE_ID, hit.chance_state.lucky_chance_raw)],
            ),
            (false, false) => continue,
        };
        let path_coverage = paths.entry(path).or_default();
        path_coverage.candidate_events = path_coverage.candidate_events.saturating_add(1);
        let mut complete = true;
        let mut all_wire = true;
        let mut all_not_after = true;
        let mut oldest_age_sequences = 0_u64;
        let mut oldest_age_micros = 0_u64;
        for (attribute_id, observation) in required_inputs {
            let attribute_coverage = attributes.entry(attribute_id).or_default();
            attribute_coverage.required_events =
                attribute_coverage.required_events.saturating_add(1);
            let Some(observation) = observation else {
                complete = false;
                all_wire = false;
                all_not_after = false;
                attribute_coverage.missing_events =
                    attribute_coverage.missing_events.saturating_add(1);
                if missing_examples.len() < example_limit {
                    missing_examples.push(MissingFormulaInputExample {
                        path,
                        session_id: hit.session_id.clone(),
                        run_ordinal: hit.run_ordinal,
                        damage_sequence: hit.sequence,
                        attribute_id,
                    });
                }
                continue;
            };
            attribute_coverage.present_events = attribute_coverage.present_events.saturating_add(1);
            if observation.wire.is_some() {
                attribute_coverage.wire_provenance_events =
                    attribute_coverage.wire_provenance_events.saturating_add(1);
            } else {
                all_wire = false;
                attribute_coverage.non_wire_or_unknown_provenance_events = attribute_coverage
                    .non_wire_or_unknown_provenance_events
                    .saturating_add(1);
            }
            let Some(age_sequences) = hit.sequence.checked_sub(observation.sequence) else {
                all_not_after = false;
                attribute_coverage.observed_after_damage_events = attribute_coverage
                    .observed_after_damage_events
                    .saturating_add(1);
                continue;
            };
            let Some(age_micros) = hit.observed_micros.checked_sub(observation.observed_micros)
            else {
                all_not_after = false;
                attribute_coverage.observed_after_damage_events = attribute_coverage
                    .observed_after_damage_events
                    .saturating_add(1);
                continue;
            };
            attribute_coverage.observed_not_after_damage_events = attribute_coverage
                .observed_not_after_damage_events
                .saturating_add(1);
            attribute_coverage.maximum_age_sequences = Some(
                attribute_coverage
                    .maximum_age_sequences
                    .unwrap_or_default()
                    .max(age_sequences),
            );
            attribute_coverage.maximum_age_micros = Some(
                attribute_coverage
                    .maximum_age_micros
                    .unwrap_or_default()
                    .max(age_micros),
            );
            oldest_age_sequences = oldest_age_sequences.max(age_sequences);
            oldest_age_micros = oldest_age_micros.max(age_micros);
            let same_wire_as_damage =
                observation.wire.is_some() && observation.wire == hit.damage_wire;
            if same_wire_as_damage {
                attribute_coverage.same_wire_as_damage_events = attribute_coverage
                    .same_wire_as_damage_events
                    .saturating_add(1);
            }
            observed_examples.push(FormulaInputSnapshotExample {
                path,
                session_id: hit.session_id.clone(),
                run_ordinal: hit.run_ordinal,
                damage_sequence: hit.sequence,
                damage_observed_micros: hit.observed_micros,
                attribute_id,
                value: observation.value,
                attribute_sequence: observation.sequence,
                attribute_observed_micros: observation.observed_micros,
                age_sequences,
                age_micros,
                wire_provenance: observation.wire.is_some(),
                same_wire_as_damage,
            });
        }
        if complete {
            path_coverage.complete_input_sets = path_coverage.complete_input_sets.saturating_add(1);
        }
        if complete && all_wire {
            path_coverage.all_inputs_wire_provenance =
                path_coverage.all_inputs_wire_provenance.saturating_add(1);
        }
        if complete && all_not_after {
            path_coverage.all_inputs_observed_not_after_damage = path_coverage
                .all_inputs_observed_not_after_damage
                .saturating_add(1);
            path_coverage.maximum_oldest_input_age_sequences = Some(
                path_coverage
                    .maximum_oldest_input_age_sequences
                    .unwrap_or_default()
                    .max(oldest_age_sequences),
            );
            path_coverage.maximum_oldest_input_age_micros = Some(
                path_coverage
                    .maximum_oldest_input_age_micros
                    .unwrap_or_default()
                    .max(oldest_age_micros),
            );
        }
    }
    observed_examples.sort_by(|left, right| {
        right
            .age_micros
            .cmp(&left.age_micros)
            .then_with(|| right.age_sequences.cmp(&left.age_sequences))
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.damage_sequence.cmp(&right.damage_sequence))
            .then_with(|| left.attribute_id.cmp(&right.attribute_id))
    });
    observed_examples.truncate(example_limit);
    FormulaInputSnapshotCoverage {
        scope: "exact_single_locally_observed_player_provider_window_with_exact_instance_magnitude_before_formula_evaluation",
        exact_single_provider_candidate_events,
        paths: paths
            .into_iter()
            .map(|(path, coverage)| FormulaPathSnapshotCoverage {
                path,
                candidate_events: coverage.candidate_events,
                complete_input_sets: coverage.complete_input_sets,
                all_inputs_wire_provenance: coverage.all_inputs_wire_provenance,
                all_inputs_observed_not_after_damage: coverage.all_inputs_observed_not_after_damage,
                maximum_oldest_input_age_sequences: coverage.maximum_oldest_input_age_sequences,
                maximum_oldest_input_age_micros: coverage.maximum_oldest_input_age_micros,
            })
            .collect(),
        attributes: attributes
            .into_iter()
            .map(|(attribute_id, coverage)| AttributeSnapshotCoverage {
                attribute_id,
                required_events: coverage.required_events,
                present_events: coverage.present_events,
                missing_events: coverage.missing_events,
                wire_provenance_events: coverage.wire_provenance_events,
                non_wire_or_unknown_provenance_events: coverage
                    .non_wire_or_unknown_provenance_events,
                observed_not_after_damage_events: coverage.observed_not_after_damage_events,
                observed_after_damage_events: coverage.observed_after_damage_events,
                same_wire_as_damage_events: coverage.same_wire_as_damage_events,
                maximum_age_sequences: coverage.maximum_age_sequences,
                maximum_age_micros: coverage.maximum_age_micros,
            })
            .collect(),
        oldest_observed_examples: observed_examples,
        missing_examples,
        event_time_snapshot_authority: false,
    }
}

fn exact_stage_input_freshness(
    hit: &CandidateHit,
    observations: &[Option<ObservedAttribute>],
) -> StageInputFreshness {
    if observations.iter().any(Option::is_none) {
        return StageInputFreshness {
            observation_state: "missing_required_stage_input",
            oldest_age_sequences: None,
            oldest_age_micros: None,
            all_wire_provenance: false,
            all_same_wire_as_damage: false,
        };
    }
    let mut oldest_age_sequences = 0_u64;
    let mut oldest_age_micros = 0_u64;
    let mut all_wire_provenance = true;
    let mut all_same_wire_as_damage = true;
    for observation in observations.iter().flatten() {
        all_wire_provenance &= observation.wire.is_some();
        all_same_wire_as_damage &=
            observation.wire.is_some() && observation.wire == hit.damage_wire;
        let (Some(age_sequences), Some(age_micros)) = (
            hit.sequence.checked_sub(observation.sequence),
            hit.observed_micros.checked_sub(observation.observed_micros),
        ) else {
            return StageInputFreshness {
                observation_state: "observed_after_damage",
                oldest_age_sequences: None,
                oldest_age_micros: None,
                all_wire_provenance,
                all_same_wire_as_damage,
            };
        };
        oldest_age_sequences = oldest_age_sequences.max(age_sequences);
        oldest_age_micros = oldest_age_micros.max(age_micros);
    }
    StageInputFreshness {
        observation_state: "complete_not_after_damage",
        oldest_age_sequences: Some(oldest_age_sequences),
        oldest_age_micros: Some(oldest_age_micros),
        all_wire_provenance,
        all_same_wire_as_damage,
    }
}

fn stage_input_age_bucket(observation_state: &str, oldest_age_micros: Option<u64>) -> &'static str {
    if observation_state != "complete_not_after_damage" {
        return match observation_state {
            "missing_required_stage_input" => "missing",
            "observed_after_damage" => "observed_after_damage",
            _ => "invalid_observation_state",
        };
    }
    match oldest_age_micros {
        Some(0) => "same_observed_micros",
        Some(1..=100_000) => "1us_to_100ms",
        Some(100_001..=500_000) => "100ms_to_500ms",
        Some(500_001..=1_000_000) => "500ms_to_1s",
        Some(1_000_001..=5_000_000) => "1s_to_5s",
        Some(_) => "over_5s",
        None => "missing_age",
    }
}

fn converged_stage_counterfactual<'a>(
    candidates: impl Iterator<Item = &'a IntegerStageCandidate>,
) -> Option<i64> {
    let mut exact = None::<i64>;
    let mut seen = false;
    for candidate in candidates {
        seen = true;
        let (Some(minimum), Some(maximum)) =
            (candidate.counterfactual_min, candidate.counterfactual_max)
        else {
            return None;
        };
        if minimum != maximum || exact.is_some_and(|value| value != minimum) {
            return None;
        }
        exact = Some(minimum);
    }
    seen.then_some(exact).flatten()
}

fn integer_stage_counterfactual_coverage(
    candidates: &[CandidateHit],
    magnitudes: &BTreeMap<MagnitudeKey, ProvenMagnitude>,
    players: &HashSet<(u32, u32, i64)>,
    damage_surface: &DamageFormulaSurface,
    example_limit: usize,
) -> IntegerStageCounterfactualCoverage {
    let mut paths = BTreeMap::<&'static str, IntegerStagePathCoverage>::from([
        (
            "critical_proc_bonus",
            IntegerStagePathCoverage {
                path: "critical_proc_bonus",
                ..IntegerStagePathCoverage::default()
            },
        ),
        (
            "combined_lucky_occurrence_and_critical_bonus",
            IntegerStagePathCoverage {
                path: "combined_lucky_occurrence_and_critical_bonus",
                ..IntegerStagePathCoverage::default()
            },
        ),
    ]);
    let mut exact_single_provider_candidate_events = 0_u64;
    let mut lucky_only_events_without_critical_stage = 0_u64;
    let mut critical_stage_events = 0_u64;
    let mut events_with_complete_stage_inputs = 0_u64;
    let mut events_without_complete_stage_inputs = 0_u64;
    let mut events_with_at_least_one_compatible_candidate = 0_u64;
    let mut events_without_compatible_candidates = 0_u64;
    let mut exact_stage_independent_events = 0_u64;
    let mut unresolved_stage_or_rounding_events = 0_u64;
    let mut exact_examples = Vec::new();
    let mut unresolved_examples = Vec::new();
    let mut critical_factor_event_records = Vec::new();
    let mut identity_groups =
        BTreeMap::<IntegerStageIdentityKey, IntegerStageIdentityAccumulator>::new();
    let mut critical_factor_interpretation_breakdown =
        BTreeMap::<(&'static str, &'static str, &'static str), u64>::new();

    for hit in candidates {
        let player_windows = hit
            .active_windows
            .iter()
            .filter(|window| {
                players.contains(&(
                    window.key.session_ordinal,
                    window.key.run_ordinal,
                    window.key.provider_entity_uuid,
                ))
            })
            .collect::<Vec<_>>();
        if player_windows.len() != 1 {
            continue;
        }
        let window = player_windows[0];
        let (Some(level), Some(origin)) = (window.level, window.origin) else {
            continue;
        };
        let Some(magnitude) = magnitudes.get(&MagnitudeKey {
            session_ordinal: window.key.session_ordinal,
            run_ordinal: window.key.run_ordinal,
            target_entity_uuid: window.key.target_entity_uuid,
            provider_entity_uuid: window.key.provider_entity_uuid,
            instance_id: window.key.instance_id,
            level,
            origin_source_type_id: origin.source_type_id,
            origin_source_config_id: origin.source_config_id,
        }) else {
            continue;
        };
        exact_single_provider_candidate_events =
            exact_single_provider_candidate_events.saturating_add(1);
        if hit.lucky && !hit.critical {
            lucky_only_events_without_critical_stage =
                lucky_only_events_without_critical_stage.saturating_add(1);
            continue;
        }
        if !hit.critical {
            continue;
        }

        critical_stage_events = critical_stage_events.saturating_add(1);
        let path = if hit.lucky {
            "combined_lucky_occurrence_and_critical_bonus"
        } else {
            "critical_proc_bonus"
        };
        let path_coverage = paths.get_mut(path).expect("known stage path");
        path_coverage.events = path_coverage.events.saturating_add(1);
        let critical_damage_raw = hit
            .chance_state
            .critical_damage_raw
            .map(|observation| observation.value);
        let lucky_damage_raw = hit
            .chance_state
            .lucky_damage_raw
            .map(|observation| observation.value);
        let stage_input_observations = if hit.lucky {
            vec![
                hit.chance_state.critical_damage_raw,
                hit.chance_state.lucky_damage_raw,
            ]
        } else {
            vec![hit.chance_state.critical_damage_raw]
        };
        let stage_input_freshness = exact_stage_input_freshness(hit, &stage_input_observations);
        let complete_inputs =
            critical_damage_raw.is_some() && (!hit.lucky || lucky_damage_raw.is_some());
        let stage_candidates = if let Some(critical_damage_raw) = critical_damage_raw {
            if hit.lucky {
                lucky_damage_raw.map_or_else(Vec::new, |lucky_damage_raw| {
                    combined_critical_bonus_stage_candidates(
                        hit.amount,
                        critical_damage_raw,
                        lucky_damage_raw,
                    )
                })
            } else {
                critical_bonus_stage_candidates(hit.amount, critical_damage_raw)
            }
        } else {
            Vec::new()
        };
        critical_factor_event_records.push(critical_factor_event_record(
            hit,
            window,
            level,
            origin,
            *magnitude,
            path,
            &stage_candidates,
            damage_surface,
        ));

        if complete_inputs {
            events_with_complete_stage_inputs = events_with_complete_stage_inputs.saturating_add(1);
            path_coverage.complete_stage_inputs =
                path_coverage.complete_stage_inputs.saturating_add(1);
        } else {
            events_without_complete_stage_inputs =
                events_without_complete_stage_inputs.saturating_add(1);
        }
        let compatible = stage_candidates
            .iter()
            .filter(|candidate| candidate.compatible_with_observed_damage)
            .collect::<Vec<_>>();
        let additive_compatible = compatible
            .iter()
            .copied()
            .filter(|candidate| candidate.critical_factor_interpretation == "additive_bonus")
            .collect::<Vec<_>>();
        let direct_compatible = compatible
            .iter()
            .copied()
            .filter(|candidate| candidate.critical_factor_interpretation == "direct_total")
            .collect::<Vec<_>>();
        let additive_counterfactual =
            converged_stage_counterfactual(additive_compatible.iter().copied());
        let direct_counterfactual =
            converged_stage_counterfactual(direct_compatible.iter().copied());
        let interpretation_compatibility =
            match (additive_compatible.is_empty(), direct_compatible.is_empty()) {
                (false, false) => "both",
                (false, true) => "additive_only",
                (true, false) => "direct_only",
                (true, true) => "neither",
            };
        let counterfactual_relation = match (
            additive_counterfactual,
            direct_counterfactual,
            interpretation_compatibility,
        ) {
            (Some(additive), Some(direct), "both") if additive == direct => "same_exact",
            (Some(_), Some(_), "both") => "divergent_exact",
            (Some(_), None, "additive_only") | (None, Some(_), "direct_only") => {
                "single_interpretation_exact"
            }
            (None, None, "neither") => "no_compatible_interpretation",
            _ => "within_interpretation_unresolved",
        };
        let interpretation_count = critical_factor_interpretation_breakdown
            .entry((path, interpretation_compatibility, counterfactual_relation))
            .or_default();
        *interpretation_count = interpretation_count.saturating_add(1);
        if compatible.is_empty() {
            events_without_compatible_candidates =
                events_without_compatible_candidates.saturating_add(1);
        } else {
            events_with_at_least_one_compatible_candidate =
                events_with_at_least_one_compatible_candidate.saturating_add(1);
            path_coverage.compatible_candidate_events =
                path_coverage.compatible_candidate_events.saturating_add(1);
        }
        let exact_noncritical_counterfactual =
            converged_stage_counterfactual(compatible.iter().copied());
        let exact_critical_bonus = exact_noncritical_counterfactual
            .and_then(|counterfactual| hit.amount.checked_sub(counterfactual))
            .filter(|bonus| *bonus >= 0 && *bonus <= hit.amount);
        if exact_critical_bonus.is_some() {
            exact_stage_independent_events = exact_stage_independent_events.saturating_add(1);
            path_coverage.exact_stage_independent_events = path_coverage
                .exact_stage_independent_events
                .saturating_add(1);
        } else if !compatible.is_empty() {
            unresolved_stage_or_rounding_events =
                unresolved_stage_or_rounding_events.saturating_add(1);
            path_coverage.unresolved_stage_or_rounding_events = path_coverage
                .unresolved_stage_or_rounding_events
                .saturating_add(1);
        }
        let identity = IntegerStageIdentityKey {
            path,
            ability_id: hit.ability_id,
            hit_event_id: hit.hit_event_id,
            damage_source: hit.damage_source,
            damage_type: hit.damage_type,
            type_flags: hit.type_flags,
            reported_critical: hit.reported_critical,
            owner_level: hit.owner_level,
            owner_stage: hit.owner_stage,
            normal_hit: hit.normal_hit,
            property: hit.property,
            passive_uuid: hit.passive_uuid,
            rainbow: hit.rainbow,
            damage_mode: hit.damage_mode,
            skill_effect_uuid: hit.skill_effect_uuid,
            skill_effect_group_index: hit.skill_effect_group_index,
            skill_effect_component_index: hit.skill_effect_component_index,
            skill_effect_component_count: hit.skill_effect_component_count,
            critical_damage_raw,
            lucky_damage_raw,
            stage_input_observation_state: stage_input_freshness.observation_state,
            oldest_stage_input_age_sequences: stage_input_freshness.oldest_age_sequences,
            oldest_stage_input_age_micros: stage_input_freshness.oldest_age_micros,
            stage_inputs_all_wire_provenance: stage_input_freshness.all_wire_provenance,
            stage_inputs_all_same_wire_as_damage: stage_input_freshness.all_same_wire_as_damage,
        };
        let identity_group = identity_groups.entry(identity).or_default();
        identity_group.events = identity_group.events.saturating_add(1);
        if complete_inputs {
            identity_group.complete_stage_inputs =
                identity_group.complete_stage_inputs.saturating_add(1);
        }
        if compatible.is_empty() {
            identity_group.events_without_compatible_candidates = identity_group
                .events_without_compatible_candidates
                .saturating_add(1);
        } else {
            identity_group.compatible_candidate_events =
                identity_group.compatible_candidate_events.saturating_add(1);
        }
        if exact_critical_bonus.is_some() {
            identity_group.exact_stage_independent_events = identity_group
                .exact_stage_independent_events
                .saturating_add(1);
        } else if !compatible.is_empty() {
            identity_group.unresolved_stage_or_rounding_events = identity_group
                .unresolved_stage_or_rounding_events
                .saturating_add(1);
        }
        identity_group.observed_damage_sum = identity_group
            .observed_damage_sum
            .checked_add(i128::from(hit.amount))
            .expect("bounded exact-subset damage sum");
        if let Some(value) = critical_damage_raw {
            identity_group.critical_damage_raw_values.insert(value);
        }
        if let Some(value) = lucky_damage_raw.filter(|_| hit.lucky) {
            identity_group.lucky_damage_raw_values.insert(value);
        }
        let example = IntegerStageCounterfactualExample {
            path,
            session_id: hit.session_id.clone(),
            run_ordinal: hit.run_ordinal,
            damage_sequence: hit.sequence,
            source_entity_uuid: hit.source_entity_uuid,
            target_entity_uuid: hit.target_entity_uuid,
            ability_id: hit.ability_id,
            observed_damage: hit.amount,
            normal_value: hit.normal_value,
            lucky_value: hit.lucky_value,
            reported_critical: hit.reported_critical,
            type_flags: hit.type_flags,
            hit_event_id: hit.hit_event_id,
            damage_source: hit.damage_source,
            damage_type: hit.damage_type,
            owner_level: hit.owner_level,
            owner_stage: hit.owner_stage,
            normal_hit: hit.normal_hit,
            property: hit.property,
            passive_uuid: hit.passive_uuid,
            rainbow: hit.rainbow,
            damage_mode: hit.damage_mode,
            skill_effect_uuid: hit.skill_effect_uuid,
            skill_effect_group_index: hit.skill_effect_group_index,
            skill_effect_component_index: hit.skill_effect_component_index,
            skill_effect_component_count: hit.skill_effect_component_count,
            critical_damage_raw,
            lucky_damage_raw: hit.lucky.then_some(lucky_damage_raw).flatten(),
            candidates: stage_candidates,
            exact_noncritical_counterfactual,
            exact_critical_bonus,
        };
        if exact_critical_bonus.is_some() {
            push_prioritized_stage_example(&mut exact_examples, example, example_limit, true);
        } else {
            push_prioritized_stage_example(&mut unresolved_examples, example, example_limit, false);
        }
    }

    let damage_surface_join = integer_stage_damage_surface_join(identity_groups, damage_surface);
    let critical_factor_interpretation_breakdown = critical_factor_interpretation_breakdown
        .into_iter()
        .map(|((path, compatibility, counterfactual_relation), events)| {
            CriticalFactorInterpretationBreakdown {
                path,
                compatibility,
                counterfactual_relation,
                events,
                formula_authority: false,
            }
        })
        .collect();
    IntegerStageCounterfactualCoverage {
        scope: "exact_single_locally_observed_player_provider_window_with_exact_instance_magnitude; enumerate positive-integer latent bases for critical-only and critical-plus-lucky packet rows under additive-bonus and direct-total AttrCritDamage interpretations without requiring remote cast packets",
        candidate_family: vec![
            "critical-only additive-bonus fixed-point stage under floor and nearest-half-up",
            "critical-only direct-total fixed-point stage under floor and nearest-half-up",
            "critical-plus-lucky additive-bonus nested stages in both orders under every floor/nearest-half-up combination",
            "critical-plus-lucky direct-total nested stages in both orders under every floor/nearest-half-up combination",
            "critical-plus-lucky additive-bonus single-product stage under floor and nearest-half-up",
            "critical-plus-lucky direct-total single-product stage under floor and nearest-half-up",
        ],
        exact_single_provider_candidate_events,
        lucky_only_events_without_critical_stage,
        critical_stage_events,
        events_with_complete_stage_inputs,
        events_without_complete_stage_inputs,
        events_with_at_least_one_compatible_candidate,
        events_without_compatible_candidates,
        exact_stage_independent_events,
        unresolved_stage_or_rounding_events,
        paths: paths.into_values().collect(),
        exact_examples,
        unresolved_examples,
        critical_factor_event_records,
        damage_surface_join,
        critical_factor_interpretation_breakdown,
        critical_factor_interpretation_breakdown_authority: false,
        candidate_family_authority: false,
        counterfactual_authority: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn critical_factor_event_record(
    hit: &CandidateHit,
    window: &WindowSnapshot,
    level: i32,
    origin: StatusOrigin,
    magnitude: ProvenMagnitude,
    path: &'static str,
    stage_candidates: &[IntegerStageCandidate],
    damage_surface: &DamageFormulaSurface,
) -> CriticalFactorEventRecord {
    let rows = hit
        .ability_id
        .zip(hit.hit_event_id)
        .and_then(|lookup| damage_surface.rows_by_key.get(&lookup))
        .map(Vec::as_slice)
        .unwrap_or_default();
    let damage_surface_resolution = match rows.len() {
        0 => "no_exact_build_surface_row",
        1 => "exactly_one_exact_build_surface_row",
        _ => "ambiguous_exact_build_surface_rows",
    };
    CriticalFactorEventRecord {
        protocol_pack_digest: hit.protocol_pack_digest.clone(),
        session_id: hit.session_id.clone(),
        run_ordinal: hit.run_ordinal,
        damage_sequence: hit.sequence,
        damage_observed_micros: hit.observed_micros,
        source_entity_uuid: hit.source_entity_uuid,
        target_entity_uuid: hit.target_entity_uuid,
        ability_id: hit.ability_id,
        hit_event_id: hit.hit_event_id,
        damage_source: hit.damage_source,
        damage_type: hit.damage_type,
        type_flags: hit.type_flags,
        reported_critical: hit.reported_critical,
        owner_level: hit.owner_level,
        owner_stage: hit.owner_stage,
        normal_hit: hit.normal_hit,
        property: hit.property,
        passive_uuid: hit.passive_uuid,
        rainbow: hit.rainbow,
        damage_mode: hit.damage_mode,
        skill_effect_uuid: hit.skill_effect_uuid,
        skill_effect_group_index: hit.skill_effect_group_index,
        skill_effect_component_index: hit.skill_effect_component_index,
        skill_effect_component_count: hit.skill_effect_component_count,
        observed_damage: hit.amount,
        normal_value: hit.normal_value,
        lucky_value: hit.lucky_value,
        path,
        critical_damage: critical_factor_input_observation(
            hit,
            hit.chance_state.critical_damage_raw,
        ),
        lucky_damage: hit
            .lucky
            .then(|| critical_factor_input_observation(hit, hit.chance_state.lucky_damage_raw))
            .flatten(),
        provider_entity_uuid: window.key.provider_entity_uuid,
        provider_instance_id: window.key.instance_id,
        provider_level: level,
        provider_origin_source_type_id: origin.source_type_id,
        provider_origin_source_config_id: origin.source_config_id,
        provider_critical_raw_delta: magnitude.critical_raw_delta,
        provider_lucky_raw_delta: magnitude.lucky_raw_delta,
        damage_surface_resolution,
        damage_surface_candidates: rows
            .iter()
            .map(|row| damage_surface_candidate(row, hit.owner_stage))
            .collect(),
        candidate_arithmetic: stage_candidates.to_vec(),
        // The current canonical stream does not yet identify which player
        // entity is controlled by this client. A last-observed remote actor
        // attribute therefore cannot be promoted into hit-time authority.
        event_time_local_state_authority: false,
        // The packet row does not retain complete attack and mitigation
        // preimages. Keep that absence explicit instead of treating an equal
        // damage surface or latent-base overlap as a controlled experiment.
        attack_preimage_complete: false,
        mitigation_preimage_complete: false,
        formula_authority: false,
    }
}

fn critical_factor_input_observation(
    hit: &CandidateHit,
    observation: Option<ObservedAttribute>,
) -> Option<CriticalFactorInputObservation> {
    let observation = observation?;
    Some(CriticalFactorInputObservation {
        value: observation.value,
        attribute_sequence: observation.sequence,
        attribute_observed_micros: observation.observed_micros,
        age_sequences: hit.sequence.checked_sub(observation.sequence),
        age_micros: hit.observed_micros.checked_sub(observation.observed_micros),
        wire_provenance: observation.wire.is_some(),
        same_wire_as_damage: observation.wire.is_some() && observation.wire == hit.damage_wire,
    })
}

fn push_prioritized_stage_example(
    examples: &mut Vec<IntegerStageCounterfactualExample>,
    example: IntegerStageCounterfactualExample,
    limit: usize,
    exact: bool,
) {
    if limit == 0 {
        return;
    }
    examples.push(example);
    examples.sort_by(|left, right| {
        integer_stage_example_priority(left, exact)
            .cmp(&integer_stage_example_priority(right, exact))
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.run_ordinal.cmp(&right.run_ordinal))
            .then_with(|| left.damage_sequence.cmp(&right.damage_sequence))
    });
    examples.truncate(limit);
}

fn integer_stage_example_priority(example: &IntegerStageCounterfactualExample, exact: bool) -> u8 {
    let combined = example.path == "combined_lucky_occurrence_and_critical_bonus";
    let has_compatible_candidate = example
        .candidates
        .iter()
        .any(|candidate| candidate.compatible_with_observed_damage);
    if exact {
        return u8::from(!combined);
    }
    match (combined, has_compatible_candidate) {
        (true, true) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (false, false) => 3,
    }
}

fn integer_stage_damage_surface_join(
    identity_groups: BTreeMap<IntegerStageIdentityKey, IntegerStageIdentityAccumulator>,
    damage_surface: &DamageFormulaSurface,
) -> IntegerStageDamageSurfaceJoin {
    let mut events = 0_u64;
    let mut events_with_exactly_one_surface_row = 0_u64;
    let mut events_with_ambiguous_surface_rows = 0_u64;
    let mut events_without_surface_row = 0_u64;
    let mut events_with_resolved_damage_script = 0_u64;
    let mut events_without_resolved_damage_script = 0_u64;
    let mut events_with_unique_ability_surface_candidate_when_hit_event_absent = 0_u64;
    let mut
    events_with_unique_ability_surface_candidate_and_resolved_damage_script_when_hit_event_absent =
        0_u64;
    let mut events_without_exact_or_unique_ability_surface_candidate = 0_u64;
    let mut damage_script_preimage_breakdown =
        BTreeMap::<(&'static str, String), DamageScriptPreimageAccumulator>::new();
    let mut stage_input_freshness_breakdown = BTreeMap::<
        (&'static str, &'static str, &'static str, bool, bool),
        DamageScriptPreimageAccumulator,
    >::new();
    let groups = identity_groups
        .into_iter()
        .map(|(key, accumulator)| {
            events = events.saturating_add(accumulator.events);
            let rows = key
                .ability_id
                .zip(key.hit_event_id)
                .and_then(|lookup| damage_surface.rows_by_key.get(&lookup))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let damage_surface_resolution = match rows.len() {
                0 => {
                    events_without_surface_row =
                        events_without_surface_row.saturating_add(accumulator.events);
                    "no_exact_build_surface_row"
                }
                1 => {
                    events_with_exactly_one_surface_row =
                        events_with_exactly_one_surface_row.saturating_add(accumulator.events);
                    "exactly_one_exact_build_surface_row"
                }
                _ => {
                    events_with_ambiguous_surface_rows =
                        events_with_ambiguous_surface_rows.saturating_add(accumulator.events);
                    "ambiguous_exact_build_surface_rows"
                }
            };
            if rows.len() == 1 && rows[0].damage_script.is_some() {
                events_with_resolved_damage_script =
                    events_with_resolved_damage_script.saturating_add(accumulator.events);
            } else {
                events_without_resolved_damage_script =
                    events_without_resolved_damage_script.saturating_add(accumulator.events);
            }
            let unique_ability_rows = if rows.is_empty() && key.hit_event_id.is_none() {
                key.ability_id
                    .and_then(|ability_id| damage_surface.rows_by_ability.get(&ability_id))
                    .map(Vec::as_slice)
                    .unwrap_or_default()
            } else {
                &[]
            };
            let unique_ability_damage_surface_resolution = if !rows.is_empty() {
                "not_applicable_exact_hit_event_surface_present"
            } else if key.hit_event_id.is_some() {
                events_without_exact_or_unique_ability_surface_candidate =
                    events_without_exact_or_unique_ability_surface_candidate
                        .saturating_add(accumulator.events);
                "hit_event_present_no_exact_surface_row"
            } else {
                match unique_ability_rows.len() {
                    0 => {
                        events_without_exact_or_unique_ability_surface_candidate =
                            events_without_exact_or_unique_ability_surface_candidate
                                .saturating_add(accumulator.events);
                        "hit_event_absent_no_ability_surface_candidate"
                    }
                    1 => {
                        events_with_unique_ability_surface_candidate_when_hit_event_absent =
                            events_with_unique_ability_surface_candidate_when_hit_event_absent
                                .saturating_add(accumulator.events);
                        if unique_ability_rows[0].damage_script.is_some() {
                            events_with_unique_ability_surface_candidate_and_resolved_damage_script_when_hit_event_absent =
                                events_with_unique_ability_surface_candidate_and_resolved_damage_script_when_hit_event_absent
                                    .saturating_add(accumulator.events);
                        }
                        "hit_event_absent_one_unique_ability_surface_candidate_diagnostic"
                    }
                    _ => {
                        events_without_exact_or_unique_ability_surface_candidate =
                            events_without_exact_or_unique_ability_surface_candidate
                                .saturating_add(accumulator.events);
                        "hit_event_absent_ambiguous_ability_surface_candidates"
                    }
                }
            };
            let (surface_binding, damage_script) = if rows.len() == 1 {
                (
                    "exact_hit_event",
                    rows[0]
                        .damage_script
                        .clone()
                        .unwrap_or_else(|| "<missing_damage_script>".to_owned()),
                )
            } else if rows.len() > 1 {
                ("ambiguous_exact_hit_event", "<ambiguous_damage_script>".to_owned())
            } else if unique_ability_rows.len() == 1 {
                (
                    "ability_only_diagnostic",
                    unique_ability_rows[0]
                        .damage_script
                        .clone()
                        .unwrap_or_else(|| "<missing_damage_script>".to_owned()),
                )
            } else {
                (
                    "unresolved",
                    if unique_ability_rows.len() > 1 {
                        "<ambiguous_damage_script>"
                    } else {
                        "<missing_damage_script>"
                    }
                    .to_owned(),
                )
            };
            let script_breakdown = damage_script_preimage_breakdown
                .entry((surface_binding, damage_script))
                .or_default();
            script_breakdown.identity_groups = script_breakdown.identity_groups.saturating_add(1);
            script_breakdown.events = script_breakdown.events.saturating_add(accumulator.events);
            script_breakdown.complete_stage_inputs = script_breakdown
                .complete_stage_inputs
                .saturating_add(accumulator.complete_stage_inputs);
            script_breakdown.compatible_candidate_events = script_breakdown
                .compatible_candidate_events
                .saturating_add(accumulator.compatible_candidate_events);
            script_breakdown.exact_stage_independent_events = script_breakdown
                .exact_stage_independent_events
                .saturating_add(accumulator.exact_stage_independent_events);
            script_breakdown.unresolved_stage_or_rounding_events = script_breakdown
                .unresolved_stage_or_rounding_events
                .saturating_add(accumulator.unresolved_stage_or_rounding_events);
            script_breakdown.events_without_compatible_candidates = script_breakdown
                .events_without_compatible_candidates
                .saturating_add(accumulator.events_without_compatible_candidates);
            let stage_age_bucket = stage_input_age_bucket(
                key.stage_input_observation_state,
                key.oldest_stage_input_age_micros,
            );
            let freshness_breakdown = stage_input_freshness_breakdown
                .entry((
                    key.path,
                    key.stage_input_observation_state,
                    stage_age_bucket,
                    key.stage_inputs_all_wire_provenance,
                    key.stage_inputs_all_same_wire_as_damage,
                ))
                .or_default();
            freshness_breakdown.identity_groups =
                freshness_breakdown.identity_groups.saturating_add(1);
            freshness_breakdown.events = freshness_breakdown
                .events
                .saturating_add(accumulator.events);
            freshness_breakdown.complete_stage_inputs = freshness_breakdown
                .complete_stage_inputs
                .saturating_add(accumulator.complete_stage_inputs);
            freshness_breakdown.compatible_candidate_events = freshness_breakdown
                .compatible_candidate_events
                .saturating_add(accumulator.compatible_candidate_events);
            freshness_breakdown.exact_stage_independent_events = freshness_breakdown
                .exact_stage_independent_events
                .saturating_add(accumulator.exact_stage_independent_events);
            freshness_breakdown.unresolved_stage_or_rounding_events = freshness_breakdown
                .unresolved_stage_or_rounding_events
                .saturating_add(accumulator.unresolved_stage_or_rounding_events);
            freshness_breakdown.events_without_compatible_candidates = freshness_breakdown
                .events_without_compatible_candidates
                .saturating_add(accumulator.events_without_compatible_candidates);
            let damage_surface_candidates = rows
                .iter()
                .map(|row| damage_surface_candidate(row, key.owner_stage))
                .collect();
            let unique_ability_damage_surface_candidates = unique_ability_rows
                .iter()
                .map(|row| damage_surface_candidate(row, key.owner_stage))
                .collect();
            IntegerStageIdentityGroup {
                path: key.path,
                ability_id: key.ability_id,
                hit_event_id: key.hit_event_id,
                damage_source: key.damage_source,
                damage_type: key.damage_type,
                type_flags: key.type_flags,
                reported_critical: key.reported_critical,
                owner_level: key.owner_level,
                owner_stage: key.owner_stage,
                normal_hit: key.normal_hit,
                property: key.property,
                passive_uuid: key.passive_uuid,
                rainbow: key.rainbow,
                damage_mode: key.damage_mode,
                skill_effect_uuid: key.skill_effect_uuid,
                skill_effect_group_index: key.skill_effect_group_index,
                skill_effect_component_index: key.skill_effect_component_index,
                skill_effect_component_count: key.skill_effect_component_count,
                events: accumulator.events,
                complete_stage_inputs: accumulator.complete_stage_inputs,
                compatible_candidate_events: accumulator.compatible_candidate_events,
                exact_stage_independent_events: accumulator.exact_stage_independent_events,
                unresolved_stage_or_rounding_events: accumulator
                    .unresolved_stage_or_rounding_events,
                events_without_compatible_candidates: accumulator
                    .events_without_compatible_candidates,
                observed_damage_sum: accumulator.observed_damage_sum.to_string(),
                critical_damage_raw_values: accumulator
                    .critical_damage_raw_values
                    .into_iter()
                    .collect(),
                lucky_damage_raw_values: accumulator.lucky_damage_raw_values.into_iter().collect(),
                stage_input_observation_state: key.stage_input_observation_state,
                oldest_stage_input_age_sequences: key.oldest_stage_input_age_sequences,
                oldest_stage_input_age_micros: key.oldest_stage_input_age_micros,
                stage_inputs_all_wire_provenance: key.stage_inputs_all_wire_provenance,
                stage_inputs_all_same_wire_as_damage: key.stage_inputs_all_same_wire_as_damage,
                damage_surface_resolution,
                damage_surface_candidates,
                unique_ability_damage_surface_resolution,
                unique_ability_damage_surface_candidates,
            }
        })
        .collect::<Vec<_>>();
    let damage_script_preimage_breakdown = damage_script_preimage_breakdown
        .into_iter()
        .map(
            |((surface_binding, damage_script), accumulator)| DamageScriptPreimageBreakdown {
                surface_binding,
                damage_script,
                identity_groups: accumulator.identity_groups,
                events: accumulator.events,
                complete_stage_inputs: accumulator.complete_stage_inputs,
                compatible_candidate_events: accumulator.compatible_candidate_events,
                exact_stage_independent_events: accumulator.exact_stage_independent_events,
                unresolved_stage_or_rounding_events: accumulator
                    .unresolved_stage_or_rounding_events,
                events_without_compatible_candidates: accumulator
                    .events_without_compatible_candidates,
                formula_authority: false,
            },
        )
        .collect();
    let stage_input_freshness_breakdown = stage_input_freshness_breakdown
        .into_iter()
        .map(
            |(
                (
                    path,
                    observation_state,
                    oldest_age_bucket,
                    all_wire_provenance,
                    all_same_wire_as_damage,
                ),
                accumulator,
            )| StageInputFreshnessBreakdown {
                path,
                observation_state,
                oldest_age_bucket,
                all_wire_provenance,
                all_same_wire_as_damage,
                identity_groups: accumulator.identity_groups,
                events: accumulator.events,
                complete_stage_inputs: accumulator.complete_stage_inputs,
                compatible_candidate_events: accumulator.compatible_candidate_events,
                exact_stage_independent_events: accumulator.exact_stage_independent_events,
                unresolved_stage_or_rounding_events: accumulator
                    .unresolved_stage_or_rounding_events,
                events_without_compatible_candidates: accumulator
                    .events_without_compatible_candidates,
                formula_authority: false,
            },
        )
        .collect();
    IntegerStageDamageSurfaceJoin {
        surface_runtime_formula_authority: false,
        identity_groups: groups.len(),
        events,
        events_with_exactly_one_surface_row,
        events_with_ambiguous_surface_rows,
        events_without_surface_row,
        events_with_resolved_damage_script,
        events_without_resolved_damage_script,
        events_with_unique_ability_surface_candidate_when_hit_event_absent,
        events_with_unique_ability_surface_candidate_and_resolved_damage_script_when_hit_event_absent,
        events_without_exact_or_unique_ability_surface_candidate,
        unique_ability_surface_candidate_authority: false,
        damage_script_preimage_breakdown_authority: false,
        damage_script_preimage_breakdown,
        stage_input_freshness_breakdown_authority: false,
        stage_input_freshness_breakdown,
        groups,
    }
}

fn damage_surface_candidate(
    row: &DamageFormulaSurfaceRow,
    owner_stage: Option<i32>,
) -> IntegerStageDamageSurfaceCandidate {
    IntegerStageDamageSurfaceCandidate {
        damage_id: row.damage_id.clone(),
        damage_script: row.damage_script.clone(),
        pve_damage_ratio: row.pve_damage_ratio.clone(),
        pve_fixed_parameter: row.pve_fixed_parameter.clone(),
        selected_pve_damage_ratio: select_owner_stage_value(&row.pve_damage_ratio, owner_stage),
        selected_pve_fixed_parameter: select_owner_stage_value(
            &row.pve_fixed_parameter,
            owner_stage,
        ),
        owner_stage_selection_authority: false,
    }
}

fn select_owner_stage_value(values: &[i64], owner_stage: Option<i32>) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    if values.len() == 1 {
        return values.first().copied();
    }
    let owner_stage = owner_stage?;
    if owner_stage < 0 {
        return None;
    }
    values.get(usize::try_from(owner_stage).ok()?).copied()
}

fn critical_bonus_stage_candidates(
    observed_damage: i64,
    critical_damage_raw: i64,
) -> Vec<IntegerStageCandidate> {
    let mut candidates = Vec::new();
    if let Some(current_factor) = BPSR_FIXED_POINT_SCALE.checked_add(critical_damage_raw) {
        candidates.extend(StageRounding::ALL.into_iter().map(|rounding| {
            single_stage_candidate(
                "additive_bonus",
                "round(base * (10000 + critical_damage_raw) / 10000)",
                observed_damage,
                current_factor,
                BPSR_FIXED_POINT_SCALE,
                rounding,
            )
        }));
    }
    candidates.extend(StageRounding::ALL.into_iter().map(|rounding| {
        single_stage_candidate(
            "direct_total",
            "round(base * critical_damage_raw / 10000)",
            observed_damage,
            critical_damage_raw,
            BPSR_FIXED_POINT_SCALE,
            rounding,
        )
    }));
    candidates
}

fn combined_critical_bonus_stage_candidates(
    observed_damage: i64,
    critical_damage_raw: i64,
    lucky_damage_raw: i64,
) -> Vec<IntegerStageCandidate> {
    let mut candidates = Vec::new();
    let mut interpretations = Vec::new();
    if let Some(critical_factor) = BPSR_FIXED_POINT_SCALE.checked_add(critical_damage_raw) {
        interpretations.push((
            "additive_bonus",
            critical_factor,
            "round_second(round_first(base * lucky_damage_raw / 10000) * (10000 + critical_damage_raw) / 10000)",
            "round_second(round_first(base * (10000 + critical_damage_raw) / 10000) * lucky_damage_raw / 10000)",
            "round(base * lucky_damage_raw * (10000 + critical_damage_raw) / 100000000)",
        ));
    }
    interpretations.push((
        "direct_total",
        critical_damage_raw,
        "round_second(round_first(base * lucky_damage_raw / 10000) * critical_damage_raw / 10000)",
        "round_second(round_first(base * critical_damage_raw / 10000) * lucky_damage_raw / 10000)",
        "round(base * lucky_damage_raw * critical_damage_raw / 100000000)",
    ));
    for (interpretation, critical_factor, lucky_then_critical, critical_then_lucky, product) in
        interpretations
    {
        for first_rounding in StageRounding::ALL {
            for second_rounding in StageRounding::ALL {
                candidates.push(nested_stage_candidate(
                    interpretation,
                    lucky_then_critical,
                    observed_damage,
                    lucky_damage_raw,
                    critical_factor,
                    lucky_damage_raw,
                    BPSR_FIXED_POINT_SCALE,
                    first_rounding,
                    second_rounding,
                ));
                candidates.push(nested_stage_candidate(
                    interpretation,
                    critical_then_lucky,
                    observed_damage,
                    critical_factor,
                    lucky_damage_raw,
                    BPSR_FIXED_POINT_SCALE,
                    lucky_damage_raw,
                    first_rounding,
                    second_rounding,
                ));
            }
        }
        for rounding in StageRounding::ALL {
            candidates.push(single_product_stage_candidate(
                interpretation,
                product,
                observed_damage,
                lucky_damage_raw,
                critical_factor,
                lucky_damage_raw,
                BPSR_FIXED_POINT_SCALE,
                rounding,
            ));
        }
    }
    candidates
}

fn single_stage_candidate(
    critical_factor_interpretation: &'static str,
    formula: &'static str,
    observed_damage: i64,
    current_factor: i64,
    removed_factor: i64,
    rounding: StageRounding,
) -> IntegerStageCandidate {
    let Some((base_min, base_max)) = stage_preimage(
        i128::from(observed_damage),
        i128::from(current_factor),
        i128::from(BPSR_FIXED_POINT_SCALE),
        rounding,
    ) else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            rounding,
            None,
            "no_integer_preimage",
        );
    };
    let Some(counterfactual_min) = stage_output(
        base_min,
        i128::from(removed_factor),
        i128::from(BPSR_FIXED_POINT_SCALE),
        rounding,
    ) else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            rounding,
            None,
            "arithmetic_overflow",
        );
    };
    let Some(counterfactual_max) = stage_output(
        base_max,
        i128::from(removed_factor),
        i128::from(BPSR_FIXED_POINT_SCALE),
        rounding,
    ) else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            rounding,
            None,
            "arithmetic_overflow",
        );
    };
    stage_candidate_from_ranges(
        critical_factor_interpretation,
        formula,
        rounding,
        None,
        base_min,
        base_max,
        counterfactual_min,
        counterfactual_max,
    )
}

#[allow(clippy::too_many_arguments)]
fn nested_stage_candidate(
    critical_factor_interpretation: &'static str,
    formula: &'static str,
    observed_damage: i64,
    current_first_factor: i64,
    current_second_factor: i64,
    removed_first_factor: i64,
    removed_second_factor: i64,
    first_rounding: StageRounding,
    second_rounding: StageRounding,
) -> IntegerStageCandidate {
    let Some((intermediate_min, intermediate_max)) = stage_preimage(
        i128::from(observed_damage),
        i128::from(current_second_factor),
        i128::from(BPSR_FIXED_POINT_SCALE),
        second_rounding,
    ) else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            first_rounding,
            Some(second_rounding),
            "no_integer_preimage",
        );
    };
    let Some(intermediate_width) = intermediate_max
        .checked_sub(intermediate_min)
        .and_then(|width| width.checked_add(1))
    else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            first_rounding,
            Some(second_rounding),
            "arithmetic_overflow",
        );
    };
    if intermediate_width > 10_000 {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            first_rounding,
            Some(second_rounding),
            "bounded_enumeration_limit_exceeded",
        );
    }
    let mut base_min = None::<i128>;
    let mut base_max = None::<i128>;
    for intermediate in intermediate_min..=intermediate_max {
        if let Some((candidate_min, candidate_max)) = stage_preimage(
            intermediate,
            i128::from(current_first_factor),
            i128::from(BPSR_FIXED_POINT_SCALE),
            first_rounding,
        ) {
            base_min = Some(base_min.map_or(candidate_min, |value| value.min(candidate_min)));
            base_max = Some(base_max.map_or(candidate_max, |value| value.max(candidate_max)));
        }
    }
    let (Some(base_min), Some(base_max)) = (base_min, base_max) else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            first_rounding,
            Some(second_rounding),
            "no_integer_preimage",
        );
    };
    let counterfactual = |base| {
        let intermediate = stage_output(
            base,
            i128::from(removed_first_factor),
            i128::from(BPSR_FIXED_POINT_SCALE),
            first_rounding,
        )?;
        stage_output(
            intermediate,
            i128::from(removed_second_factor),
            i128::from(BPSR_FIXED_POINT_SCALE),
            second_rounding,
        )
    };
    let (Some(counterfactual_min), Some(counterfactual_max)) =
        (counterfactual(base_min), counterfactual(base_max))
    else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            first_rounding,
            Some(second_rounding),
            "arithmetic_overflow",
        );
    };
    stage_candidate_from_ranges(
        critical_factor_interpretation,
        formula,
        first_rounding,
        Some(second_rounding),
        base_min,
        base_max,
        counterfactual_min,
        counterfactual_max,
    )
}

#[allow(clippy::too_many_arguments)]
fn single_product_stage_candidate(
    critical_factor_interpretation: &'static str,
    formula: &'static str,
    observed_damage: i64,
    current_first_factor: i64,
    current_second_factor: i64,
    removed_first_factor: i64,
    removed_second_factor: i64,
    rounding: StageRounding,
) -> IntegerStageCandidate {
    let Some(denominator) =
        i128::from(BPSR_FIXED_POINT_SCALE).checked_mul(i128::from(BPSR_FIXED_POINT_SCALE))
    else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            rounding,
            None,
            "arithmetic_overflow",
        );
    };
    let Some(current_factor) =
        i128::from(current_first_factor).checked_mul(i128::from(current_second_factor))
    else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            rounding,
            None,
            "arithmetic_overflow",
        );
    };
    let Some(removed_factor) =
        i128::from(removed_first_factor).checked_mul(i128::from(removed_second_factor))
    else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            rounding,
            None,
            "arithmetic_overflow",
        );
    };
    let Some((base_min, base_max)) = stage_preimage(
        i128::from(observed_damage),
        current_factor,
        denominator,
        rounding,
    ) else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            rounding,
            None,
            "no_integer_preimage",
        );
    };
    let (Some(counterfactual_min), Some(counterfactual_max)) = (
        stage_output(base_min, removed_factor, denominator, rounding),
        stage_output(base_max, removed_factor, denominator, rounding),
    ) else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            rounding,
            None,
            "arithmetic_overflow",
        );
    };
    stage_candidate_from_ranges(
        critical_factor_interpretation,
        formula,
        rounding,
        None,
        base_min,
        base_max,
        counterfactual_min,
        counterfactual_max,
    )
}

fn stage_preimage(
    output: i128,
    factor: i128,
    denominator: i128,
    rounding: StageRounding,
) -> Option<(i128, i128)> {
    if output < 0 || factor <= 0 || denominator <= 0 {
        return None;
    }
    let bias = match rounding {
        StageRounding::Floor => 0,
        StageRounding::HalfUp => denominator / 2,
    };
    let lower_numerator = output.checked_mul(denominator)?.checked_sub(bias)?;
    let upper_numerator = output
        .checked_add(1)?
        .checked_mul(denominator)?
        .checked_sub(bias)?;
    let lower = if lower_numerator <= 0 {
        0
    } else {
        ceil_div_nonnegative(lower_numerator, factor)?
    };
    let upper = ceil_div_nonnegative(upper_numerator.max(0), factor)?.checked_sub(1)?;
    (lower <= upper).then_some((lower, upper))
}

fn stage_output(
    base: i128,
    factor: i128,
    denominator: i128,
    rounding: StageRounding,
) -> Option<i128> {
    if base < 0 || factor <= 0 || denominator <= 0 {
        return None;
    }
    let bias = match rounding {
        StageRounding::Floor => 0,
        StageRounding::HalfUp => denominator / 2,
    };
    base.checked_mul(factor)?
        .checked_add(bias)?
        .checked_div(denominator)
}

fn ceil_div_nonnegative(numerator: i128, denominator: i128) -> Option<i128> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    if numerator == 0 {
        Some(0)
    } else {
        numerator
            .checked_add(denominator.checked_sub(1)?)?
            .checked_div(denominator)
    }
}

#[allow(clippy::too_many_arguments)]
fn stage_candidate_from_ranges(
    critical_factor_interpretation: &'static str,
    formula: &'static str,
    first_rounding: StageRounding,
    second_rounding: Option<StageRounding>,
    base_min: i128,
    base_max: i128,
    counterfactual_min: i128,
    counterfactual_max: i128,
) -> IntegerStageCandidate {
    let converted = (
        i64::try_from(base_min),
        i64::try_from(base_max),
        i64::try_from(counterfactual_min),
        i64::try_from(counterfactual_max),
    );
    let (Ok(base_min), Ok(base_max), Ok(counterfactual_min), Ok(counterfactual_max)) = converted
    else {
        return incompatible_stage_candidate(
            critical_factor_interpretation,
            formula,
            first_rounding,
            second_rounding,
            "integer_range_overflow",
        );
    };
    IntegerStageCandidate {
        formula,
        critical_factor_interpretation,
        first_rounding: first_rounding.label(),
        second_rounding: second_rounding.map(StageRounding::label),
        evaluation_status: "compatible_integer_preimage",
        compatible_with_observed_damage: true,
        latent_base_min: Some(base_min),
        latent_base_max: Some(base_max),
        counterfactual_min: Some(counterfactual_min),
        counterfactual_max: Some(counterfactual_max),
    }
}

fn incompatible_stage_candidate(
    critical_factor_interpretation: &'static str,
    formula: &'static str,
    first_rounding: StageRounding,
    second_rounding: Option<StageRounding>,
    evaluation_status: &'static str,
) -> IntegerStageCandidate {
    IntegerStageCandidate {
        formula,
        critical_factor_interpretation,
        first_rounding: first_rounding.label(),
        second_rounding: second_rounding.map(StageRounding::label),
        evaluation_status,
        compatible_with_observed_damage: false,
        latent_base_min: None,
        latent_base_max: None,
        counterfactual_min: None,
        counterfactual_max: None,
    }
}

fn combined_packet_evidence(
    candidates: &[CandidateHit],
    magnitudes: &BTreeMap<MagnitudeKey, ProvenMagnitude>,
    players: &HashSet<(u32, u32, i64)>,
    example_limit: usize,
) -> CombinedPacketEvidence {
    let mut evidence = CombinedPacketEvidence::default();
    let mut shapes = BTreeMap::<CombinedPacketShapeKey, u64>::new();
    let mut exact_examples = Vec::new();
    let mut retained_examples = Vec::new();
    for hit in candidates
        .iter()
        .filter(|candidate| candidate.critical && candidate.lucky)
    {
        evidence.candidates = evidence.candidates.saturating_add(1);
        let value_source = match (hit.normal_value, hit.lucky_value) {
            (Some(_), None) => {
                evidence.normal_value_only = evidence.normal_value_only.saturating_add(1);
                "normal_value_only"
            }
            (None, Some(_)) => {
                evidence.lucky_value_only = evidence.lucky_value_only.saturating_add(1);
                "lucky_value_only"
            }
            (Some(_), Some(_)) => {
                evidence.both_values = evidence.both_values.saturating_add(1);
                "both_values"
            }
            (None, None) => {
                evidence.neither_value = evidence.neither_value.saturating_add(1);
                "neither_value"
            }
        };
        if hit.normal_value == Some(hit.amount) {
            evidence.amount_matches_normal_value =
                evidence.amount_matches_normal_value.saturating_add(1);
        }
        if hit.lucky_value == Some(hit.amount) {
            evidence.amount_matches_lucky_value =
                evidence.amount_matches_lucky_value.saturating_add(1);
        }
        match hit.reported_critical {
            Some(true) => {
                evidence.reported_critical_true = evidence.reported_critical_true.saturating_add(1)
            }
            Some(false) => {
                evidence.reported_critical_false =
                    evidence.reported_critical_false.saturating_add(1)
            }
            None => {
                evidence.reported_critical_absent =
                    evidence.reported_critical_absent.saturating_add(1)
            }
        }
        *shapes
            .entry(CombinedPacketShapeKey {
                value_source,
                reported_critical: hit.reported_critical,
                type_flags: hit.type_flags,
                hit_event_id: hit.hit_event_id,
                damage_source: hit.damage_source,
            })
            .or_default() += 1;

        let exact_external_player_window_magnitudes = hit
            .active_windows
            .iter()
            .filter(|window| {
                players.contains(&(
                    window.key.session_ordinal,
                    window.key.run_ordinal,
                    window.key.provider_entity_uuid,
                ))
            })
            .filter_map(|window| {
                let level = window.level?;
                let origin = window.origin?;
                let magnitude = magnitudes.get(&MagnitudeKey {
                    session_ordinal: window.key.session_ordinal,
                    run_ordinal: window.key.run_ordinal,
                    target_entity_uuid: window.key.target_entity_uuid,
                    provider_entity_uuid: window.key.provider_entity_uuid,
                    instance_id: window.key.instance_id,
                    level,
                    origin_source_type_id: origin.source_type_id,
                    origin_source_config_id: origin.source_config_id,
                })?;
                Some(CombinedWindowMagnitude {
                    provider_entity_uuid: window.key.provider_entity_uuid,
                    instance_id: window.key.instance_id,
                    level,
                    origin_source_type_id: origin.source_type_id,
                    origin_source_config_id: origin.source_config_id,
                    critical_raw_delta: magnitude.critical_raw_delta,
                    lucky_raw_delta: magnitude.lucky_raw_delta,
                })
            })
            .collect::<Vec<_>>();
        if !exact_external_player_window_magnitudes.is_empty() {
            evidence.candidates_with_any_exact_external_player_window_magnitude = evidence
                .candidates_with_any_exact_external_player_window_magnitude
                .saturating_add(1);
        }
        let example = CombinedPacketExample {
            session_id: hit.session_id.clone(),
            sequence: hit.sequence,
            observed_micros: hit.observed_micros,
            run_ordinal: hit.run_ordinal,
            source_entity_uuid: hit.source_entity_uuid,
            target_entity_uuid: hit.target_entity_uuid,
            ability_id: hit.ability_id,
            amount: hit.amount,
            normal_value: hit.normal_value,
            lucky_value: hit.lucky_value,
            reported_critical: hit.reported_critical,
            type_flags: hit.type_flags,
            hit_event_id: hit.hit_event_id,
            damage_source: hit.damage_source,
            current_critical_chance_raw: hit
                .chance_state
                .critical_chance_raw
                .map(|observation| observation.value),
            current_lucky_chance_raw: hit
                .chance_state
                .lucky_chance_raw
                .map(|observation| observation.value),
            critical_damage_raw: hit
                .chance_state
                .critical_damage_raw
                .map(|observation| observation.value),
            exact_external_player_window_magnitudes,
        };
        if example.exact_external_player_window_magnitudes.is_empty() {
            if retained_examples.len() < example_limit {
                retained_examples.push(example);
            }
        } else if exact_examples.len() < example_limit {
            exact_examples.push(example);
        }
    }
    evidence.packet_shapes = shapes
        .into_iter()
        .map(|(key, events)| CombinedPacketShape {
            value_source: key.value_source,
            reported_critical: key.reported_critical,
            type_flags: key.type_flags,
            hit_event_id: key.hit_event_id,
            damage_source: key.damage_source,
            events,
        })
        .collect();
    evidence.examples = exact_examples
        .into_iter()
        .chain(retained_examples)
        .take(example_limit)
        .collect();
    evidence
}

fn active_player_window_count(
    candidates: &[CandidateHit],
    players: &HashSet<(u32, u32, i64)>,
) -> u64 {
    u64::try_from(
        candidates
            .iter()
            .flat_map(|hit| hit.active_windows.iter().map(|window| window.key))
            .filter(|key| {
                players.contains(&(
                    key.session_ordinal,
                    key.run_ordinal,
                    key.provider_entity_uuid,
                ))
            })
            .collect::<HashSet<_>>()
            .len(),
    )
    .unwrap_or(u64::MAX)
}

#[derive(Debug)]
struct Arguments {
    deployment_id: String,
    game_build: String,
    proof: PathBuf,
    damage_surface: PathBuf,
    critical_factor_proof: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    example_limit: usize,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let deployment_id = take_value(&mut values, "--deployment")?
        .to_string_lossy()
        .into_owned();
    let game_build = take_value(&mut values, "--build")?
        .to_string_lossy()
        .into_owned();
    if deployment_id.trim().is_empty()
        || game_build.trim().is_empty()
        || !game_build.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("--deployment and numeric --build must be non-empty".to_owned());
    }
    let proof = PathBuf::from(take_value(&mut values, "--transition-proof")?);
    let damage_surface = PathBuf::from(take_value(&mut values, "--damage-surface")?);
    let critical_factor_proof = PathBuf::from(take_value(&mut values, "--critical-factor-proof")?);
    let mut rlogs = Vec::new();
    while let Some(value) = take_optional_value(&mut values, "--rlog") {
        rlogs.push(PathBuf::from(value));
    }
    if rlogs.is_empty() {
        return Err(usage());
    }
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let example_limit = take_optional_value(&mut values, "--example-limit")
        .map(|value| value.to_string_lossy().parse::<usize>())
        .transpose()
        .map_err(|_| "--example-limit must be an unsigned integer".to_owned())?
        .unwrap_or(24);
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        deployment_id,
        game_build,
        proof,
        damage_surface,
        critical_factor_proof,
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
    "usage: rlogs-bpsr-inspiration-proc-attribution-proof --deployment <deployment-id> --build <client-build> --transition-proof <status-attribute-proof.json> --damage-surface <damage-formula-surface.json> --critical-factor-proof <critical-factor-proof.json> --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <report.json> [--example-limit <count>]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed(value: i64) -> ObservedAttribute {
        ObservedAttribute {
            value,
            sequence: 0,
            observed_micros: 0,
            wire: None,
        }
    }

    fn hit(amount: i64, critical: bool, lucky: bool) -> CandidateHit {
        CandidateHit {
            session_ordinal: 0,
            session_id: "session".to_owned(),
            protocol_pack_digest: "sha256:test".to_owned(),
            sequence: 1,
            observed_micros: 1,
            run_ordinal: 1,
            source_actor_id: 1,
            source_entity_uuid: 1,
            target_entity_uuid: 2,
            ability_id: Some(3),
            amount,
            normal_value: (!lucky).then_some(amount),
            lucky_value: lucky.then_some(amount),
            reported_critical: None,
            type_flags: critical.then_some(1),
            hit_event_id: Some(1),
            damage_source: None,
            damage_type: None,
            owner_level: None,
            owner_stage: None,
            normal_hit: None,
            property: None,
            passive_uuid: None,
            rainbow: None,
            damage_mode: None,
            skill_effect_uuid: None,
            skill_effect_group_index: None,
            skill_effect_component_index: None,
            skill_effect_component_count: None,
            critical,
            lucky,
            damage_wire: None,
            chance_state: ChanceState {
                critical_chance_raw: Some(observed(3_000)),
                lucky_chance_raw: Some(observed(1_500)),
                critical_damage_raw: Some(observed(5_000)),
                lucky_damage_raw: Some(observed(4_500)),
            },
            active_windows: Vec::new(),
        }
    }

    #[test]
    fn critical_path_uses_the_exact_build_additive_bonus_interpretation() {
        assert_eq!(
            critical_fraction(&hit(12_000, true, false), 300),
            FractionOutcome::Exact(400, 1, 3_000)
        );
    }

    #[test]
    fn critical_factor_authority_rejects_build_or_combined_stage_drift() {
        let mut proof = serde_json::json!({
            "schema_version": 1,
            "deployment_id": "global",
            "game_build": "24687926",
            "proof_state": "critical-only-attribute-12510-additive-bonus-factor-proven",
            "interpretation": {
                "attribute_id": 12510,
                "authoritative_interpretation": "additive_bonus"
            },
            "policy": {
                "critical_only_factor_interpretation_authority": true,
                "ordinary_damage_totals_unchanged": true,
                "current_character_snapshot_substitution_allowed": false,
                "combined_critical_lucky_order_authority": false
            },
            "runtime_decision": {
                "critical_only_team_luck_component_promotion_allowed": true,
                "combined_critical_lucky_promotion_allowed": false
            }
        });
        assert!(validate_critical_factor_proof(&proof, "global", "24687926").is_ok());

        proof["game_build"] = Value::String("24687927".into());
        assert!(validate_critical_factor_proof(&proof, "global", "24687926").is_err());
        proof["game_build"] = Value::String("24687926".into());
        proof["policy"]["combined_critical_lucky_order_authority"] = Value::Bool(true);
        assert!(validate_critical_factor_proof(&proof, "global", "24687926").is_err());
    }

    #[test]
    fn lucky_path_assigns_provider_share_of_a_noncritical_lucky_row() {
        assert_eq!(
            lucky_fraction(&hit(9_000, false, true), 300),
            FractionOutcome::Exact(1_800, 1, 1_500)
        );
    }

    #[test]
    fn single_lucky_path_rejects_combined_rows_so_joint_path_owns_them() {
        let hit = hit(15_000, true, true);
        assert_eq!(
            lucky_fraction(&hit, 300),
            FractionOutcome::ArithmeticOverflow
        );
    }

    #[test]
    fn combined_packet_component_stays_blocked_while_factor_interpretation_is_unresolved() {
        let hit = hit(15_000, true, true);
        assert_eq!(
            combined_fraction(
                &hit,
                ProvenMagnitude {
                    critical_raw_delta: 300,
                    lucky_raw_delta: 300,
                },
            ),
            CombinedFractionOutcome::CriticalDamageInterpretationUnresolved
        );
    }

    #[test]
    fn formula_input_coverage_retains_exact_sequence_time_and_wire_provenance() {
        let attribute_wire = WireKey {
            capture_sequence: 4,
            connection_id: 1,
            stream_id: 1,
        };
        let damage_wire = WireKey {
            capture_sequence: 9,
            connection_id: 1,
            stream_id: 1,
        };
        let mut hit = hit(12_000, true, true);
        hit.sequence = 10;
        hit.observed_micros = 1_000;
        hit.damage_wire = Some(damage_wire);
        hit.chance_state.critical_chance_raw = Some(ObservedAttribute {
            value: 3_000,
            sequence: 5,
            observed_micros: 500,
            wire: Some(attribute_wire),
        });
        hit.chance_state.critical_damage_raw = Some(ObservedAttribute {
            value: 5_000,
            sequence: 8,
            observed_micros: 800,
            wire: Some(damage_wire),
        });
        hit.chance_state.lucky_chance_raw = Some(ObservedAttribute {
            value: 1_500,
            sequence: 6,
            observed_micros: 600,
            wire: Some(attribute_wire),
        });
        hit.chance_state.lucky_damage_raw = Some(ObservedAttribute {
            value: 4_500,
            sequence: 7,
            observed_micros: 700,
            wire: Some(attribute_wire),
        });
        hit.active_windows.push(WindowSnapshot {
            key: WindowKey {
                session_ordinal: 0,
                run_ordinal: 1,
                target_entity_uuid: 1,
                provider_entity_uuid: 40,
                instance_id: 30,
            },
            provider_actor_id: 40,
            level: Some(2),
            origin: Some(StatusOrigin {
                source_type_id: 1,
                source_config_id: INSPIRATION_PARENT_EFFECT_ID,
            }),
            applied_observed_micros: 100,
        });
        let magnitudes = BTreeMap::from([(
            MagnitudeKey {
                session_ordinal: 0,
                run_ordinal: 1,
                target_entity_uuid: 1,
                provider_entity_uuid: 40,
                instance_id: 30,
                level: 2,
                origin_source_type_id: 1,
                origin_source_config_id: INSPIRATION_PARENT_EFFECT_ID,
            },
            ProvenMagnitude {
                critical_raw_delta: 150,
                lucky_raw_delta: 150,
            },
        )]);
        let players = HashSet::from([(0, 1, 40)]);
        let coverage = formula_input_snapshot_coverage(&[hit], &magnitudes, &players, 8);
        assert_eq!(coverage.exact_single_provider_candidate_events, 1);
        assert_eq!(coverage.paths.len(), 1);
        assert_eq!(coverage.paths[0].candidate_events, 1);
        assert_eq!(coverage.paths[0].complete_input_sets, 1);
        assert_eq!(coverage.paths[0].all_inputs_wire_provenance, 1);
        assert_eq!(coverage.paths[0].maximum_oldest_input_age_micros, Some(500));
        assert_eq!(coverage.attributes.len(), 4);
        assert_eq!(coverage.oldest_observed_examples.len(), 4);
        assert_eq!(coverage.oldest_observed_examples[0].attribute_id, 11_710);
        assert_eq!(coverage.oldest_observed_examples[0].age_sequences, 5);
        assert!(!coverage.event_time_snapshot_authority);
    }

    #[test]
    fn integer_stage_candidates_retain_divergent_critical_factor_interpretations() {
        let critical = critical_bonus_stage_candidates(1_500, 5_000);
        assert_eq!(critical.len(), 4);
        let additive = critical
            .iter()
            .filter(|candidate| candidate.critical_factor_interpretation == "additive_bonus")
            .collect::<Vec<_>>();
        let direct = critical
            .iter()
            .filter(|candidate| candidate.critical_factor_interpretation == "direct_total")
            .collect::<Vec<_>>();
        assert_eq!(additive.len(), 2);
        assert_eq!(direct.len(), 2);
        assert!(additive.iter().all(|candidate| {
            candidate.compatible_with_observed_damage
                && candidate.counterfactual_min == Some(1_000)
                && candidate.counterfactual_max == Some(1_000)
        }));
        assert!(
            direct
                .iter()
                .all(|candidate| candidate.compatible_with_observed_damage)
        );
        assert_eq!(converged_stage_counterfactual(direct.iter().copied()), None);
        assert_eq!(
            converged_stage_counterfactual(critical.iter()),
            None,
            "unproven factor interpretations must not vote as one formula"
        );

        let combined = combined_critical_bonus_stage_candidates(675, 5_000, 4_500);
        assert_eq!(combined.len(), 20);
        let additive = combined
            .iter()
            .filter(|candidate| candidate.critical_factor_interpretation == "additive_bonus")
            .collect::<Vec<_>>();
        let direct = combined
            .iter()
            .filter(|candidate| candidate.critical_factor_interpretation == "direct_total")
            .collect::<Vec<_>>();
        assert!(additive.iter().all(|candidate| {
            candidate.compatible_with_observed_damage
                && candidate.counterfactual_min == Some(450)
                && candidate.counterfactual_max == Some(450)
        }));
        assert!(
            direct
                .iter()
                .all(|candidate| candidate.compatible_with_observed_damage)
        );
        assert_eq!(converged_stage_counterfactual(direct.iter().copied()), None);
    }

    #[test]
    fn incompatible_rounding_model_is_retained_but_cannot_vote_on_convergence() {
        let candidates = critical_bonus_stage_candidates(2, 5_000);
        assert_eq!(candidates.len(), 4);
        assert_eq!(
            candidates
                .iter()
                .filter(|candidate| candidate.critical_factor_interpretation == "additive_bonus")
                .filter(|candidate| candidate.compatible_with_observed_damage)
                .count(),
            1
        );
        assert!(candidates.iter().any(|candidate| {
            candidate.critical_factor_interpretation == "additive_bonus"
                && !candidate.compatible_with_observed_damage
                && candidate.evaluation_status == "no_integer_preimage"
        }));
        assert!(candidates.iter().any(|candidate| {
            candidate.critical_factor_interpretation == "direct_total"
                && candidate.compatible_with_observed_damage
        }));
    }

    fn stage_identity(owner_stage: Option<i32>) -> IntegerStageIdentityKey {
        IntegerStageIdentityKey {
            path: "critical_proc_bonus",
            ability_id: Some(3),
            hit_event_id: Some(1),
            damage_source: Some(2),
            damage_type: Some(4),
            type_flags: Some(1),
            reported_critical: Some(true),
            owner_level: Some(80),
            owner_stage,
            normal_hit: Some(false),
            property: Some(5),
            passive_uuid: None,
            rainbow: Some(false),
            damage_mode: Some(6),
            skill_effect_uuid: None,
            skill_effect_group_index: None,
            skill_effect_component_index: None,
            skill_effect_component_count: None,
            critical_damage_raw: Some(5_000),
            lucky_damage_raw: None,
            stage_input_observation_state: "complete_not_after_damage",
            oldest_stage_input_age_sequences: Some(10),
            oldest_stage_input_age_micros: Some(100_000),
            stage_inputs_all_wire_provenance: true,
            stage_inputs_all_same_wire_as_damage: false,
        }
    }

    fn stage_accumulator(events: u64) -> IntegerStageIdentityAccumulator {
        IntegerStageIdentityAccumulator {
            events,
            complete_stage_inputs: events,
            compatible_candidate_events: events,
            exact_stage_independent_events: events,
            unresolved_stage_or_rounding_events: 0,
            events_without_compatible_candidates: 0,
            observed_damage_sum: i128::from(events) * 1_500,
            critical_damage_raw_values: BTreeSet::from([5_000]),
            lucky_damage_raw_values: BTreeSet::new(),
        }
    }

    #[test]
    fn stage_input_freshness_uses_oldest_exact_observation_without_granting_authority() {
        let same_wire = WireKey {
            capture_sequence: 1,
            connection_id: 2,
            stream_id: 3,
        };
        let other_wire = WireKey {
            capture_sequence: 4,
            connection_id: 2,
            stream_id: 3,
        };
        let mut candidate = hit(1_500, true, true);
        candidate.sequence = 100;
        candidate.observed_micros = 2_000_000;
        candidate.damage_wire = Some(same_wire);
        let critical = ObservedAttribute {
            value: 5_000,
            sequence: 99,
            observed_micros: 1_999_900,
            wire: Some(same_wire),
        };
        let lucky = ObservedAttribute {
            value: 4_500,
            sequence: 80,
            observed_micros: 1_400_000,
            wire: Some(other_wire),
        };
        let freshness = exact_stage_input_freshness(&candidate, &[Some(critical), Some(lucky)]);
        assert_eq!(freshness.observation_state, "complete_not_after_damage");
        assert_eq!(freshness.oldest_age_sequences, Some(20));
        assert_eq!(freshness.oldest_age_micros, Some(600_000));
        assert!(freshness.all_wire_provenance);
        assert!(!freshness.all_same_wire_as_damage);
        assert_eq!(
            stage_input_age_bucket(freshness.observation_state, freshness.oldest_age_micros),
            "500ms_to_1s"
        );
        let missing = exact_stage_input_freshness(&candidate, &[Some(critical), None]);
        assert_eq!(missing.observation_state, "missing_required_stage_input");
        assert_eq!(
            stage_input_age_bucket(missing.observation_state, missing.oldest_age_micros),
            "missing"
        );
    }

    #[test]
    fn exact_damage_surface_join_retains_coefficients_without_granting_authority() {
        let surface = DamageFormulaSurface {
            schema_version: 1,
            rows_by_key: BTreeMap::from([(
                (3, 1),
                vec![DamageFormulaSurfaceRow {
                    damage_id: "Damage.3.1".to_owned(),
                    damage_script: Some("DamageScript".to_owned()),
                    pve_damage_ratio: vec![100, 200, 300],
                    pve_fixed_parameter: vec![10, 20, 30],
                }],
            )]),
            rows_by_ability: BTreeMap::new(),
        };
        let joined = integer_stage_damage_surface_join(
            BTreeMap::from([(stage_identity(Some(1)), stage_accumulator(2))]),
            &surface,
        );
        assert_eq!(joined.events, 2);
        assert_eq!(joined.events_with_exactly_one_surface_row, 2);
        assert_eq!(joined.events_with_resolved_damage_script, 2);
        assert!(!joined.surface_runtime_formula_authority);
        assert!(!joined.damage_script_preimage_breakdown_authority);
        assert_eq!(joined.damage_script_preimage_breakdown.len(), 1);
        assert_eq!(
            joined.damage_script_preimage_breakdown[0].surface_binding,
            "exact_hit_event"
        );
        assert_eq!(
            joined.damage_script_preimage_breakdown[0].damage_script,
            "DamageScript"
        );
        assert_eq!(joined.damage_script_preimage_breakdown[0].events, 2);
        assert_eq!(
            joined.damage_script_preimage_breakdown[0].exact_stage_independent_events,
            2
        );
        assert!(!joined.damage_script_preimage_breakdown[0].formula_authority);
        assert!(!joined.stage_input_freshness_breakdown_authority);
        assert_eq!(joined.stage_input_freshness_breakdown.len(), 1);
        assert_eq!(joined.stage_input_freshness_breakdown[0].events, 2);
        assert_eq!(
            joined.stage_input_freshness_breakdown[0].oldest_age_bucket,
            "1us_to_100ms"
        );
        assert!(!joined.stage_input_freshness_breakdown[0].formula_authority);
        assert_eq!(joined.groups.len(), 1);
        assert_eq!(
            joined.groups[0].damage_surface_resolution,
            "exactly_one_exact_build_surface_row"
        );
        assert_eq!(
            joined.groups[0].damage_surface_candidates[0].selected_pve_damage_ratio,
            Some(200)
        );
        assert_eq!(
            joined.groups[0].damage_surface_candidates[0].selected_pve_fixed_parameter,
            Some(20)
        );
        assert!(!joined.groups[0].damage_surface_candidates[0].owner_stage_selection_authority);
    }

    #[test]
    fn exact_stage_inputs_are_damage_surface_identity_dimensions() {
        let row = DamageFormulaSurfaceRow {
            damage_id: "Damage.3.1".to_owned(),
            damage_script: Some("DamageScript".to_owned()),
            pve_damage_ratio: vec![100],
            pve_fixed_parameter: vec![10],
        };
        let surface = DamageFormulaSurface {
            schema_version: 2,
            rows_by_key: BTreeMap::from([((3, 1), vec![row])]),
            rows_by_ability: BTreeMap::new(),
        };
        let first = stage_identity(Some(0));
        let mut second = stage_identity(Some(0));
        second.critical_damage_raw = Some(5_100);
        let mut second_accumulator = stage_accumulator(3);
        second_accumulator.critical_damage_raw_values = BTreeSet::from([5_100]);
        let joined = integer_stage_damage_surface_join(
            BTreeMap::from([(first, stage_accumulator(2)), (second, second_accumulator)]),
            &surface,
        );
        assert_eq!(joined.groups.len(), 2);
        assert_eq!(joined.events, 5);
        assert_eq!(joined.groups[0].critical_damage_raw_values.len(), 1);
        assert_eq!(joined.groups[1].critical_damage_raw_values.len(), 1);
        assert_ne!(
            joined.groups[0].critical_damage_raw_values,
            joined.groups[1].critical_damage_raw_values
        );
        assert_eq!(joined.damage_script_preimage_breakdown.len(), 1);
        assert_eq!(
            joined.damage_script_preimage_breakdown[0].identity_groups,
            2
        );
        assert_eq!(joined.damage_script_preimage_breakdown[0].events, 5);
    }

    #[test]
    fn ambiguous_damage_surface_rows_are_retained_and_never_resolved() {
        let rows = vec![
            DamageFormulaSurfaceRow {
                damage_id: "Damage.A".to_owned(),
                damage_script: Some("ScriptA".to_owned()),
                pve_damage_ratio: vec![100],
                pve_fixed_parameter: vec![10],
            },
            DamageFormulaSurfaceRow {
                damage_id: "Damage.B".to_owned(),
                damage_script: Some("ScriptB".to_owned()),
                pve_damage_ratio: vec![200],
                pve_fixed_parameter: vec![20],
            },
        ];
        let surface = DamageFormulaSurface {
            schema_version: 1,
            rows_by_key: BTreeMap::from([((3, 1), rows)]),
            rows_by_ability: BTreeMap::new(),
        };
        let joined = integer_stage_damage_surface_join(
            BTreeMap::from([(stage_identity(Some(0)), stage_accumulator(3))]),
            &surface,
        );
        assert_eq!(joined.events_with_ambiguous_surface_rows, 3);
        assert_eq!(joined.events_with_resolved_damage_script, 0);
        assert_eq!(joined.events_without_resolved_damage_script, 3);
        assert_eq!(
            joined.groups[0].damage_surface_resolution,
            "ambiguous_exact_build_surface_rows"
        );
        assert_eq!(joined.groups[0].damage_surface_candidates.len(), 2);
        assert_eq!(
            joined.damage_script_preimage_breakdown[0].surface_binding,
            "ambiguous_exact_hit_event"
        );
        assert_eq!(
            joined.damage_script_preimage_breakdown[0].damage_script,
            "<ambiguous_damage_script>"
        );
        assert_eq!(joined.damage_script_preimage_breakdown[0].events, 3);
    }

    #[test]
    fn missing_hit_event_retains_unique_ability_surface_as_diagnostic_only() {
        let row = DamageFormulaSurfaceRow {
            damage_id: "Damage.3.0".to_owned(),
            damage_script: Some("AbilityOnlyScript".to_owned()),
            pve_damage_ratio: vec![100],
            pve_fixed_parameter: vec![10],
        };
        let surface = DamageFormulaSurface {
            schema_version: 2,
            rows_by_key: BTreeMap::from([((3, 0), vec![row.clone()])]),
            rows_by_ability: BTreeMap::from([(3, vec![row])]),
        };
        let mut identity = stage_identity(Some(0));
        identity.hit_event_id = None;
        let joined = integer_stage_damage_surface_join(
            BTreeMap::from([(identity, stage_accumulator(4))]),
            &surface,
        );
        assert_eq!(joined.events_without_surface_row, 4);
        assert_eq!(
            joined.events_with_unique_ability_surface_candidate_when_hit_event_absent,
            4
        );
        assert_eq!(
            joined.events_with_unique_ability_surface_candidate_and_resolved_damage_script_when_hit_event_absent,
            4
        );
        assert_eq!(
            joined.events_without_exact_or_unique_ability_surface_candidate,
            0
        );
        assert!(!joined.unique_ability_surface_candidate_authority);
        assert!(!joined.damage_script_preimage_breakdown_authority);
        assert_eq!(
            joined.damage_script_preimage_breakdown[0].surface_binding,
            "ability_only_diagnostic"
        );
        assert_eq!(
            joined.damage_script_preimage_breakdown[0].damage_script,
            "AbilityOnlyScript"
        );
        assert_eq!(joined.damage_script_preimage_breakdown[0].events, 4);
        assert_eq!(
            joined.groups[0].unique_ability_damage_surface_resolution,
            "hit_event_absent_one_unique_ability_surface_candidate_diagnostic"
        );
        assert_eq!(
            joined.groups[0].unique_ability_damage_surface_candidates[0]
                .damage_script
                .as_deref(),
            Some("AbilityOnlyScript")
        );
    }

    #[test]
    fn exact_removal_magnitude_retains_provider_and_effect_level_identity() {
        let mut proof: StatusAttributeProof = serde_json::from_value(serde_json::json!({
            "schema_version": 28,
            "expected_deployment_id": "global",
            "expected_game_build": "24687926",
            "selected_effect_ids": [2202041],
            "selected_attribute_ids": [11710, 11712, 11780, 11782, 12510],
            "sessions": [{
                "rlog": "one.rlog",
                "session_id": "session",
                "bytes": 1,
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "deployment_id": "global",
                "game_build": "24687926",
                "protocol_pack_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            }],
            "wire_additive_equation_systems": [
                {
                    "attribute_id": 11712,
                    "equations": [{
                        "terms": [{"effect_id": 2202041, "origin": {"source_type_id": 1, "source_config_id": 2202040}, "level": 2, "signed_presence_delta": -1}],
                        "raw_attribute_delta": -150,
                        "examples": [{
                            "session_id": "session",
                            "run_ordinal": 1,
                            "target_entity_uuid": 20,
                            "status_instances": [{
                                "effect_id": 2202041,
                                "instance_id": 30,
                                "state": "removed",
                                "source_entity_uuid": 40
                            }]
                        }]
                    }]
                },
                {
                    "attribute_id": 11782,
                    "equations": [{
                        "terms": [{"effect_id": 2202041, "origin": {"source_type_id": 1, "source_config_id": 2202040}, "level": 2, "signed_presence_delta": -1}],
                        "raw_attribute_delta": -150,
                        "examples": [{
                            "session_id": "session",
                            "run_ordinal": 1,
                            "target_entity_uuid": 20,
                            "status_instances": [{
                                "effect_id": 2202041,
                                "instance_id": 30,
                                "state": "removed",
                                "source_entity_uuid": 40
                            }]
                        }]
                    }]
                }
            ]
        }))
        .expect("valid test proof");
        for attribute_id in [
            CRITICAL_CHANCE_ADD_ATTRIBUTE_ID,
            LUCKY_CHANCE_ADD_ATTRIBUTE_ID,
        ] {
            let fingerprint = ProofFingerprint {
                effect_id: INSPIRATION_EFFECT_ID,
                origin: Some(ProofOrigin {
                    source_type_id: INSPIRATION_ORIGIN_SOURCE_TYPE_ID,
                    source_config_id: INSPIRATION_PARENT_EFFECT_ID,
                }),
                level: Some(2),
                part_id: None,
                stacks: Some(1),
                count: Some(-1),
            };
            proof
                .reversible_static_coefficient_proofs
                .push(ReversibleStaticCoefficientProof {
                    attribute_id,
                    fingerprint: fingerprint.clone(),
                    status: "proven_reversible_static_coefficient".to_owned(),
                    proven_coefficient_units: Some(150),
                    normalized_coefficient_counts: BTreeMap::from([(150, 2)]),
                    apply_occurrences: 1,
                    remove_occurrences: 1,
                    independent_run_contexts: 2,
                    cross_actor_occurrences: 2,
                });
            proof
                .matched_lifecycle_coefficient_proofs
                .push(MatchedLifecycleCoefficientProof {
                    attribute_id,
                    fingerprint,
                    status: "proven_matched_lifecycle_coefficient".to_owned(),
                    proven_coefficient_units: Some(150),
                    exact_coefficient_counts: BTreeMap::from([(150, 2)]),
                    exact_pair_count: 2,
                    contradictory_pair_count: 0,
                    ambiguous_instance_count: 0,
                    application_only_instance_count: 0,
                    removal_only_instance_count: 0,
                    independent_run_contexts: 2,
                    cross_actor_exact_pairs: 2,
                    examples: Vec::new(),
                });
        }
        let (magnitudes, conflicts) = extract_magnitudes(&proof);
        assert_eq!(conflicts, 0);
        assert_eq!(magnitudes.len(), 1);
        assert!(magnitudes.contains_key(&MagnitudeKey {
            session_ordinal: 0,
            run_ordinal: 1,
            target_entity_uuid: 20,
            provider_entity_uuid: 40,
            instance_id: 30,
            level: 2,
            origin_source_type_id: 1,
            origin_source_config_id: 2_202_040,
        }));
        let lifecycle = level_lifecycle_evidence(&proof, &magnitudes).expect("valid lifecycle");
        assert_eq!(lifecycle.len(), 1);
        assert!(lifecycle[0].reversible_static_transform_proven);
        assert!(lifecycle[0].blockers.is_empty());
        proof.matched_lifecycle_coefficient_proofs[0].status =
            "contradicted_matched_lifecycle".to_owned();
        let contradicted =
            level_lifecycle_evidence(&proof, &magnitudes).expect("retained contradiction");
        assert!(!contradicted[0].reversible_static_transform_proven);
        assert!(
            contradicted[0]
                .blockers
                .iter()
                .any(|blocker| blocker.contains("contradicted_matched_lifecycle"))
        );
    }
}
