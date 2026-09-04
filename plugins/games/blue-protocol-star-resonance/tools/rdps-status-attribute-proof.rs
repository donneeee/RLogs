#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorEvent, ActorKind, CanonicalEvent, EntityAttributeValue, EntityRef, EventEnvelope,
    EvidenceSource, RunState, StatusOrigin, StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 29;
const WATCHLIST_SCHEMA_VERSION: u16 = 3;
const DEFAULT_AFTER_WINDOW_MICROS: u64 = 250_000;
const DEFAULT_EXAMPLE_LIMIT: usize = 8;

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    expected_deployment_id: Option<String>,
    expected_game_build: Option<String>,
    watchlist_source_inputs: Option<ProofInputs>,
    watchlist_buff_table: Option<ProofInputArtifact>,
    watchlist_candidates: Vec<ProofWatchlistCandidate>,
    policy: AuditPolicy,
    selected_effect_ids: Vec<i64>,
    reported_effect_ids: Vec<i64>,
    selected_attribute_ids: Vec<i32>,
    non_attributable_context_attribute_ids: Vec<i32>,
    selected_target_entity_uuids: Vec<i64>,
    after_window_micros: u64,
    sessions: Vec<SessionSummary>,
    effects: Vec<EffectReport>,
    wire_additive_equation_systems: Vec<WireAdditiveAttributeReport>,
    reversible_static_coefficient_proofs: Vec<ReversibleStaticCoefficientProof>,
    wire_stack_delta_equation_systems: Vec<WireStackDeltaAttributeReport>,
    reversible_per_stack_coefficient_proofs: Vec<ReversiblePerStackCoefficientProof>,
    matched_lifecycle_coefficient_proofs: Vec<MatchedLifecycleCoefficientProof>,
    candidate_magnitude_proof_reports: Vec<CandidateMagnitudeProofReport>,
}

#[derive(Debug, Serialize)]
struct CompactTransitionSeedBundle {
    schema_version: u16,
    generated_by: &'static str,
    expected_deployment_id: Option<String>,
    expected_game_build: Option<String>,
    policy: &'static str,
    source_rlogs: Vec<String>,
    selected_effect_ids: Vec<i64>,
    selected_attribute_ids: Vec<i32>,
    example_limit: usize,
    exact_single_term_equation_occurrences: u64,
    retained_transition_seeds: usize,
    all_equation_occurrences_retained: bool,
    transitions: Vec<CompactTransitionSeed>,
}

#[derive(Debug, Serialize)]
struct CompactTransitionSeed {
    effect_id: i64,
    attribute_id: i32,
    term: WireAdditiveTerm,
    raw_attribute_delta: i64,
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    wire_capture_sequence: u64,
    wire_observed_micros: u64,
    before_value: i64,
    after_value: i64,
    source_entity_uuids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct ProofWatchlist {
    schema_version: u16,
    deployment_id: String,
    game_build: String,
    source_inputs: ProofInputs,
    buff_table: ProofInputArtifact,
    selected_effect_ids: Vec<i64>,
    reported_effect_ids: Vec<i64>,
    selected_attribute_ids: Vec<i32>,
    non_attributable_context_attribute_ids: Vec<i32>,
    stateful_attribute_ids: Vec<i32>,
    after_window_micros: Option<u64>,
    example_limit: Option<usize>,
    candidates: Vec<ProofWatchlistCandidate>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProofInputs {
    classification: ProofInputArtifact,
    contribution: ProofInputArtifact,
    recount: ProofInputArtifact,
    value_proof: ProofInputArtifact,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProofInputArtifact {
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProofWatchlistCandidate {
    source_rule_id: String,
    source_id: Option<String>,
    #[serde(default)]
    declared_effect_references: Vec<i64>,
    effect_ids: Vec<i64>,
    #[serde(default)]
    rejected_effect_references: Vec<ProofWatchlistRejectedEffectReference>,
    formula_terms: Vec<String>,
    selected_attribute_ids: Vec<i32>,
    required_runtime_evidence: Vec<String>,
    static_value_state: String,
    static_value_proofs: Vec<serde_json::Value>,
    static_blockers: Vec<String>,
    lifecycle_effects: Vec<ProofWatchlistLifecycleEffect>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProofWatchlistRejectedEffectReference {
    effect_id: i64,
    reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProofWatchlistLifecycleEffect {
    effect_id: i64,
    name: Option<String>,
    icon: Option<String>,
    repeat_add_rule: Vec<i64>,
    declared_max_stacks: Option<i64>,
    proof_model: String,
    destroy_param: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_use: &'static str,
    session_scope: &'static str,
    run_scope: &'static str,
    provider_resolution: &'static str,
    actor_metadata_resolution: &'static str,
    before_value: &'static str,
    after_value: &'static str,
    isolation: &'static str,
    attribute_units: &'static str,
    formula_inference: bool,
    unresolved_evidence_is_hidden: bool,
    wire_message_state: &'static str,
    duplicate_status_transitions: &'static str,
    snapshot_status_rows: &'static str,
    wire_net_state: &'static str,
    active_stack_surface: &'static str,
    active_stack_surfaces_generated: bool,
    aggregate_scope: &'static str,
    wire_additive_equations: &'static str,
    reversible_static_coefficient_gate: &'static str,
    wire_stack_delta_equations: &'static str,
    reversible_per_stack_coefficient_gate: &'static str,
    stateful_attribute_exclusions: Vec<i32>,
    non_attributable_context_attributes: Vec<i32>,
    selected_attributes_are_formula_context_not_credit_authority: bool,
    matched_lifecycle_gate: &'static str,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    rlog: String,
    bytes: u64,
    sha256: String,
    deployment_id: String,
    game_build: String,
    protocol_pack_digest: String,
    session_id: String,
    run_ordinals_observed: u32,
    actor_events: u64,
    attribute_events: u64,
    decoded_selected_attribute_values: u64,
    undecodable_selected_attribute_values: u64,
    all_status_events: u64,
    selected_status_events: u64,
}

#[derive(Debug, Serialize)]
struct EffectReport {
    effect_id: i64,
    selected_status_events: u64,
    selected_mechanic_state_changes: u64,
    attributes: Vec<AttributeReport>,
    percent_family_formulas: Vec<PercentFamilyFormulaReport>,
    active_stack_attribute_surfaces: Vec<ActiveStackAttributeSurfaceReport>,
}

#[derive(Debug, Default, Serialize)]
struct WireAdditiveAttributeReport {
    attribute_id: i32,
    wire_messages_with_attribute_update: u64,
    binary_presence_equations: u64,
    equations_containing_reported_effect: u64,
    excluded_nonbinary_mechanic_equations: u64,
    unique_equations: usize,
    conflicting_term_sets: usize,
    equations: Vec<WireAdditiveEquation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct WireAdditiveTerm {
    effect_id: i64,
    origin: Option<OriginSnapshot>,
    level: Option<i32>,
    part_id: Option<i32>,
    stacks: Option<u32>,
    count: Option<i32>,
    signed_presence_delta: i8,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WireAdditiveEquationKey {
    terms: Vec<WireAdditiveTerm>,
    raw_attribute_delta: i64,
}

#[derive(Debug, Default)]
struct WireAdditiveEquationAccumulator {
    count: u64,
    independent_run_contexts: BTreeSet<(String, u32)>,
    target_entity_uuids: BTreeSet<i64>,
    source_entity_uuids: BTreeSet<i64>,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    examples: Vec<WireAdditiveEquationExample>,
}

#[derive(Debug, Serialize)]
struct WireAdditiveEquation {
    terms: Vec<WireAdditiveTerm>,
    raw_attribute_delta: i64,
    count: u64,
    independent_run_contexts: usize,
    target_entity_count: usize,
    source_entity_count: usize,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    examples: Vec<WireAdditiveEquationExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct StaticCoefficientFingerprint {
    effect_id: i64,
    origin: Option<OriginSnapshot>,
    level: Option<i32>,
    part_id: Option<i32>,
    stacks: Option<u32>,
    count: Option<i32>,
}

#[derive(Debug, Default)]
struct StaticCoefficientAccumulator {
    normalized_coefficients: BTreeMap<i64, u64>,
    apply_occurrences: u64,
    remove_occurrences: u64,
    independent_run_contexts: BTreeSet<(String, u32)>,
    target_entity_uuids: BTreeSet<i64>,
    source_entity_uuids: BTreeSet<i64>,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    source_equations: u64,
}

#[derive(Debug, Serialize)]
struct ReversibleStaticCoefficientProof {
    attribute_id: i32,
    fingerprint: StaticCoefficientFingerprint,
    status: &'static str,
    proven_coefficient_units: Option<i64>,
    normalized_coefficient_counts: BTreeMap<i64, u64>,
    apply_occurrences: u64,
    remove_occurrences: u64,
    independent_run_contexts: usize,
    target_entity_count: usize,
    source_entity_count: usize,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    source_equations: u64,
    runtime_eligible_for_rdps: bool,
    blocker: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct StackCoefficientFingerprint {
    effect_id: i64,
    origin: Option<OriginSnapshot>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WireStackDeltaEquationKey {
    fingerprint: StackCoefficientFingerprint,
    signed_stack_delta: i64,
    raw_attribute_delta: i64,
}

#[derive(Debug, Default)]
struct WireStackDeltaEquationAccumulator {
    count: u64,
    independent_run_contexts: BTreeSet<(String, u32)>,
    target_entity_uuids: BTreeSet<i64>,
    source_entity_uuids: BTreeSet<i64>,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    examples: Vec<WireStackDeltaEquationExample>,
}

#[derive(Debug, Default, Serialize)]
struct WireStackDeltaAttributeReport {
    attribute_id: i32,
    wire_messages_with_attribute_update: u64,
    exact_single_effect_stack_equations: u64,
    excluded_missing_stack_count: u64,
    excluded_ambiguous_effect_or_instance_transition: u64,
    unique_equations: usize,
    equations: Vec<WireStackDeltaEquation>,
}

#[derive(Debug, Serialize)]
struct WireStackDeltaEquation {
    fingerprint: StackCoefficientFingerprint,
    signed_stack_delta: i64,
    raw_attribute_delta: i64,
    exact_coefficient_units_per_stack: Option<i64>,
    count: u64,
    independent_run_contexts: usize,
    target_entity_count: usize,
    source_entity_count: usize,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    examples: Vec<WireStackDeltaEquationExample>,
    #[serde(skip)]
    independent_run_context_keys: BTreeSet<(String, u32)>,
    #[serde(skip)]
    target_entity_uuid_keys: BTreeSet<i64>,
    #[serde(skip)]
    source_entity_uuid_keys: BTreeSet<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct WireStackDeltaEquationExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    source_entity_uuid: Option<i64>,
    wire_capture_sequence: u64,
    before_stacks: u32,
    after_stacks: u32,
    before_value: i64,
    after_value: i64,
}

#[derive(Debug, Default)]
struct PerStackCoefficientAccumulator {
    exact_coefficient_counts: BTreeMap<i64, u64>,
    non_integral_equations: u64,
    positive_stack_delta_occurrences: u64,
    negative_stack_delta_occurrences: u64,
    independent_run_contexts: BTreeSet<(String, u32)>,
    target_entity_uuids: BTreeSet<i64>,
    source_entity_uuids: BTreeSet<i64>,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    source_equations: u64,
}

#[derive(Debug, Serialize)]
struct ReversiblePerStackCoefficientProof {
    attribute_id: i32,
    fingerprint: StackCoefficientFingerprint,
    status: &'static str,
    proven_coefficient_units_per_stack: Option<i64>,
    exact_coefficient_counts: BTreeMap<i64, u64>,
    non_integral_equations: u64,
    positive_stack_delta_occurrences: u64,
    negative_stack_delta_occurrences: u64,
    independent_run_contexts: usize,
    target_entity_count: usize,
    source_entity_count: usize,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    source_equations: u64,
    runtime_eligible_for_rdps: bool,
    blocker: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct CandidateMagnitudeProofReport {
    source_rule_id: String,
    source_id: Option<String>,
    formula_terms: Vec<String>,
    required_runtime_evidence: Vec<String>,
    static_value_state: String,
    static_value_proofs: Vec<serde_json::Value>,
    static_blockers: Vec<String>,
    evidence_state: &'static str,
    runtime_eligible_for_rdps: bool,
    surfaces: Vec<CandidateMagnitudeProofSurface>,
}

#[derive(Debug, Serialize)]
struct CandidateMagnitudeProofSurface {
    effect_id: i64,
    attribute_id: i32,
    proof_model: String,
    observed_proof_statuses: Vec<String>,
    proven_coefficient_units: Vec<i64>,
    evidence_records: u64,
    has_exact_reversible_coefficient: bool,
    attribution_role: &'static str,
    runtime_eligible_for_rdps: bool,
}

#[derive(Debug, Clone)]
struct LifecycleTransitionEvidence {
    fingerprint: StaticCoefficientFingerprint,
    raw_attribute_delta: i64,
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    source_entity_uuid: Option<i64>,
    cross_actor: bool,
    wire_capture_sequence: u64,
    before_value: i64,
    after_value: i64,
}

#[derive(Debug, Default)]
struct LifecycleInstanceAccumulator {
    applications: Vec<LifecycleTransitionEvidence>,
    removals: Vec<LifecycleTransitionEvidence>,
}

#[derive(Debug, Default)]
struct LifecycleProofAccumulator {
    exact_coefficient_counts: BTreeMap<i64, u64>,
    exact_pair_count: u64,
    contradictory_pair_count: u64,
    ambiguous_instance_count: u64,
    application_only_instance_count: u64,
    removal_only_instance_count: u64,
    independent_run_contexts: BTreeSet<(String, u32)>,
    target_entity_uuids: BTreeSet<i64>,
    source_entity_uuids: BTreeSet<i64>,
    cross_actor_exact_pairs: u64,
    examples: Vec<MatchedLifecycleExample>,
}

#[derive(Debug, Serialize)]
struct MatchedLifecycleCoefficientProof {
    attribute_id: i32,
    fingerprint: StaticCoefficientFingerprint,
    status: &'static str,
    proven_coefficient_units: Option<i64>,
    exact_coefficient_counts: BTreeMap<i64, u64>,
    exact_pair_count: u64,
    contradictory_pair_count: u64,
    ambiguous_instance_count: u64,
    application_only_instance_count: u64,
    removal_only_instance_count: u64,
    independent_run_contexts: usize,
    target_entity_count: usize,
    source_entity_count: usize,
    cross_actor_exact_pairs: u64,
    runtime_eligible_for_rdps: bool,
    blocker: Option<&'static str>,
    examples: Vec<MatchedLifecycleExample>,
}

#[derive(Debug, Serialize)]
struct MatchedLifecycleExample {
    classification: &'static str,
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    effect_id: i64,
    instance_id: i64,
    fingerprint: StaticCoefficientFingerprint,
    applications: Vec<LifecycleTransitionExample>,
    removals: Vec<LifecycleTransitionExample>,
}

#[derive(Debug, Serialize)]
struct LifecycleTransitionExample {
    raw_attribute_delta: i64,
    source_entity_uuid: Option<i64>,
    cross_actor: bool,
    wire_capture_sequence: u64,
    before_value: i64,
    after_value: i64,
}

#[derive(Debug, Clone, Serialize)]
struct WireAdditiveEquationExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    target_actor_sequence: Option<u64>,
    target_kind: Option<&'static str>,
    target_display_name: Option<String>,
    target_class_id: Option<i32>,
    target_specialization_id: Option<i32>,
    wire_capture_sequence: u64,
    wire_observed_micros: u64,
    status_instances: Vec<WireStatusInstanceEvidence>,
    before_value: i64,
    after_value: i64,
    source_entity_uuids: Vec<i64>,
    source_attribute_values_before: Vec<SourceAttributeValue>,
    source_attribute_values_nearest: Vec<NearestSourceAttributeValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_selected_attribute_values_before: Vec<SelectedAttributeValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    target_selected_attribute_values_before: Vec<SelectedAttributeValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_active_statuses: Vec<SourceActiveStatusEvidence>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    target_active_statuses: Vec<SourceActiveStatusEvidence>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    reported_effect_lifecycle_starts: Vec<ReportedEffectLifecycleStartEvidence>,
}

#[derive(Debug, Clone, Serialize)]
struct WireStatusInstanceEvidence {
    sequence: u64,
    effect_id: i64,
    instance_id: Option<i64>,
    state: &'static str,
    source_entity_uuid: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct SourceAttributeValue {
    source_entity_uuid: i64,
    value: i64,
    sequence: u64,
    age_events: u64,
}

#[derive(Debug, Clone, Serialize)]
struct NearestSourceAttributeValue {
    source_entity_uuid: i64,
    value: i64,
    sequence: u64,
    event_distance: u64,
    relation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SelectedAttributeValue {
    entity_uuid: i64,
    attribute_id: i32,
    value: i64,
    sequence: u64,
    age_events: u64,
}

#[derive(Debug, Clone, Serialize)]
struct SourceActiveStatusEvidence {
    source_entity_uuid: i64,
    effect_id: i64,
    instance_id: Option<i64>,
    origin: Option<OriginSnapshot>,
    level: Option<i32>,
    part_id: Option<i32>,
    stacks: Option<u32>,
    count: Option<i32>,
    provider_entity_uuid: Option<i64>,
    last_observed_micros: u64,
    duration_millis: Option<u64>,
    expires_at_observed_micros: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct ReportedEffectLifecycleStartEvidence {
    effect_id: i64,
    instance_id: Option<i64>,
    sequence: u64,
    observed_micros: u64,
    state: &'static str,
    target_active_statuses: Vec<SourceActiveStatusEvidence>,
    source_active_statuses: Vec<SourceActiveStatusEvidence>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveStatusContextState {
    mechanic: ActiveStatusMechanicState,
    last_observed_micros: u64,
    duration_millis: Option<u64>,
    expires_at_observed_micros: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
struct PercentFamilyFormulaReport {
    family: &'static str,
    final_attribute_id: i32,
    intermediate_attribute_id: i32,
    base_attribute_id: i32,
    raw_extra_add_attribute_id: i32,
    raw_percent_attribute_id: i32,
    raw_extra_percent_attribute_id: i32,
    intermediate_expression: &'static str,
    final_expression: &'static str,
    scale: i64,
    transitions_examined: u64,
    transitions_with_exact_wire_inputs: u64,
    intermediate_exact_delta_matches: u64,
    intermediate_residual_mismatches: u64,
    nearest_intermediate_exact_delta_matches: u64,
    nearest_intermediate_residual_mismatches: u64,
    final_transitions_with_known_extra_percent: u64,
    final_exact_delta_matches: u64,
    final_residual_mismatches: u64,
    final_transitions_with_unknown_extra_percent: u64,
    transitions_with_changed_base: u64,
    aggregates: Vec<PercentFamilyFormulaAggregate>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PercentFamilyFormulaKey {
    state: &'static str,
    stacks: Option<u32>,
    raw_percent_delta_units: i64,
    base_delta_units: i64,
    before_raw_extra_add: Option<i64>,
    after_raw_extra_add: Option<i64>,
    intermediate_delta_units: i64,
    predicted_intermediate_delta_units: i64,
    intermediate_residual_units: i64,
    predicted_nearest_intermediate_delta_units: i64,
    nearest_intermediate_residual_units: i64,
    before_raw_extra_percent: Option<i64>,
    after_raw_extra_percent: Option<i64>,
    final_delta_units: i64,
    predicted_final_delta_units: Option<i64>,
    final_residual_units: Option<i64>,
    provider_resolution: &'static str,
    provider_is_target: Option<bool>,
}

#[derive(Debug, Default)]
struct PercentFamilyFormulaAggregateAccumulator {
    count: u64,
    examples: Vec<PercentFamilyFormulaExample>,
}

#[derive(Debug, Serialize)]
struct PercentFamilyFormulaAggregate {
    state: &'static str,
    stacks: Option<u32>,
    raw_percent_delta_units: i64,
    base_delta_units: i64,
    before_raw_extra_add: Option<i64>,
    after_raw_extra_add: Option<i64>,
    intermediate_delta_units: i64,
    predicted_intermediate_delta_units: i64,
    intermediate_residual_units: i64,
    predicted_nearest_intermediate_delta_units: i64,
    nearest_intermediate_residual_units: i64,
    before_raw_extra_percent: Option<i64>,
    after_raw_extra_percent: Option<i64>,
    final_delta_units: i64,
    predicted_final_delta_units: Option<i64>,
    final_residual_units: Option<i64>,
    provider_resolution: &'static str,
    provider_is_target: Option<bool>,
    count: u64,
    examples: Vec<PercentFamilyFormulaExample>,
}

#[derive(Debug, Clone, Serialize)]
struct PercentFamilyFormulaExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    status_sequence: u64,
    wire_capture_sequence: u64,
    effect_id: i64,
    instance_id: Option<i64>,
    state: &'static str,
    stacks: Option<u32>,
    origin: Option<OriginSnapshot>,
    target_entity_uuid: i64,
    raw_source_entity_uuid: Option<i64>,
    resolved_provider_entity_uuid: Option<i64>,
    provider_resolution: &'static str,
    provider_display_name: Option<String>,
    provider_is_target: Option<bool>,
    before_final_value: i64,
    after_final_value: i64,
    before_intermediate_value: i64,
    after_intermediate_value: i64,
    before_base_add: i64,
    after_base_add: i64,
    before_raw_extra_add: Option<i64>,
    after_raw_extra_add: Option<i64>,
    before_raw_percent: i64,
    after_raw_percent: i64,
    before_raw_extra_percent: Option<i64>,
    after_raw_extra_percent: Option<i64>,
    observed_intermediate_delta: i64,
    predicted_intermediate_delta: i64,
    intermediate_residual_units: i64,
    predicted_nearest_intermediate_delta: i64,
    nearest_intermediate_residual_units: i64,
    observed_final_delta: i64,
    predicted_final_delta: Option<i64>,
    final_residual_units: Option<i64>,
}

#[derive(Debug, Default, Serialize)]
struct AttributeReport {
    attribute_id: i32,
    transitions_examined: u64,
    complete_before_and_after: u64,
    missing_before: u64,
    missing_after_within_window: u64,
    isolated_transitions: u64,
    transitions_with_competing_target_statuses: u64,
    aggregates: Vec<TransitionAggregate>,
}

#[derive(Debug, Serialize)]
struct ActiveStackAttributeSurfaceReport {
    attribute_id: i32,
    attribute_samples: u64,
    samples_with_effect_inactive: u64,
    samples_with_multiple_active_instances: u64,
    samples_with_missing_stack_count: u64,
    exact_single_instance_stack_samples: u64,
    every_observed_stack_has_one_attribute_value: bool,
    stack_value_pairs: Vec<StackAttributeValueAggregate>,
}

#[derive(Debug, Default)]
struct ActiveStackAttributeSurfaceAccumulator {
    attribute_samples: u64,
    samples_with_effect_inactive: u64,
    samples_with_multiple_active_instances: u64,
    samples_with_missing_stack_count: u64,
    exact_single_instance_stack_samples: u64,
    pairs: BTreeMap<(u32, i64), StackAttributeValueAccumulator>,
}

#[derive(Debug, Default)]
struct StackAttributeValueAccumulator {
    count: u64,
    examples: Vec<StackAttributeValueExample>,
}

#[derive(Debug, Serialize)]
struct StackAttributeValueAggregate {
    stacks: u32,
    attribute_value: i64,
    count: u64,
    examples: Vec<StackAttributeValueExample>,
}

#[derive(Debug, Clone, Serialize)]
struct StackAttributeValueExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    attribute_sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    stacks: u32,
    attribute_value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AggregateKey {
    state: &'static str,
    raw_delta_units: i64,
    isolated: bool,
    provider_resolution: &'static str,
    provider_kind: Option<&'static str>,
    provider_class_id: Option<i32>,
    provider_specialization_id: Option<i32>,
    provider_is_target: Option<bool>,
    same_wire_attribute_update: bool,
}

#[derive(Debug, Default)]
struct AggregateAccumulator {
    count: u64,
    examples: Vec<TransitionExample>,
}

#[derive(Debug, Serialize)]
struct TransitionAggregate {
    state: &'static str,
    raw_delta_units: i64,
    isolated: bool,
    provider_resolution: &'static str,
    provider_kind: Option<&'static str>,
    provider_class_id: Option<i32>,
    provider_specialization_id: Option<i32>,
    provider_is_target: Option<bool>,
    same_wire_attribute_update: bool,
    count: u64,
    examples: Vec<TransitionExample>,
}

#[derive(Debug, Clone, Serialize)]
struct TransitionExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    status_sequence: u64,
    status_observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    effect_id: i64,
    instance_id: Option<i64>,
    origin: Option<OriginSnapshot>,
    state: &'static str,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    created_at_millis: Option<i64>,
    raw_source_entity_uuid: Option<i64>,
    resolved_provider_entity_uuid: Option<i64>,
    provider_resolution: &'static str,
    provider_kind: Option<&'static str>,
    provider_display_name: Option<String>,
    provider_class_id: Option<i32>,
    provider_specialization_id: Option<i32>,
    provider_is_target: Option<bool>,
    target_entity_uuid: i64,
    target_actor_sequence: Option<u64>,
    target_kind: Option<&'static str>,
    target_display_name: Option<String>,
    target_class_id: Option<i32>,
    target_specialization_id: Option<i32>,
    attribute_id: i32,
    before_sequence: u64,
    before_observed_micros: u64,
    before_age_micros: u64,
    before_value: i64,
    before_decode: &'static str,
    after_sequence: u64,
    after_observed_micros: u64,
    after_latency_micros: u64,
    after_value: i64,
    after_decode: &'static str,
    raw_delta_units: i64,
    same_wire_attribute_update: bool,
    isolated: bool,
    competing_status_transition_count: usize,
    competing_effect_ids: Vec<i64>,
    competing_status_transitions: Vec<CompetingStatusTransition>,
}

#[derive(Debug, Clone, Serialize)]
struct CompetingStatusTransition {
    sequence: u64,
    effect_id: i64,
    instance_id: Option<i64>,
    state: &'static str,
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct OriginSnapshot {
    source_type_id: i32,
    source_config_id: i64,
}

impl From<StatusOrigin> for OriginSnapshot {
    fn from(value: StatusOrigin) -> Self {
        Self {
            source_type_id: value.source_type_id,
            source_config_id: value.source_config_id,
        }
    }
}

#[derive(Debug, Clone)]
struct ActorSnapshot {
    sequence: u64,
    kind: ActorKind,
    display_name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
}

fn merge_actor_snapshot(
    previous: Option<&ActorSnapshot>,
    sequence: u64,
    actor: &ActorEvent,
) -> ActorSnapshot {
    let mut snapshot = previous.cloned().unwrap_or(ActorSnapshot {
        sequence,
        kind: actor.kind,
        display_name: None,
        class_id: None,
        specialization_id: None,
    });
    let class_changed = actor
        .class_id
        .zip(snapshot.class_id)
        .is_some_and(|(observed, retained)| observed != retained);

    snapshot.sequence = sequence;
    snapshot.kind = actor.kind;
    if let Some(value) = &actor.display_name {
        snapshot.display_name = Some(value.clone());
    }
    if class_changed {
        snapshot.specialization_id = None;
    }
    if let Some(value) = actor.class_id {
        snapshot.class_id = Some(value);
    }
    if let Some(value) = actor.specialization_id {
        snapshot.specialization_id = Some(value);
    }
    snapshot
}

#[derive(Debug, Clone, Copy)]
struct AttributePoint {
    sequence: u64,
    observed_micros: u64,
    value: i64,
    decode: &'static str,
    wire_message: Option<WireMessageKey>,
}

#[derive(Debug, Clone, Copy)]
struct WireAttributeTransition {
    before: AttributePoint,
    after: AttributePoint,
    updated_in_status_wire: bool,
}

#[derive(Debug, Clone)]
struct StatusPoint {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    effect_id: i64,
    instance_id: Option<i64>,
    origin: Option<OriginSnapshot>,
    state: StatusState,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    created_at_millis: Option<i64>,
    source: Option<EntityRef>,
    target: EntityRef,
    wire_message: Option<WireMessageKey>,
    mechanic_state_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ActiveStatusMechanicState {
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    origin: Option<OriginSnapshot>,
    source_entity_uuid: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WireMessageKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Default)]
struct SessionData {
    rlog: String,
    bytes: u64,
    sha256: String,
    deployment_id: String,
    game_build: String,
    protocol_pack_digest: String,
    session_id: Option<String>,
    current_run_ordinal: u32,
    maximum_run_ordinal: u32,
    actor_events: u64,
    attribute_events: u64,
    decoded_selected_attribute_values: u64,
    undecodable_selected_attribute_values: u64,
    all_status_events: u64,
    selected_status_events: u64,
    last_sequence: u64,
    actor_history_by_entity: HashMap<i64, Vec<ActorSnapshot>>,
    owner_by_run_and_direct_entity: HashMap<(u32, i64), i64>,
    attributes: BTreeMap<(u32, i64, i32), Vec<AttributePoint>>,
    statuses_by_target: BTreeMap<(u32, i64), Vec<StatusPoint>>,
    selected_statuses: Vec<StatusPoint>,
    active_status_mechanics: HashMap<(u32, i64, i64, i64), ActiveStatusMechanicState>,
    wire_effect_transition_sequence: BTreeMap<(u32, i64, WireMessageKey, i64), u64>,
}

impl SessionData {
    fn new(
        path: &Path,
        bytes: u64,
        sha256: String,
        deployment_id: String,
        game_build: String,
        protocol_pack_digest: String,
    ) -> Self {
        Self {
            rlog: path.display().to_string(),
            bytes,
            sha256,
            deployment_id,
            game_build,
            protocol_pack_digest,
            ..Self::default()
        }
    }

    fn observe(
        &mut self,
        envelope: &EventEnvelope,
        selected_effects: &BTreeSet<i64>,
        selected_attributes: &BTreeSet<i32>,
        selected_targets: &BTreeSet<i64>,
        expected_deployment_id: Option<&str>,
        expected_game_build: Option<&str>,
    ) -> Result<(), String> {
        if let Some(expected) = expected_deployment_id
            && envelope.region.identity.deployment_id != expected
        {
            return Err(format!(
                "{} contains deployment {} but the proof watchlist requires {expected}",
                self.rlog, envelope.region.identity.deployment_id
            ));
        }
        if let Some(expected) = expected_game_build
            && envelope.region.client_build != expected
        {
            return Err(format!(
                "{} contains client build {} but the proof watchlist requires {expected}",
                self.rlog, envelope.region.client_build
            ));
        }
        if let Some(expected) = &self.session_id {
            if expected != &envelope.session_id {
                return Err(format!(
                    "{} contains multiple sessions: {expected} and {}",
                    self.rlog, envelope.session_id
                ));
            }
        } else {
            self.session_id = Some(envelope.session_id.clone());
        }
        if envelope.sequence < self.last_sequence {
            return Err(format!(
                "{} sequence moved backward from {} to {}",
                self.rlog, self.last_sequence, envelope.sequence
            ));
        }
        self.last_sequence = envelope.sequence;
        let wire_message = wire_message_key(&envelope.provenance.source);

        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return Ok(());
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => self.observe_boundary(*state),
            TimelineEventKind::Actor(actor) => self.observe_actor(envelope.sequence, actor),
            TimelineEventKind::EntityAttributes(event) => {
                self.attribute_events = self.attribute_events.saturating_add(1);
                if !selected_targets.is_empty()
                    && !selected_targets.contains(&event.actor.entity_uuid.0)
                {
                    return Ok(());
                }
                for attribute in &event.attributes {
                    if !selected_attributes.contains(&attribute.attribute_id) {
                        continue;
                    }
                    let Some((value, decode)) = decode_attribute(attribute) else {
                        self.undecodable_selected_attribute_values =
                            self.undecodable_selected_attribute_values.saturating_add(1);
                        continue;
                    };
                    self.decoded_selected_attribute_values =
                        self.decoded_selected_attribute_values.saturating_add(1);
                    self.attributes
                        .entry((
                            self.current_run_ordinal,
                            event.actor.entity_uuid.0,
                            attribute.attribute_id,
                        ))
                        .or_default()
                        .push(AttributePoint {
                            sequence: envelope.sequence,
                            observed_micros: envelope.time.observed_micros,
                            value,
                            decode,
                            wire_message,
                        });
                }
            }
            TimelineEventKind::Status(status) => {
                self.all_status_events = self.all_status_events.saturating_add(1);
                if !selected_targets.is_empty()
                    && !selected_targets.contains(&status.target.entity_uuid.0)
                {
                    return Ok(());
                }
                let status_key = (
                    self.current_run_ordinal,
                    status.target.entity_uuid.0,
                    status.effect.0,
                    status.instance_id.map(|value| value.0).unwrap_or(i64::MIN),
                );
                let previous = self.active_status_mechanics.get(&status_key).copied();
                let mechanic_state_changed = match status.state {
                    StatusState::Removed | StatusState::Consumed if status.stacks == Some(0) => {
                        self.active_status_mechanics.remove(&status_key).is_some()
                    }
                    StatusState::Applied
                    | StatusState::Refreshed
                    | StatusState::Stacked
                    | StatusState::Consumed => {
                        let next = ActiveStatusMechanicState {
                            stacks: status.stacks.or(previous.and_then(|value| value.stacks)),
                            level: status.level.or(previous.and_then(|value| value.level)),
                            part_id: status.part_id.or(previous.and_then(|value| value.part_id)),
                            count: status.count.or(previous.and_then(|value| value.count)),
                            origin: status
                                .origin
                                .map(Into::into)
                                .or(previous.and_then(|value| value.origin)),
                            source_entity_uuid: status
                                .source
                                .map(|value| value.entity_uuid.0)
                                .or(previous.and_then(|value| value.source_entity_uuid)),
                        };
                        self.active_status_mechanics.insert(status_key, next);
                        previous != Some(next)
                    }
                    StatusState::Removed => {
                        self.active_status_mechanics.remove(&status_key).is_some()
                    }
                };
                let point = StatusPoint {
                    rlog: self.rlog.clone(),
                    session_id: envelope.session_id.clone(),
                    run_ordinal: self.current_run_ordinal,
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    effect_id: status.effect.0,
                    instance_id: status.instance_id.map(|value| value.0),
                    origin: status.origin.map(Into::into),
                    state: status.state,
                    stacks: status.stacks,
                    duration_millis: status.duration_millis,
                    level: status.level,
                    part_id: status.part_id,
                    count: status.count,
                    created_at_millis: status.created_at_millis,
                    source: status.source,
                    target: status.target,
                    wire_message,
                    mechanic_state_changed,
                };
                self.statuses_by_target
                    .entry((self.current_run_ordinal, status.target.entity_uuid.0))
                    .or_default()
                    .push(point.clone());
                if selected_effects.contains(&status.effect.0) {
                    self.selected_status_events = self.selected_status_events.saturating_add(1);
                    self.selected_statuses.push(point);
                }
            }
            TimelineEventKind::Damage(damage) => {
                self.observe_owner_link(damage.source, damage.direct_source)
            }
            TimelineEventKind::Healing(healing) => {
                self.observe_owner_link(healing.source, healing.direct_source)
            }
            _ => {}
        }
        Ok(())
    }

    fn observe_boundary(&mut self, state: RunState) {
        match state {
            RunState::Entered => {
                self.current_run_ordinal = self.current_run_ordinal.saturating_add(1);
                self.maximum_run_ordinal = self.maximum_run_ordinal.max(self.current_run_ordinal);
            }
            RunState::Started if self.current_run_ordinal == 0 => {
                self.current_run_ordinal = 1;
                self.maximum_run_ordinal = 1;
            }
            _ => {}
        }
    }

    fn observe_actor(&mut self, sequence: u64, actor: &ActorEvent) {
        self.actor_events = self.actor_events.saturating_add(1);
        let history = self
            .actor_history_by_entity
            .entry(actor.actor.entity_uuid.0)
            .or_default();
        let snapshot = merge_actor_snapshot(history.last(), sequence, actor);
        let changed = history.last().is_none_or(|previous| {
            previous.kind != snapshot.kind
                || previous.display_name != snapshot.display_name
                || previous.class_id != snapshot.class_id
                || previous.specialization_id != snapshot.specialization_id
        });
        if changed {
            history.push(snapshot);
        }
    }

    fn observe_owner_link(&mut self, owner: EntityRef, direct_source: Option<EntityRef>) {
        if let Some(direct) = direct_source.filter(|direct| direct.entity_uuid != owner.entity_uuid)
        {
            self.owner_by_run_and_direct_entity
                .entry((self.current_run_ordinal, direct.entity_uuid.0))
                .or_insert(owner.entity_uuid.0);
        }
    }

    fn finalize_status_wire_changes(&mut self) {
        let mut representatives = BTreeMap::new();
        for ((run_ordinal, target_entity_uuid), statuses) in &self.statuses_by_target {
            let mut active = BTreeMap::<(i64, i64), ActiveStatusMechanicState>::new();
            let mut index = 0;
            while index < statuses.len() {
                let Some(message) = statuses[index].wire_message else {
                    apply_status_point(&mut active, &statuses[index]);
                    index += 1;
                    continue;
                };
                let end = statuses[index..]
                    .partition_point(|status| status.wire_message == Some(message))
                    + index;
                let effects = statuses[index..end]
                    .iter()
                    .map(|status| status.effect_id)
                    .collect::<BTreeSet<_>>();
                let before = effects
                    .iter()
                    .map(|effect_id| (*effect_id, active_effect_state(&active, *effect_id)))
                    .collect::<BTreeMap<_, _>>();
                for status in &statuses[index..end] {
                    apply_status_point(&mut active, status);
                }
                for effect_id in effects {
                    if before.get(&effect_id) != Some(&active_effect_state(&active, effect_id)) {
                        let sequence = statuses[index..end]
                            .iter()
                            .rev()
                            .find(|status| status.effect_id == effect_id)
                            .expect("wire effect has a representative")
                            .sequence;
                        representatives.insert(
                            (*run_ordinal, *target_entity_uuid, message, effect_id),
                            sequence,
                        );
                    }
                }
                index = end;
            }
        }
        self.wire_effect_transition_sequence = representatives;
    }

    fn is_effective_status_transition(&self, status: &StatusPoint) -> bool {
        let Some(message) = status.wire_message else {
            return status.mechanic_state_changed;
        };
        self.wire_effect_transition_sequence
            .get(&(
                status.run_ordinal,
                status.target.entity_uuid.0,
                message,
                status.effect_id,
            ))
            .is_some_and(|sequence| *sequence == status.sequence)
    }

    fn actor_snapshot_at(&self, entity_uuid: i64, sequence: u64) -> Option<&ActorSnapshot> {
        let history = self.actor_history_by_entity.get(&entity_uuid)?;
        let split = history.partition_point(|snapshot| snapshot.sequence <= sequence);
        split.checked_sub(1).and_then(|index| history.get(index))
    }

    fn provider_at(&self, status: &StatusPoint) -> ProviderEvidence<'_> {
        let Some(source) = status.source else {
            return ProviderEvidence::unresolved();
        };
        let raw_entity = source.entity_uuid.0;
        let resolved_entity = self
            .owner_by_run_and_direct_entity
            .get(&(status.run_ordinal, raw_entity))
            .copied()
            .unwrap_or(raw_entity);
        let snapshot = self
            .actor_snapshot_at(resolved_entity, status.sequence)
            .or_else(|| self.actor_snapshot_at(raw_entity, status.sequence));
        let Some(snapshot) = snapshot else {
            return ProviderEvidence {
                resolved_entity_uuid: Some(resolved_entity),
                resolution: "unresolved_actor_metadata",
                snapshot: None,
            };
        };
        let resolution = match snapshot.kind {
            ActorKind::Player if resolved_entity == raw_entity => "direct_player",
            ActorKind::Player => "owner_link_within_run",
            _ => "non_player",
        };
        ProviderEvidence {
            resolved_entity_uuid: Some(resolved_entity),
            resolution,
            snapshot: Some(snapshot),
        }
    }

    fn active_statuses_at(
        &self,
        run_ordinal: u32,
        target_entity_uuid: i64,
        sequence: u64,
        observed_micros: u64,
    ) -> Vec<SourceActiveStatusEvidence> {
        let Some(statuses) = self
            .statuses_by_target
            .get(&(run_ordinal, target_entity_uuid))
        else {
            return Vec::new();
        };
        let mut active = BTreeMap::<(i64, i64), ActiveStatusContextState>::new();
        for status in statuses
            .iter()
            .take_while(|status| status.sequence <= sequence)
        {
            let key = (status.effect_id, status.instance_id.unwrap_or(i64::MIN));
            let previous = active.get(&key).copied();
            match status.state {
                StatusState::Removed | StatusState::Consumed if status.stacks == Some(0) => {
                    active.remove(&key);
                }
                StatusState::Applied
                | StatusState::Refreshed
                | StatusState::Stacked
                | StatusState::Consumed => {
                    let previous_mechanic = previous.map(|value| value.mechanic);
                    let duration_millis = status
                        .duration_millis
                        .or(previous.and_then(|value| value.duration_millis));
                    let expires_at_observed_micros = duration_millis
                        .and_then(|duration| {
                            status
                                .observed_micros
                                .checked_add(duration.saturating_mul(1_000))
                        })
                        .or(previous.and_then(|value| value.expires_at_observed_micros));
                    active.insert(
                        key,
                        ActiveStatusContextState {
                            mechanic: ActiveStatusMechanicState {
                                stacks: status
                                    .stacks
                                    .or(previous_mechanic.and_then(|value| value.stacks)),
                                level: status
                                    .level
                                    .or(previous_mechanic.and_then(|value| value.level)),
                                part_id: status
                                    .part_id
                                    .or(previous_mechanic.and_then(|value| value.part_id)),
                                count: status
                                    .count
                                    .or(previous_mechanic.and_then(|value| value.count)),
                                origin: status
                                    .origin
                                    .or(previous_mechanic.and_then(|value| value.origin)),
                                source_entity_uuid: status
                                    .source
                                    .map(|value| value.entity_uuid.0)
                                    .or(previous_mechanic
                                        .and_then(|value| value.source_entity_uuid)),
                            },
                            last_observed_micros: status.observed_micros,
                            duration_millis,
                            expires_at_observed_micros,
                        },
                    );
                }
                StatusState::Removed => {
                    active.remove(&key);
                }
            }
        }
        active
            .into_iter()
            .filter(|(_, state)| {
                state
                    .expires_at_observed_micros
                    .is_none_or(|expires_at| observed_micros <= expires_at)
            })
            .map(
                |((effect_id, instance_key), state)| SourceActiveStatusEvidence {
                    source_entity_uuid: target_entity_uuid,
                    effect_id,
                    instance_id: (instance_key != i64::MIN).then_some(instance_key),
                    origin: state.mechanic.origin,
                    level: state.mechanic.level,
                    part_id: state.mechanic.part_id,
                    stacks: state.mechanic.stacks,
                    count: state.mechanic.count,
                    provider_entity_uuid: state.mechanic.source_entity_uuid,
                    last_observed_micros: state.last_observed_micros,
                    duration_millis: state.duration_millis,
                    expires_at_observed_micros: state.expires_at_observed_micros,
                },
            )
            .collect()
    }

    fn lifecycle_start_for(&self, status: &StatusPoint) -> Option<&StatusPoint> {
        self.statuses_by_target
            .get(&(status.run_ordinal, status.target.entity_uuid.0))?
            .iter()
            .rev()
            .find(|candidate| {
                candidate.sequence <= status.sequence
                    && candidate.effect_id == status.effect_id
                    && candidate.instance_id == status.instance_id
                    && matches!(
                        candidate.state,
                        StatusState::Applied
                            | StatusState::Refreshed
                            | StatusState::Stacked
                            | StatusState::Consumed
                    )
            })
    }

    fn attribute_transition_in_status_wire(
        &self,
        status: &StatusPoint,
        attribute_id: i32,
    ) -> Option<WireAttributeTransition> {
        let message = status.wire_message?;
        let points = self.attributes.get(&(
            status.run_ordinal,
            status.target.entity_uuid.0,
            attribute_id,
        ))?;
        let split = points.partition_point(|point| point.sequence <= status.sequence);
        let before = points[..split]
            .iter()
            .rev()
            .find(|point| point.wire_message != Some(message))
            .copied()?;
        let same_wire = points
            .iter()
            .rev()
            .find(|point| point.wire_message == Some(message))
            .copied();
        Some(WireAttributeTransition {
            before,
            after: same_wire.unwrap_or(before),
            updated_in_status_wire: same_wire.is_some(),
        })
    }

    fn summary(&self) -> SessionSummary {
        SessionSummary {
            rlog: self.rlog.clone(),
            bytes: self.bytes,
            sha256: self.sha256.clone(),
            deployment_id: self.deployment_id.clone(),
            game_build: self.game_build.clone(),
            protocol_pack_digest: self.protocol_pack_digest.clone(),
            session_id: self.session_id.clone().unwrap_or_default(),
            run_ordinals_observed: self.maximum_run_ordinal,
            actor_events: self.actor_events,
            attribute_events: self.attribute_events,
            decoded_selected_attribute_values: self.decoded_selected_attribute_values,
            undecodable_selected_attribute_values: self.undecodable_selected_attribute_values,
            all_status_events: self.all_status_events,
            selected_status_events: self.selected_status_events,
        }
    }
}

fn active_effect_state(
    active: &BTreeMap<(i64, i64), ActiveStatusMechanicState>,
    effect_id: i64,
) -> Vec<ActiveStatusMechanicState> {
    let mut states = active
        .iter()
        .filter_map(|((active_effect_id, _), state)| {
            (*active_effect_id == effect_id).then_some(*state)
        })
        .collect::<Vec<_>>();
    states.sort_unstable();
    states
}

fn apply_status_point(
    active: &mut BTreeMap<(i64, i64), ActiveStatusMechanicState>,
    status: &StatusPoint,
) {
    let key = (status.effect_id, status.instance_id.unwrap_or(i64::MIN));
    let previous = active.get(&key).copied();
    match status.state {
        StatusState::Removed | StatusState::Consumed if status.stacks == Some(0) => {
            active.remove(&key);
        }
        StatusState::Applied
        | StatusState::Refreshed
        | StatusState::Stacked
        | StatusState::Consumed => {
            active.insert(
                key,
                ActiveStatusMechanicState {
                    stacks: status.stacks.or(previous.and_then(|value| value.stacks)),
                    level: status.level.or(previous.and_then(|value| value.level)),
                    part_id: status.part_id.or(previous.and_then(|value| value.part_id)),
                    count: status.count.or(previous.and_then(|value| value.count)),
                    origin: status.origin.or(previous.and_then(|value| value.origin)),
                    source_entity_uuid: status
                        .source
                        .map(|value| value.entity_uuid.0)
                        .or(previous.and_then(|value| value.source_entity_uuid)),
                },
            );
        }
        StatusState::Removed => {
            active.remove(&key);
        }
    }
}

fn active_effect_instances_at(
    statuses: &[&StatusPoint],
    attribute: &AttributePoint,
) -> BTreeMap<i64, Option<u32>> {
    let sequence_end = statuses.partition_point(|status| status.sequence <= attribute.sequence);
    let wire_end = attribute.wire_message.and_then(|message| {
        statuses
            .iter()
            .rposition(|status| status.wire_message == Some(message))
            .map(|index| index + 1)
    });
    let end = wire_end.unwrap_or(sequence_end).max(sequence_end);
    let mut active = BTreeMap::<i64, Option<u32>>::new();
    for status in &statuses[..end] {
        let instance_id = status.instance_id.unwrap_or(i64::MIN);
        update_active_effect_instance(&mut active, instance_id, status.state, status.stacks);
    }
    active
}

fn update_active_effect_instance(
    active: &mut BTreeMap<i64, Option<u32>>,
    instance_id: i64,
    state: StatusState,
    stacks: Option<u32>,
) {
    match state {
        StatusState::Removed | StatusState::Consumed if stacks == Some(0) => {
            active.remove(&instance_id);
        }
        StatusState::Applied
        | StatusState::Refreshed
        | StatusState::Stacked
        | StatusState::Consumed => {
            let previous = active.get(&instance_id).copied().flatten();
            active.insert(instance_id, stacks.or(previous));
        }
        StatusState::Removed => {
            active.remove(&instance_id);
        }
    }
}

fn wire_additive_term(
    effect_id: i64,
    state: ActiveStatusMechanicState,
    signed_presence_delta: i8,
) -> WireAdditiveTerm {
    WireAdditiveTerm {
        effect_id,
        origin: state.origin,
        level: state.level,
        part_id: state.part_id,
        stacks: state.stacks,
        count: state.count,
        signed_presence_delta,
    }
}

fn wire_additive_attribute_report(
    sessions: &[SessionData],
    attribute_id: i32,
    selected_attribute_ids: &BTreeSet<i32>,
    reported_effects: &BTreeSet<i64>,
    example_limit: usize,
    include_source_status_context: bool,
    include_target_status_context: bool,
    include_selected_attribute_context: bool,
) -> WireAdditiveAttributeReport {
    let mut report = WireAdditiveAttributeReport {
        attribute_id,
        ..WireAdditiveAttributeReport::default()
    };
    let mut equations = BTreeMap::<WireAdditiveEquationKey, WireAdditiveEquationAccumulator>::new();
    let mut results_by_term_set = BTreeMap::<Vec<WireAdditiveTerm>, BTreeSet<i64>>::new();

    for session in sessions {
        let session_id = session.session_id.clone().unwrap_or_default();
        for ((run_ordinal, target_entity_uuid), statuses) in &session.statuses_by_target {
            let Some(attribute_points) =
                session
                    .attributes
                    .get(&(*run_ordinal, *target_entity_uuid, attribute_id))
            else {
                continue;
            };
            let mut active = BTreeMap::<(i64, i64), ActiveStatusMechanicState>::new();
            let mut index = 0;
            while index < statuses.len() {
                let Some(message) = statuses[index].wire_message else {
                    apply_status_point(&mut active, &statuses[index]);
                    index += 1;
                    continue;
                };
                let end = statuses[index..]
                    .partition_point(|status| status.wire_message == Some(message))
                    + index;
                let first_sequence = statuses[index].sequence;
                let Some(after) = attribute_points
                    .iter()
                    .rev()
                    .find(|point| point.wire_message == Some(message))
                    .copied()
                else {
                    for status in &statuses[index..end] {
                        apply_status_point(&mut active, status);
                    }
                    index = end;
                    continue;
                };
                let Some(before) = attribute_points
                    .iter()
                    .rev()
                    .find(|point| {
                        point.sequence < first_sequence && point.wire_message != Some(message)
                    })
                    .copied()
                else {
                    for status in &statuses[index..end] {
                        apply_status_point(&mut active, status);
                    }
                    index = end;
                    continue;
                };
                report.wire_messages_with_attribute_update =
                    report.wire_messages_with_attribute_update.saturating_add(1);

                let before_active = active.clone();
                for status in &statuses[index..end] {
                    apply_status_point(&mut active, status);
                }
                let changed_effects = statuses[index..end]
                    .iter()
                    .map(|status| status.effect_id)
                    .collect::<BTreeSet<_>>();
                let mut terms = Vec::new();
                let mut source_entity_uuids = BTreeSet::new();
                let mut nonbinary = false;
                for effect_id in changed_effects {
                    let before_state = active_effect_state(&before_active, effect_id);
                    let after_state = active_effect_state(&active, effect_id);
                    if before_state == after_state {
                        continue;
                    }
                    let (state, signed_presence_delta) =
                        match (before_state.as_slice(), after_state.as_slice()) {
                            ([], [state]) => (*state, 1),
                            ([state], []) => (*state, -1),
                            _ => {
                                nonbinary = true;
                                break;
                            }
                        };
                    if let Some(source_entity_uuid) = state.source_entity_uuid {
                        source_entity_uuids.insert(source_entity_uuid);
                    }
                    terms.push(wire_additive_term(effect_id, state, signed_presence_delta));
                }
                if nonbinary {
                    report.excluded_nonbinary_mechanic_equations = report
                        .excluded_nonbinary_mechanic_equations
                        .saturating_add(1);
                    index = end;
                    continue;
                }
                if terms.is_empty() {
                    index = end;
                    continue;
                }
                terms.sort_unstable();
                let causal_effect_ids = terms
                    .iter()
                    .map(|term| term.effect_id)
                    .collect::<BTreeSet<_>>();
                report.binary_presence_equations =
                    report.binary_presence_equations.saturating_add(1);
                if terms
                    .iter()
                    .any(|term| reported_effects.contains(&term.effect_id))
                {
                    report.equations_containing_reported_effect = report
                        .equations_containing_reported_effect
                        .saturating_add(1);
                }
                let raw_attribute_delta = after.value.saturating_sub(before.value);
                results_by_term_set
                    .entry(terms.clone())
                    .or_default()
                    .insert(raw_attribute_delta);
                let equation = equations
                    .entry(WireAdditiveEquationKey {
                        terms,
                        raw_attribute_delta,
                    })
                    .or_default();
                equation.count = equation.count.saturating_add(1);
                equation
                    .independent_run_contexts
                    .insert((session_id.clone(), *run_ordinal));
                equation.target_entity_uuids.insert(*target_entity_uuid);
                equation
                    .source_entity_uuids
                    .extend(source_entity_uuids.iter().copied());
                if source_entity_uuids.is_empty() {
                    equation.missing_source_occurrences =
                        equation.missing_source_occurrences.saturating_add(1);
                } else {
                    if source_entity_uuids
                        .iter()
                        .any(|source| source != target_entity_uuid)
                    {
                        equation.cross_actor_occurrences =
                            equation.cross_actor_occurrences.saturating_add(1);
                    }
                    if source_entity_uuids.contains(target_entity_uuid) {
                        equation.self_source_occurrences =
                            equation.self_source_occurrences.saturating_add(1);
                    }
                }
                if equation.examples.len() < example_limit {
                    let contains_reported_effect = causal_effect_ids
                        .iter()
                        .any(|effect_id| reported_effects.contains(effect_id));
                    let source_attribute_values_before = source_entity_uuids
                        .iter()
                        .filter_map(|source_entity_uuid| {
                            let points = session.attributes.get(&(
                                *run_ordinal,
                                *source_entity_uuid,
                                attribute_id,
                            ))?;
                            let point = points.iter().rev().find(|point| {
                                point.sequence < first_sequence
                                    && point.wire_message != Some(message)
                            })?;
                            Some(SourceAttributeValue {
                                source_entity_uuid: *source_entity_uuid,
                                value: point.value,
                                sequence: point.sequence,
                                age_events: first_sequence.saturating_sub(point.sequence),
                            })
                        })
                        .collect();
                    let source_attribute_values_nearest = source_entity_uuids
                        .iter()
                        .filter_map(|source_entity_uuid| {
                            let points = session.attributes.get(&(
                                *run_ordinal,
                                *source_entity_uuid,
                                attribute_id,
                            ))?;
                            let point = points
                                .iter()
                                .min_by_key(|point| point.sequence.abs_diff(first_sequence))?;
                            Some(NearestSourceAttributeValue {
                                source_entity_uuid: *source_entity_uuid,
                                value: point.value,
                                sequence: point.sequence,
                                event_distance: point.sequence.abs_diff(first_sequence),
                                relation: if point.sequence < first_sequence {
                                    "before"
                                } else if point.sequence > first_sequence {
                                    "after"
                                } else {
                                    "same-event"
                                },
                            })
                        })
                        .collect();
                    let source_selected_attribute_values_before =
                        if include_selected_attribute_context && contains_reported_effect {
                            source_entity_uuids
                                .iter()
                                .flat_map(|source_entity_uuid| {
                                    selected_attribute_ids.iter().filter_map(
                                        |selected_attribute_id| {
                                            let points = session.attributes.get(&(
                                                *run_ordinal,
                                                *source_entity_uuid,
                                                *selected_attribute_id,
                                            ))?;
                                            let point = points.iter().rev().find(|point| {
                                                point.sequence < first_sequence
                                                    && point.wire_message != Some(message)
                                            })?;
                                            Some(SelectedAttributeValue {
                                                entity_uuid: *source_entity_uuid,
                                                attribute_id: *selected_attribute_id,
                                                value: point.value,
                                                sequence: point.sequence,
                                                age_events: first_sequence
                                                    .saturating_sub(point.sequence),
                                            })
                                        },
                                    )
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                    let target_selected_attribute_values_before =
                        if include_selected_attribute_context && contains_reported_effect {
                            selected_attribute_ids
                                .iter()
                                .filter_map(|selected_attribute_id| {
                                    let points = session.attributes.get(&(
                                        *run_ordinal,
                                        *target_entity_uuid,
                                        *selected_attribute_id,
                                    ))?;
                                    let point = points.iter().rev().find(|point| {
                                        point.sequence < first_sequence
                                            && point.wire_message != Some(message)
                                    })?;
                                    Some(SelectedAttributeValue {
                                        entity_uuid: *target_entity_uuid,
                                        attribute_id: *selected_attribute_id,
                                        value: point.value,
                                        sequence: point.sequence,
                                        age_events: first_sequence.saturating_sub(point.sequence),
                                    })
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                    let source_active_statuses =
                        if include_source_status_context && contains_reported_effect {
                            source_entity_uuids
                                .iter()
                                .flat_map(|source_entity_uuid| {
                                    session.active_statuses_at(
                                        *run_ordinal,
                                        *source_entity_uuid,
                                        first_sequence,
                                        after.observed_micros,
                                    )
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                    let target_active_statuses = if include_target_status_context
                        && causal_effect_ids
                            .iter()
                            .any(|effect_id| reported_effects.contains(effect_id))
                    {
                        session.active_statuses_at(
                            *run_ordinal,
                            *target_entity_uuid,
                            first_sequence,
                            after.observed_micros,
                        )
                    } else {
                        Vec::new()
                    };
                    let reported_effect_lifecycle_starts = if include_target_status_context {
                        statuses[index..end]
                            .iter()
                            .filter(|status| reported_effects.contains(&status.effect_id))
                            .filter_map(|status| {
                                let start = session.lifecycle_start_for(status)?;
                                Some(ReportedEffectLifecycleStartEvidence {
                                    effect_id: status.effect_id,
                                    instance_id: status.instance_id,
                                    sequence: start.sequence,
                                    observed_micros: start.observed_micros,
                                    state: status_state(start.state),
                                    target_active_statuses: session.active_statuses_at(
                                        start.run_ordinal,
                                        start.target.entity_uuid.0,
                                        start.sequence,
                                        start.observed_micros,
                                    ),
                                    source_active_statuses: start
                                        .source
                                        .map(|source| {
                                            session.active_statuses_at(
                                                start.run_ordinal,
                                                source.entity_uuid.0,
                                                start.sequence,
                                                start.observed_micros,
                                            )
                                        })
                                        .unwrap_or_default(),
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    equation.examples.push(WireAdditiveEquationExample {
                        rlog: session.rlog.clone(),
                        session_id: session_id.clone(),
                        run_ordinal: *run_ordinal,
                        target_entity_uuid: *target_entity_uuid,
                        target_actor_sequence: session
                            .actor_snapshot_at(*target_entity_uuid, first_sequence)
                            .map(|snapshot| snapshot.sequence),
                        target_kind: session
                            .actor_snapshot_at(*target_entity_uuid, first_sequence)
                            .map(|snapshot| actor_kind(snapshot.kind)),
                        target_display_name: session
                            .actor_snapshot_at(*target_entity_uuid, first_sequence)
                            .and_then(|snapshot| snapshot.display_name.clone()),
                        target_class_id: session
                            .actor_snapshot_at(*target_entity_uuid, first_sequence)
                            .and_then(|snapshot| snapshot.class_id),
                        target_specialization_id: session
                            .actor_snapshot_at(*target_entity_uuid, first_sequence)
                            .and_then(|snapshot| snapshot.specialization_id),
                        wire_capture_sequence: message.capture_sequence,
                        wire_observed_micros: after.observed_micros,
                        status_instances: statuses[index..end]
                            .iter()
                            .filter(|status| causal_effect_ids.contains(&status.effect_id))
                            .map(|status| WireStatusInstanceEvidence {
                                sequence: status.sequence,
                                effect_id: status.effect_id,
                                instance_id: status.instance_id,
                                state: status_state(status.state),
                                source_entity_uuid: status
                                    .source
                                    .map(|source| source.entity_uuid.0),
                            })
                            .collect(),
                        before_value: before.value,
                        after_value: after.value,
                        source_entity_uuids: source_entity_uuids.into_iter().collect(),
                        source_attribute_values_before,
                        source_attribute_values_nearest,
                        source_selected_attribute_values_before,
                        target_selected_attribute_values_before,
                        source_active_statuses,
                        target_active_statuses,
                        reported_effect_lifecycle_starts,
                    });
                }
                index = end;
            }
        }
    }

    report.unique_equations = equations.len();
    report.conflicting_term_sets = results_by_term_set
        .values()
        .filter(|results| results.len() > 1)
        .count();
    report.equations = equations
        .into_iter()
        .map(|(key, value)| WireAdditiveEquation {
            terms: key.terms,
            raw_attribute_delta: key.raw_attribute_delta,
            count: value.count,
            independent_run_contexts: value.independent_run_contexts.len(),
            target_entity_count: value.target_entity_uuids.len(),
            source_entity_count: value.source_entity_uuids.len(),
            cross_actor_occurrences: value.cross_actor_occurrences,
            self_source_occurrences: value.self_source_occurrences,
            missing_source_occurrences: value.missing_source_occurrences,
            examples: value.examples,
        })
        .collect();
    report
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackTransitionExclusion {
    MissingStackCount,
    AmbiguousEffectOrInstance,
}

fn stack_coefficient_fingerprint(
    effect_id: i64,
    state: ActiveStatusMechanicState,
) -> StackCoefficientFingerprint {
    StackCoefficientFingerprint {
        effect_id,
        origin: state.origin,
        level: state.level,
        part_id: state.part_id,
        count: state.count,
    }
}

fn exact_single_instance_stack_transition(
    effect_id: i64,
    before: &[ActiveStatusMechanicState],
    after: &[ActiveStatusMechanicState],
) -> Result<Option<(StackCoefficientFingerprint, u32, u32, Option<i64>)>, StackTransitionExclusion>
{
    let (state, before_stacks, after_stacks) = match (before, after) {
        ([], [after_state]) => (
            *after_state,
            0,
            after_state
                .stacks
                .ok_or(StackTransitionExclusion::MissingStackCount)?,
        ),
        ([before_state], []) => (
            *before_state,
            before_state
                .stacks
                .ok_or(StackTransitionExclusion::MissingStackCount)?,
            0,
        ),
        ([before_state], [after_state]) => {
            let before_fingerprint = stack_coefficient_fingerprint(effect_id, *before_state);
            let after_fingerprint = stack_coefficient_fingerprint(effect_id, *after_state);
            if before_fingerprint != after_fingerprint
                || before_state.source_entity_uuid != after_state.source_entity_uuid
            {
                return Err(StackTransitionExclusion::AmbiguousEffectOrInstance);
            }
            (
                *after_state,
                before_state
                    .stacks
                    .ok_or(StackTransitionExclusion::MissingStackCount)?,
                after_state
                    .stacks
                    .ok_or(StackTransitionExclusion::MissingStackCount)?,
            )
        }
        _ => return Err(StackTransitionExclusion::AmbiguousEffectOrInstance),
    };
    if before_stacks == after_stacks {
        return Ok(None);
    }
    Ok(Some((
        stack_coefficient_fingerprint(effect_id, state),
        before_stacks,
        after_stacks,
        state.source_entity_uuid,
    )))
}

fn exact_coefficient_per_stack(raw_attribute_delta: i64, signed_stack_delta: i64) -> Option<i64> {
    if signed_stack_delta == 0 || raw_attribute_delta % signed_stack_delta != 0 {
        return None;
    }
    Some(raw_attribute_delta / signed_stack_delta)
}

fn wire_stack_delta_attribute_report(
    sessions: &[SessionData],
    attribute_id: i32,
    reported_effects: &BTreeSet<i64>,
    example_limit: usize,
) -> WireStackDeltaAttributeReport {
    let mut report = WireStackDeltaAttributeReport {
        attribute_id,
        ..WireStackDeltaAttributeReport::default()
    };
    let mut equations =
        BTreeMap::<WireStackDeltaEquationKey, WireStackDeltaEquationAccumulator>::new();

    for session in sessions {
        let session_id = session.session_id.clone().unwrap_or_default();
        for ((run_ordinal, target_entity_uuid), statuses) in &session.statuses_by_target {
            let Some(attribute_points) =
                session
                    .attributes
                    .get(&(*run_ordinal, *target_entity_uuid, attribute_id))
            else {
                continue;
            };
            let mut active = BTreeMap::<(i64, i64), ActiveStatusMechanicState>::new();
            let mut index = 0;
            while index < statuses.len() {
                let Some(message) = statuses[index].wire_message else {
                    apply_status_point(&mut active, &statuses[index]);
                    index += 1;
                    continue;
                };
                let end = statuses[index..]
                    .partition_point(|status| status.wire_message == Some(message))
                    + index;
                let first_sequence = statuses[index].sequence;
                let Some(after_attribute) = attribute_points
                    .iter()
                    .rev()
                    .find(|point| point.wire_message == Some(message))
                    .copied()
                else {
                    for status in &statuses[index..end] {
                        apply_status_point(&mut active, status);
                    }
                    index = end;
                    continue;
                };
                let Some(before_attribute) = attribute_points
                    .iter()
                    .rev()
                    .find(|point| {
                        point.sequence < first_sequence && point.wire_message != Some(message)
                    })
                    .copied()
                else {
                    for status in &statuses[index..end] {
                        apply_status_point(&mut active, status);
                    }
                    index = end;
                    continue;
                };
                report.wire_messages_with_attribute_update =
                    report.wire_messages_with_attribute_update.saturating_add(1);

                let before_active = active.clone();
                for status in &statuses[index..end] {
                    apply_status_point(&mut active, status);
                }
                let changed_effects = statuses[index..end]
                    .iter()
                    .map(|status| status.effect_id)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .filter(|effect_id| {
                        active_effect_state(&before_active, *effect_id)
                            != active_effect_state(&active, *effect_id)
                    })
                    .collect::<Vec<_>>();
                let reported_changed_effects = changed_effects
                    .iter()
                    .filter(|effect_id| reported_effects.contains(effect_id))
                    .copied()
                    .collect::<Vec<_>>();
                if reported_changed_effects.is_empty() {
                    index = end;
                    continue;
                }
                if changed_effects.len() != 1 || reported_changed_effects.len() != 1 {
                    report.excluded_ambiguous_effect_or_instance_transition = report
                        .excluded_ambiguous_effect_or_instance_transition
                        .saturating_add(1);
                    index = end;
                    continue;
                }
                let effect_id = reported_changed_effects[0];
                let before_state = active_effect_state(&before_active, effect_id);
                let after_state = active_effect_state(&active, effect_id);
                let transition = match exact_single_instance_stack_transition(
                    effect_id,
                    &before_state,
                    &after_state,
                ) {
                    Ok(Some(transition)) => transition,
                    Ok(None) => {
                        index = end;
                        continue;
                    }
                    Err(StackTransitionExclusion::MissingStackCount) => {
                        report.excluded_missing_stack_count =
                            report.excluded_missing_stack_count.saturating_add(1);
                        index = end;
                        continue;
                    }
                    Err(StackTransitionExclusion::AmbiguousEffectOrInstance) => {
                        report.excluded_ambiguous_effect_or_instance_transition = report
                            .excluded_ambiguous_effect_or_instance_transition
                            .saturating_add(1);
                        index = end;
                        continue;
                    }
                };
                let (fingerprint, before_stacks, after_stacks, raw_source_entity_uuid) = transition;
                let signed_stack_delta = i64::from(after_stacks) - i64::from(before_stacks);
                let raw_attribute_delta =
                    after_attribute.value.saturating_sub(before_attribute.value);
                let representative = statuses[index..end]
                    .iter()
                    .rev()
                    .find(|status| status.effect_id == effect_id)
                    .expect("changed effect has a representative status");
                let source_entity_uuid = session
                    .provider_at(representative)
                    .resolved_entity_uuid
                    .or(raw_source_entity_uuid);
                let equation = equations
                    .entry(WireStackDeltaEquationKey {
                        fingerprint,
                        signed_stack_delta,
                        raw_attribute_delta,
                    })
                    .or_default();
                equation.count = equation.count.saturating_add(1);
                equation
                    .independent_run_contexts
                    .insert((session_id.clone(), *run_ordinal));
                equation.target_entity_uuids.insert(*target_entity_uuid);
                if let Some(source) = source_entity_uuid {
                    equation.source_entity_uuids.insert(source);
                    if source == *target_entity_uuid {
                        equation.self_source_occurrences =
                            equation.self_source_occurrences.saturating_add(1);
                    } else {
                        equation.cross_actor_occurrences =
                            equation.cross_actor_occurrences.saturating_add(1);
                    }
                } else {
                    equation.missing_source_occurrences =
                        equation.missing_source_occurrences.saturating_add(1);
                }
                if equation.examples.len() < example_limit {
                    equation.examples.push(WireStackDeltaEquationExample {
                        rlog: session.rlog.clone(),
                        session_id: session_id.clone(),
                        run_ordinal: *run_ordinal,
                        target_entity_uuid: *target_entity_uuid,
                        source_entity_uuid,
                        wire_capture_sequence: message.capture_sequence,
                        before_stacks,
                        after_stacks,
                        before_value: before_attribute.value,
                        after_value: after_attribute.value,
                    });
                }
                report.exact_single_effect_stack_equations =
                    report.exact_single_effect_stack_equations.saturating_add(1);
                index = end;
            }
        }
    }

    report.unique_equations = equations.len();
    report.equations = equations
        .into_iter()
        .map(|(key, accumulator)| WireStackDeltaEquation {
            fingerprint: key.fingerprint,
            signed_stack_delta: key.signed_stack_delta,
            raw_attribute_delta: key.raw_attribute_delta,
            exact_coefficient_units_per_stack: exact_coefficient_per_stack(
                key.raw_attribute_delta,
                key.signed_stack_delta,
            ),
            count: accumulator.count,
            independent_run_contexts: accumulator.independent_run_contexts.len(),
            target_entity_count: accumulator.target_entity_uuids.len(),
            source_entity_count: accumulator.source_entity_uuids.len(),
            cross_actor_occurrences: accumulator.cross_actor_occurrences,
            self_source_occurrences: accumulator.self_source_occurrences,
            missing_source_occurrences: accumulator.missing_source_occurrences,
            examples: accumulator.examples,
            independent_run_context_keys: accumulator.independent_run_contexts,
            target_entity_uuid_keys: accumulator.target_entity_uuids,
            source_entity_uuid_keys: accumulator.source_entity_uuids,
        })
        .collect();
    report
}

fn reversible_per_stack_coefficient_proofs(
    reports: &[WireStackDeltaAttributeReport],
    stateful_attributes: &BTreeSet<i32>,
) -> Vec<ReversiblePerStackCoefficientProof> {
    let mut accumulators =
        BTreeMap::<(i32, StackCoefficientFingerprint), PerStackCoefficientAccumulator>::new();
    for report in reports {
        for equation in &report.equations {
            let accumulator = accumulators
                .entry((report.attribute_id, equation.fingerprint.clone()))
                .or_default();
            if let Some(coefficient) = equation.exact_coefficient_units_per_stack {
                *accumulator
                    .exact_coefficient_counts
                    .entry(coefficient)
                    .or_default() += equation.count;
            } else {
                accumulator.non_integral_equations = accumulator
                    .non_integral_equations
                    .saturating_add(equation.count);
            }
            if equation.signed_stack_delta > 0 {
                accumulator.positive_stack_delta_occurrences = accumulator
                    .positive_stack_delta_occurrences
                    .saturating_add(equation.count);
            } else {
                accumulator.negative_stack_delta_occurrences = accumulator
                    .negative_stack_delta_occurrences
                    .saturating_add(equation.count);
            }
            accumulator.source_equations = accumulator.source_equations.saturating_add(1);
            accumulator.cross_actor_occurrences = accumulator
                .cross_actor_occurrences
                .saturating_add(equation.cross_actor_occurrences);
            accumulator.self_source_occurrences = accumulator
                .self_source_occurrences
                .saturating_add(equation.self_source_occurrences);
            accumulator.missing_source_occurrences = accumulator
                .missing_source_occurrences
                .saturating_add(equation.missing_source_occurrences);
            accumulator
                .independent_run_contexts
                .extend(equation.independent_run_context_keys.iter().cloned());
            accumulator
                .target_entity_uuids
                .extend(equation.target_entity_uuid_keys.iter().copied());
            accumulator
                .source_entity_uuids
                .extend(equation.source_entity_uuid_keys.iter().copied());
        }
    }

    accumulators
        .into_iter()
        .map(|((attribute_id, fingerprint), accumulator)| {
            let excluded_stateful = stateful_attributes.contains(&attribute_id);
            let coefficient_is_constant = accumulator.exact_coefficient_counts.len() == 1;
            let reversible = accumulator.positive_stack_delta_occurrences > 0
                && accumulator.negative_stack_delta_occurrences > 0;
            let independently_repeated = accumulator.independent_run_contexts.len() >= 2;
            let status = if excluded_stateful {
                "excluded_stateful_attribute"
            } else if accumulator.non_integral_equations > 0 {
                "contradicted_non_integral_per_stack_coefficient"
            } else if !coefficient_is_constant {
                "contradicted_nonconstant_per_stack_coefficient"
            } else if !reversible {
                "insufficient_missing_positive_negative_stack_steps"
            } else if !independently_repeated {
                "insufficient_independent_run_contexts"
            } else {
                "proven_reversible_per_stack_coefficient"
            };
            let proven_coefficient_units_per_stack =
                (status == "proven_reversible_per_stack_coefficient")
                    .then(|| accumulator.exact_coefficient_counts.keys().next().copied())
                    .flatten();
            let blocker = match status {
                "excluded_stateful_attribute" => Some(
                    "stateful pools require a dedicated state ledger instead of reversible stack math",
                ),
                "contradicted_non_integral_per_stack_coefficient" => Some(
                    "at least one exact stack step did not divide its same-wire attribute delta evenly",
                ),
                "contradicted_nonconstant_per_stack_coefficient" => Some(
                    "the same effect fingerprint produced conflicting exact per-stack coefficients",
                ),
                "insufficient_missing_positive_negative_stack_steps" => Some(
                    "both increasing and decreasing exact stack steps are required",
                ),
                "insufficient_independent_run_contexts" => Some(
                    "the reversible per-stack coefficient must repeat in at least two independent session-run contexts",
                ),
                _ if accumulator.cross_actor_occurrences == 0 => Some(
                    "the per-stack coefficient is exact but no external provider-to-recipient occurrence was observed",
                ),
                _ => Some(
                    "the external per-stack coefficient is exact; downstream damage attribution remains blocked until its formula stage is separately proven",
                ),
            };
            ReversiblePerStackCoefficientProof {
                attribute_id,
                fingerprint,
                status,
                proven_coefficient_units_per_stack,
                exact_coefficient_counts: accumulator.exact_coefficient_counts,
                non_integral_equations: accumulator.non_integral_equations,
                positive_stack_delta_occurrences: accumulator.positive_stack_delta_occurrences,
                negative_stack_delta_occurrences: accumulator.negative_stack_delta_occurrences,
                independent_run_contexts: accumulator.independent_run_contexts.len(),
                target_entity_count: accumulator.target_entity_uuids.len(),
                source_entity_count: accumulator.source_entity_uuids.len(),
                cross_actor_occurrences: accumulator.cross_actor_occurrences,
                self_source_occurrences: accumulator.self_source_occurrences,
                missing_source_occurrences: accumulator.missing_source_occurrences,
                source_equations: accumulator.source_equations,
                runtime_eligible_for_rdps: false,
                blocker,
            }
        })
        .collect()
}

fn candidate_magnitude_proof_reports(
    candidates: &[ProofWatchlistCandidate],
    reversible_static: &[ReversibleStaticCoefficientProof],
    reversible_stack: &[ReversiblePerStackCoefficientProof],
    matched_lifecycle: &[MatchedLifecycleCoefficientProof],
    non_attributable_context_attributes: &BTreeSet<i32>,
) -> Vec<CandidateMagnitudeProofReport> {
    candidates
        .iter()
        .map(|candidate| {
            let mut surfaces = Vec::new();
            for lifecycle in &candidate.lifecycle_effects {
                for attribute_id in &candidate.selected_attribute_ids {
                    let mut statuses = BTreeSet::new();
                    let mut coefficients = BTreeSet::new();
                    let mut evidence_records = 0_u64;
                    match lifecycle.proof_model.as_str() {
                        "exact-stack-delta" => {
                            for proof in reversible_stack.iter().filter(|proof| {
                                proof.attribute_id == *attribute_id
                                    && proof.fingerprint.effect_id == lifecycle.effect_id
                            }) {
                                statuses.insert(proof.status.to_owned());
                                coefficients.extend(proof.proven_coefficient_units_per_stack);
                                evidence_records =
                                    evidence_records.saturating_add(proof.source_equations);
                            }
                        }
                        "exact-binary-presence" => {
                            for proof in reversible_static.iter().filter(|proof| {
                                proof.attribute_id == *attribute_id
                                    && proof.fingerprint.effect_id == lifecycle.effect_id
                            }) {
                                statuses.insert(proof.status.to_owned());
                                coefficients.extend(proof.proven_coefficient_units);
                                evidence_records =
                                    evidence_records.saturating_add(proof.source_equations);
                            }
                            for proof in matched_lifecycle.iter().filter(|proof| {
                                proof.attribute_id == *attribute_id
                                    && proof.fingerprint.effect_id == lifecycle.effect_id
                            }) {
                                statuses.insert(proof.status.to_owned());
                                coefficients.extend(proof.proven_coefficient_units);
                                evidence_records =
                                    evidence_records.saturating_add(proof.exact_pair_count);
                            }
                        }
                        _ => {
                            statuses.insert("unsupported-watchlist-proof-model".to_owned());
                        }
                    }
                    if statuses.is_empty() {
                        statuses.insert("no-current-build-packet-equation".to_owned());
                    }
                    let proven_coefficient_units = coefficients.into_iter().collect::<Vec<_>>();
                    surfaces.push(CandidateMagnitudeProofSurface {
                        effect_id: lifecycle.effect_id,
                        attribute_id: *attribute_id,
                        proof_model: lifecycle.proof_model.clone(),
                        observed_proof_statuses: statuses.into_iter().collect(),
                        has_exact_reversible_coefficient: !proven_coefficient_units.is_empty(),
                        proven_coefficient_units,
                        evidence_records,
                        attribution_role: if non_attributable_context_attributes
                            .contains(attribute_id)
                        {
                            "self_only_formula_context_never_external_credit"
                        } else {
                            "formula_context_requires_packet_provider_recipient_and_counterfactual_proof"
                        },
                        runtime_eligible_for_rdps: false,
                    });
                }
            }
            CandidateMagnitudeProofReport {
                source_rule_id: candidate.source_rule_id.clone(),
                source_id: candidate.source_id.clone(),
                formula_terms: candidate.formula_terms.clone(),
                required_runtime_evidence: candidate.required_runtime_evidence.clone(),
                static_value_state: candidate.static_value_state.clone(),
                static_value_proofs: candidate.static_value_proofs.clone(),
                static_blockers: candidate.static_blockers.clone(),
                evidence_state: "effect_attribute_matrix_only_damage_counterfactual_still_required",
                runtime_eligible_for_rdps: false,
                surfaces,
            }
        })
        .collect()
}

fn reversible_static_coefficient_proofs(
    reports: &[WireAdditiveAttributeReport],
    reported_effects: &BTreeSet<i64>,
    stateful_attributes: &BTreeSet<i32>,
) -> Vec<ReversibleStaticCoefficientProof> {
    let mut accumulators =
        BTreeMap::<(i32, StaticCoefficientFingerprint), StaticCoefficientAccumulator>::new();
    for report in reports {
        for equation in &report.equations {
            let [term] = equation.terms.as_slice() else {
                continue;
            };
            if !reported_effects.contains(&term.effect_id) {
                continue;
            }
            let fingerprint = StaticCoefficientFingerprint {
                effect_id: term.effect_id,
                origin: term.origin,
                level: term.level,
                part_id: term.part_id,
                stacks: term.stacks,
                count: term.count,
            };
            let accumulator = accumulators
                .entry((report.attribute_id, fingerprint))
                .or_default();
            let normalized = equation
                .raw_attribute_delta
                .saturating_mul(i64::from(term.signed_presence_delta));
            *accumulator
                .normalized_coefficients
                .entry(normalized)
                .or_default() += equation.count;
            if term.signed_presence_delta > 0 {
                accumulator.apply_occurrences =
                    accumulator.apply_occurrences.saturating_add(equation.count);
            } else {
                accumulator.remove_occurrences = accumulator
                    .remove_occurrences
                    .saturating_add(equation.count);
            }
            accumulator.source_equations = accumulator.source_equations.saturating_add(1);
            accumulator.cross_actor_occurrences = accumulator
                .cross_actor_occurrences
                .saturating_add(equation.cross_actor_occurrences);
            accumulator.self_source_occurrences = accumulator
                .self_source_occurrences
                .saturating_add(equation.self_source_occurrences);
            accumulator.missing_source_occurrences = accumulator
                .missing_source_occurrences
                .saturating_add(equation.missing_source_occurrences);
            for example in &equation.examples {
                accumulator
                    .independent_run_contexts
                    .insert((example.session_id.clone(), example.run_ordinal));
                accumulator
                    .target_entity_uuids
                    .insert(example.target_entity_uuid);
                accumulator
                    .source_entity_uuids
                    .extend(example.source_entity_uuids.iter().copied());
            }
        }
    }

    accumulators
        .into_iter()
        .map(|((attribute_id, fingerprint), accumulator)| {
            let excluded_stateful = stateful_attributes.contains(&attribute_id);
            let coefficient_is_constant = accumulator.normalized_coefficients.len() == 1;
            let mirrored = accumulator.apply_occurrences > 0 && accumulator.remove_occurrences > 0;
            let independently_repeated = accumulator.independent_run_contexts.len() >= 2;
            let status = if excluded_stateful {
                "excluded_stateful_attribute"
            } else if !coefficient_is_constant {
                "contradicted_nonconstant_coefficient"
            } else if !mirrored {
                "insufficient_missing_apply_remove_mirror"
            } else if !independently_repeated {
                "insufficient_independent_run_contexts"
            } else {
                "proven_reversible_static_coefficient"
            };
            let proven_coefficient_units = (status == "proven_reversible_static_coefficient")
                .then(|| accumulator.normalized_coefficients.keys().next().copied())
                .flatten();
            let blocker = match status {
                "excluded_stateful_attribute" => Some(
                    "stateful pools can change for reasons unrelated to status presence and require a dedicated state ledger",
                ),
                "contradicted_nonconstant_coefficient" => Some(
                    "the same effect fingerprint produced conflicting normalized coefficients",
                ),
                "insufficient_missing_apply_remove_mirror" => Some(
                    "both application and removal are required before a static coefficient is proven",
                ),
                "insufficient_independent_run_contexts" => Some(
                    "the reversible coefficient must repeat in at least two independent session-run contexts",
                ),
                _ if accumulator.cross_actor_occurrences == 0 => Some(
                    "the coefficient is exact but no external provider-to-recipient occurrence was observed",
                ),
                _ => Some(
                    "the external stat coefficient is exact; its downstream damage counterfactual still requires a separately proven formula stage",
                ),
            };
            ReversibleStaticCoefficientProof {
                attribute_id,
                fingerprint,
                status,
                proven_coefficient_units,
                normalized_coefficient_counts: accumulator.normalized_coefficients,
                apply_occurrences: accumulator.apply_occurrences,
                remove_occurrences: accumulator.remove_occurrences,
                independent_run_contexts: accumulator.independent_run_contexts.len(),
                target_entity_count: accumulator.target_entity_uuids.len(),
                source_entity_count: accumulator.source_entity_uuids.len(),
                cross_actor_occurrences: accumulator.cross_actor_occurrences,
                self_source_occurrences: accumulator.self_source_occurrences,
                missing_source_occurrences: accumulator.missing_source_occurrences,
                source_equations: accumulator.source_equations,
                runtime_eligible_for_rdps: false,
                blocker,
            }
        })
        .collect()
}

fn matched_lifecycle_coefficient_proofs(
    sessions: &[SessionData],
    reported_effects: &BTreeSet<i64>,
    attributes: &BTreeSet<i32>,
    stateful_attributes: &BTreeSet<i32>,
    example_limit: usize,
) -> Vec<MatchedLifecycleCoefficientProof> {
    let mut instances =
        BTreeMap::<(String, u32, i64, i64, i64, i32), LifecycleInstanceAccumulator>::new();
    for session in sessions {
        let session_id = session.session_id.clone().unwrap_or_default();
        for status in &session.selected_statuses {
            if !reported_effects.contains(&status.effect_id)
                || !session.is_effective_status_transition(status)
            {
                continue;
            }
            let Some(instance_id) = status.instance_id else {
                continue;
            };
            let Some(wire_message) = status.wire_message else {
                continue;
            };
            let changed_effects = session
                .statuses_by_target
                .get(&(status.run_ordinal, status.target.entity_uuid.0))
                .into_iter()
                .flatten()
                .filter(|other| {
                    other.wire_message == Some(wire_message)
                        && session.is_effective_status_transition(other)
                })
                .map(|other| other.effect_id)
                .collect::<BTreeSet<_>>();
            if changed_effects.len() != 1 || !changed_effects.contains(&status.effect_id) {
                continue;
            }
            let is_application = status.state == StatusState::Applied;
            let is_removal = status.state == StatusState::Removed;
            if !is_application && !is_removal {
                continue;
            }
            let provider = session.provider_at(status);
            let source_entity_uuid = provider.resolved_entity_uuid;
            let cross_actor =
                source_entity_uuid.is_some_and(|source| source != status.target.entity_uuid.0);
            let fingerprint = StaticCoefficientFingerprint {
                effect_id: status.effect_id,
                origin: status.origin,
                level: status.level,
                part_id: status.part_id,
                stacks: status.stacks,
                count: status.count,
            };
            for attribute_id in attributes {
                let Some(transition) =
                    session.attribute_transition_in_status_wire(status, *attribute_id)
                else {
                    continue;
                };
                if !transition.updated_in_status_wire {
                    continue;
                }
                let evidence = LifecycleTransitionEvidence {
                    fingerprint: fingerprint.clone(),
                    raw_attribute_delta: transition
                        .after
                        .value
                        .saturating_sub(transition.before.value),
                    session_id: session_id.clone(),
                    run_ordinal: status.run_ordinal,
                    target_entity_uuid: status.target.entity_uuid.0,
                    source_entity_uuid,
                    cross_actor,
                    wire_capture_sequence: wire_message.capture_sequence,
                    before_value: transition.before.value,
                    after_value: transition.after.value,
                };
                let instance = instances
                    .entry((
                        session_id.clone(),
                        status.run_ordinal,
                        status.target.entity_uuid.0,
                        status.effect_id,
                        instance_id,
                        *attribute_id,
                    ))
                    .or_default();
                if is_application {
                    instance.applications.push(evidence);
                } else {
                    instance.removals.push(evidence);
                }
            }
        }
    }

    let mut proofs =
        BTreeMap::<(i32, StaticCoefficientFingerprint), LifecycleProofAccumulator>::new();
    for (
        (session_id, run_ordinal, target_entity_uuid, effect_id, instance_id, attribute_id),
        instance,
    ) in instances
    {
        let fingerprints = instance
            .applications
            .iter()
            .chain(&instance.removals)
            .map(|transition| transition.fingerprint.clone())
            .collect::<BTreeSet<_>>();
        if fingerprints.len() != 1 {
            for fingerprint in fingerprints {
                proofs
                    .entry((attribute_id, fingerprint))
                    .or_default()
                    .ambiguous_instance_count += 1;
            }
            continue;
        }
        let fingerprint = fingerprints
            .into_iter()
            .next()
            .expect("non-empty lifecycle fingerprint");
        let proof = proofs
            .entry((attribute_id, fingerprint.clone()))
            .or_default();
        let classification = match (
            instance.applications.as_slice(),
            instance.removals.as_slice(),
        ) {
            ([application], [removal]) => {
                proof
                    .independent_run_contexts
                    .insert((application.session_id.clone(), application.run_ordinal));
                proof
                    .target_entity_uuids
                    .insert(application.target_entity_uuid);
                if let Some(source) = application.source_entity_uuid {
                    proof.source_entity_uuids.insert(source);
                }
                if removal
                    .raw_attribute_delta
                    .checked_neg()
                    .is_some_and(|delta| application.raw_attribute_delta == delta)
                {
                    proof.exact_pair_count = proof.exact_pair_count.saturating_add(1);
                    *proof
                        .exact_coefficient_counts
                        .entry(application.raw_attribute_delta)
                        .or_default() += 1;
                    if application.cross_actor && removal.cross_actor {
                        proof.cross_actor_exact_pairs =
                            proof.cross_actor_exact_pairs.saturating_add(1);
                    }
                    "exact-opposite-pair"
                } else {
                    proof.contradictory_pair_count =
                        proof.contradictory_pair_count.saturating_add(1);
                    "contradictory-pair"
                }
            }
            ([], [_]) => {
                proof.removal_only_instance_count =
                    proof.removal_only_instance_count.saturating_add(1);
                "removal-only"
            }
            ([_], []) => {
                proof.application_only_instance_count =
                    proof.application_only_instance_count.saturating_add(1);
                "application-only"
            }
            _ => {
                proof.ambiguous_instance_count = proof.ambiguous_instance_count.saturating_add(1);
                "ambiguous-multiplicity"
            }
        };
        retain_lifecycle_example(
            &mut proof.examples,
            MatchedLifecycleExample {
                classification,
                session_id,
                run_ordinal,
                target_entity_uuid,
                effect_id,
                instance_id,
                fingerprint,
                applications: instance
                    .applications
                    .iter()
                    .map(lifecycle_transition_example)
                    .collect(),
                removals: instance
                    .removals
                    .iter()
                    .map(lifecycle_transition_example)
                    .collect(),
            },
            example_limit,
        );
    }

    proofs
        .into_iter()
        .map(|((attribute_id, fingerprint), proof)| {
            let excluded_stateful = stateful_attributes.contains(&attribute_id);
            let coefficient_is_constant = proof.exact_coefficient_counts.len() == 1;
            let status = if excluded_stateful {
                "excluded_stateful_attribute"
            } else if proof.contradictory_pair_count > 0 {
                "contradicted_matched_lifecycle"
            } else if proof.exact_pair_count == 0 {
                "insufficient_matched_lifecycle_pairs"
            } else if !coefficient_is_constant {
                "contradicted_nonconstant_matched_coefficient"
            } else if proof.exact_pair_count < 2 {
                "insufficient_matched_lifecycle_pairs"
            } else {
                "proven_matched_lifecycle_coefficient"
            };
            let proven_coefficient_units = (status == "proven_matched_lifecycle_coefficient")
                .then(|| proof.exact_coefficient_counts.keys().next().copied())
                .flatten();
            let blocker = match status {
                "excluded_stateful_attribute" => Some(
                    "stateful pools require a dedicated state ledger instead of reversible lifecycle math",
                ),
                "contradicted_matched_lifecycle" => Some(
                    "at least one exact status instance had application and removal deltas that were not opposites",
                ),
                "contradicted_nonconstant_matched_coefficient" => Some(
                    "matched status instances produced more than one exact reversible coefficient",
                ),
                "insufficient_matched_lifecycle_pairs" => Some(
                    "at least two exact application-removal instance pairs are required",
                ),
                _ if proof.cross_actor_exact_pairs == 0 => Some(
                    "the lifecycle coefficient is exact but has no external provider-to-recipient pair",
                ),
                _ => Some(
                    "the external stat coefficient is exact; downstream damage attribution remains blocked until its formula stage is separately proven",
                ),
            };
            MatchedLifecycleCoefficientProof {
                attribute_id,
                fingerprint,
                status,
                proven_coefficient_units,
                exact_coefficient_counts: proof.exact_coefficient_counts,
                exact_pair_count: proof.exact_pair_count,
                contradictory_pair_count: proof.contradictory_pair_count,
                ambiguous_instance_count: proof.ambiguous_instance_count,
                application_only_instance_count: proof.application_only_instance_count,
                removal_only_instance_count: proof.removal_only_instance_count,
                independent_run_contexts: proof.independent_run_contexts.len(),
                target_entity_count: proof.target_entity_uuids.len(),
                source_entity_count: proof.source_entity_uuids.len(),
                cross_actor_exact_pairs: proof.cross_actor_exact_pairs,
                runtime_eligible_for_rdps: false,
                blocker,
                examples: proof.examples,
            }
        })
        .collect()
}

fn lifecycle_transition_example(
    transition: &LifecycleTransitionEvidence,
) -> LifecycleTransitionExample {
    LifecycleTransitionExample {
        raw_attribute_delta: transition.raw_attribute_delta,
        source_entity_uuid: transition.source_entity_uuid,
        cross_actor: transition.cross_actor,
        wire_capture_sequence: transition.wire_capture_sequence,
        before_value: transition.before_value,
        after_value: transition.after_value,
    }
}

fn retain_lifecycle_example(
    examples: &mut Vec<MatchedLifecycleExample>,
    example: MatchedLifecycleExample,
    example_limit: usize,
) {
    if example_limit == 0 {
        return;
    }
    if examples.len() < example_limit {
        examples.push(example);
        return;
    }
    let incoming_priority = lifecycle_example_priority(example.classification);
    let replace = examples
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            examples
                .iter()
                .filter(|other| other.classification == candidate.classification)
                .count()
                > 1
                && lifecycle_example_priority(candidate.classification) < incoming_priority
        })
        .min_by_key(|(_, candidate)| lifecycle_example_priority(candidate.classification))
        .map(|(index, _)| index);
    if let Some(index) = replace {
        examples[index] = example;
    }
}

fn lifecycle_example_priority(classification: &str) -> u8 {
    match classification {
        "contradictory-pair" => 4,
        "exact-opposite-pair" => 3,
        "ambiguous-multiplicity" => 2,
        "application-only" | "removal-only" => 1,
        _ => 0,
    }
}

fn active_stack_attribute_surface(
    sessions: &[SessionData],
    effect_id: i64,
    attribute_id: i32,
    example_limit: usize,
) -> ActiveStackAttributeSurfaceReport {
    let mut accumulator = ActiveStackAttributeSurfaceAccumulator::default();
    for session in sessions {
        let session_id = session.session_id.clone().unwrap_or_default();
        for ((run_ordinal, target_entity_uuid, observed_attribute_id), points) in
            &session.attributes
        {
            if *observed_attribute_id != attribute_id {
                continue;
            }
            let statuses = session
                .statuses_by_target
                .get(&(*run_ordinal, *target_entity_uuid))
                .into_iter()
                .flatten()
                .filter(|status| status.effect_id == effect_id)
                .collect::<Vec<_>>();
            for point in points {
                accumulator.attribute_samples = accumulator.attribute_samples.saturating_add(1);
                let active = active_effect_instances_at(&statuses, point);
                if active.is_empty() {
                    accumulator.samples_with_effect_inactive =
                        accumulator.samples_with_effect_inactive.saturating_add(1);
                    continue;
                }
                if active.len() != 1 {
                    accumulator.samples_with_multiple_active_instances = accumulator
                        .samples_with_multiple_active_instances
                        .saturating_add(1);
                    continue;
                }
                let Some(stacks) = active.values().next().copied().flatten() else {
                    accumulator.samples_with_missing_stack_count = accumulator
                        .samples_with_missing_stack_count
                        .saturating_add(1);
                    continue;
                };
                accumulator.exact_single_instance_stack_samples = accumulator
                    .exact_single_instance_stack_samples
                    .saturating_add(1);
                let pair = accumulator.pairs.entry((stacks, point.value)).or_default();
                pair.count = pair.count.saturating_add(1);
                if pair.examples.len() < example_limit {
                    pair.examples.push(StackAttributeValueExample {
                        rlog: session.rlog.clone(),
                        session_id: session_id.clone(),
                        run_ordinal: *run_ordinal,
                        target_entity_uuid: *target_entity_uuid,
                        attribute_sequence: point.sequence,
                        observed_micros: point.observed_micros,
                        wire_capture_sequence: point
                            .wire_message
                            .map(|message| message.capture_sequence),
                        stacks,
                        attribute_value: point.value,
                    });
                }
            }
        }
    }
    let mut values_by_stack = BTreeMap::<u32, BTreeSet<i64>>::new();
    for &(stacks, value) in accumulator.pairs.keys() {
        values_by_stack.entry(stacks).or_default().insert(value);
    }
    ActiveStackAttributeSurfaceReport {
        attribute_id,
        attribute_samples: accumulator.attribute_samples,
        samples_with_effect_inactive: accumulator.samples_with_effect_inactive,
        samples_with_multiple_active_instances: accumulator.samples_with_multiple_active_instances,
        samples_with_missing_stack_count: accumulator.samples_with_missing_stack_count,
        exact_single_instance_stack_samples: accumulator.exact_single_instance_stack_samples,
        every_observed_stack_has_one_attribute_value: !values_by_stack.is_empty()
            && values_by_stack.values().all(|values| values.len() == 1),
        stack_value_pairs: accumulator
            .pairs
            .into_iter()
            .map(
                |((stacks, attribute_value), value)| StackAttributeValueAggregate {
                    stacks,
                    attribute_value,
                    count: value.count,
                    examples: value.examples,
                },
            )
            .collect(),
    }
}

struct ProviderEvidence<'a> {
    resolved_entity_uuid: Option<i64>,
    resolution: &'static str,
    snapshot: Option<&'a ActorSnapshot>,
}

impl ProviderEvidence<'_> {
    fn unresolved() -> Self {
        Self {
            resolved_entity_uuid: None,
            resolution: "missing_source",
            snapshot: None,
        }
    }
}

#[derive(Debug, Default)]
struct AttributeAccumulator {
    report: AttributeReport,
    aggregates: BTreeMap<AggregateKey, AggregateAccumulator>,
}

#[derive(Debug, Default)]
struct PercentFamilyFormulaAccumulator {
    spec: PercentFamilySpec,
    report: PercentFamilyFormulaReport,
    aggregates: BTreeMap<PercentFamilyFormulaKey, PercentFamilyFormulaAggregateAccumulator>,
}

#[derive(Debug, Clone, Copy, Default)]
struct PercentFamilySpec {
    family: &'static str,
    final_attribute_id: i32,
    intermediate_attribute_id: i32,
    base_attribute_id: i32,
    raw_extra_add_attribute_id: i32,
    raw_percent_attribute_id: i32,
    raw_extra_percent_attribute_id: i32,
    intermediate_expression: &'static str,
    final_expression: &'static str,
}

fn percent_family_specs() -> Vec<PercentFamilySpec> {
    [
        ("strength", 11010),
        ("intelligence", 11020),
        ("dexterity", 11030),
        ("vitality", 11040),
        ("haste_rating", 11120),
        ("luck_rating", 11130),
        ("mastery_rating", 11140),
        ("versatility_rating", 11150),
        ("max_hp", 11320),
        ("attack", 11330),
        ("magic_attack", 11340),
        ("physical_defense", 11350),
        ("magic_defense", 11360),
        ("critical_rate", 11710),
        ("attack_speed", 11720),
        ("cast_speed", 11730),
        ("lucky_strike_probability", 11780),
        ("haste_percent", 11930),
        ("mastery_percent", 11940),
        ("versatility_percent", 11950),
    ]
    .into_iter()
    .map(|(family, final_attribute_id)| PercentFamilySpec {
        family,
        final_attribute_id,
        intermediate_attribute_id: final_attribute_id + 1,
        base_attribute_id: final_attribute_id + 2,
        raw_extra_add_attribute_id: final_attribute_id + 3,
        raw_percent_attribute_id: final_attribute_id + 4,
        raw_extra_percent_attribute_id: final_attribute_id + 5,
        intermediate_expression: "delta(intermediate) = trunc(after base_add * (10000 + after raw_percent) / 10000) - trunc(before base_add * (10000 + before raw_percent) / 10000)",
        final_expression: "when raw_extra_percent is packet-known, delta(final) = trunc(after intermediate * (10000 + after raw_extra_percent) / 10000) - trunc(before intermediate * (10000 + before raw_extra_percent) / 10000)",
    })
    .collect()
}

impl PercentFamilyFormulaAccumulator {
    fn new(spec: PercentFamilySpec) -> Self {
        Self {
            spec,
            report: PercentFamilyFormulaReport {
                family: spec.family,
                final_attribute_id: spec.final_attribute_id,
                intermediate_attribute_id: spec.intermediate_attribute_id,
                base_attribute_id: spec.base_attribute_id,
                raw_extra_add_attribute_id: spec.raw_extra_add_attribute_id,
                raw_percent_attribute_id: spec.raw_percent_attribute_id,
                raw_extra_percent_attribute_id: spec.raw_extra_percent_attribute_id,
                intermediate_expression: spec.intermediate_expression,
                final_expression: spec.final_expression,
                scale: 10_000,
                ..PercentFamilyFormulaReport::default()
            },
            aggregates: BTreeMap::new(),
        }
    }

    fn observe(&mut self, session: &SessionData, status: &StatusPoint, example_limit: usize) {
        self.report.transitions_examined = self.report.transitions_examined.saturating_add(1);
        let Some(message) = status.wire_message else {
            return;
        };
        let Some(final_value) =
            session.attribute_transition_in_status_wire(status, self.spec.final_attribute_id)
        else {
            return;
        };
        let Some(intermediate_value) = session
            .attribute_transition_in_status_wire(status, self.spec.intermediate_attribute_id)
        else {
            return;
        };
        let Some(base_add) =
            session.attribute_transition_in_status_wire(status, self.spec.base_attribute_id)
        else {
            return;
        };
        let Some(raw_percent) =
            session.attribute_transition_in_status_wire(status, self.spec.raw_percent_attribute_id)
        else {
            return;
        };
        let extra_add = session
            .attribute_transition_in_status_wire(status, self.spec.raw_extra_add_attribute_id);
        if !final_value.updated_in_status_wire
            || !intermediate_value.updated_in_status_wire
            || !(base_add.updated_in_status_wire || raw_percent.updated_in_status_wire)
        {
            return;
        }
        let Some(before_scaled) =
            scaled_percent_family(base_add.before.value, raw_percent.before.value)
        else {
            return;
        };
        let Some(after_scaled) =
            scaled_percent_family(base_add.after.value, raw_percent.after.value)
        else {
            return;
        };
        let predicted_intermediate_delta = after_scaled.saturating_sub(before_scaled);
        let Some(before_nearest_scaled) =
            rounded_percent_family(base_add.before.value, raw_percent.before.value)
        else {
            return;
        };
        let Some(after_nearest_scaled) =
            rounded_percent_family(base_add.after.value, raw_percent.after.value)
        else {
            return;
        };
        let predicted_nearest_intermediate_delta =
            after_nearest_scaled.saturating_sub(before_nearest_scaled);
        let observed_intermediate_delta = intermediate_value
            .after
            .value
            .saturating_sub(intermediate_value.before.value);
        let intermediate_residual =
            observed_intermediate_delta.saturating_sub(predicted_intermediate_delta);
        let nearest_intermediate_residual =
            observed_intermediate_delta.saturating_sub(predicted_nearest_intermediate_delta);
        let observed_final_delta = final_value
            .after
            .value
            .saturating_sub(final_value.before.value);
        let extra_percent = session
            .attribute_transition_in_status_wire(status, self.spec.raw_extra_percent_attribute_id);
        let predicted_final_delta = extra_percent.and_then(|extra_percent| {
            let before =
                scaled_percent_family(intermediate_value.before.value, extra_percent.before.value)?;
            let after =
                scaled_percent_family(intermediate_value.after.value, extra_percent.after.value)?;
            Some(after.saturating_sub(before))
        });
        let final_residual =
            predicted_final_delta.map(|predicted| observed_final_delta.saturating_sub(predicted));
        self.report.transitions_with_exact_wire_inputs = self
            .report
            .transitions_with_exact_wire_inputs
            .saturating_add(1);
        if intermediate_residual == 0 {
            self.report.intermediate_exact_delta_matches = self
                .report
                .intermediate_exact_delta_matches
                .saturating_add(1);
        } else {
            self.report.intermediate_residual_mismatches = self
                .report
                .intermediate_residual_mismatches
                .saturating_add(1);
        }
        if nearest_intermediate_residual == 0 {
            self.report.nearest_intermediate_exact_delta_matches = self
                .report
                .nearest_intermediate_exact_delta_matches
                .saturating_add(1);
        } else {
            self.report.nearest_intermediate_residual_mismatches = self
                .report
                .nearest_intermediate_residual_mismatches
                .saturating_add(1);
        }
        match final_residual {
            Some(0) => {
                self.report.final_transitions_with_known_extra_percent = self
                    .report
                    .final_transitions_with_known_extra_percent
                    .saturating_add(1);
                self.report.final_exact_delta_matches =
                    self.report.final_exact_delta_matches.saturating_add(1);
            }
            Some(_) => {
                self.report.final_transitions_with_known_extra_percent = self
                    .report
                    .final_transitions_with_known_extra_percent
                    .saturating_add(1);
                self.report.final_residual_mismatches =
                    self.report.final_residual_mismatches.saturating_add(1);
            }
            None => {
                self.report.final_transitions_with_unknown_extra_percent = self
                    .report
                    .final_transitions_with_unknown_extra_percent
                    .saturating_add(1);
            }
        }
        let base_delta = base_add.after.value.saturating_sub(base_add.before.value);
        if base_delta != 0 {
            self.report.transitions_with_changed_base =
                self.report.transitions_with_changed_base.saturating_add(1);
        }
        let raw_percent_delta = raw_percent
            .after
            .value
            .saturating_sub(raw_percent.before.value);
        let provider = session.provider_at(status);
        let provider_is_target = provider
            .resolved_entity_uuid
            .map(|entity_uuid| entity_uuid == status.target.entity_uuid.0);
        let key = PercentFamilyFormulaKey {
            state: status_state(status.state),
            stacks: status.stacks,
            raw_percent_delta_units: raw_percent_delta,
            base_delta_units: base_delta,
            before_raw_extra_add: extra_add.map(|value| value.before.value),
            after_raw_extra_add: extra_add.map(|value| value.after.value),
            intermediate_delta_units: observed_intermediate_delta,
            predicted_intermediate_delta_units: predicted_intermediate_delta,
            intermediate_residual_units: intermediate_residual,
            predicted_nearest_intermediate_delta_units: predicted_nearest_intermediate_delta,
            nearest_intermediate_residual_units: nearest_intermediate_residual,
            before_raw_extra_percent: extra_percent.map(|value| value.before.value),
            after_raw_extra_percent: extra_percent.map(|value| value.after.value),
            final_delta_units: observed_final_delta,
            predicted_final_delta_units: predicted_final_delta,
            final_residual_units: final_residual,
            provider_resolution: provider.resolution,
            provider_is_target,
        };
        let aggregate = self.aggregates.entry(key).or_default();
        aggregate.count = aggregate.count.saturating_add(1);
        if aggregate.examples.len() < example_limit {
            aggregate.examples.push(PercentFamilyFormulaExample {
                rlog: status.rlog.clone(),
                session_id: status.session_id.clone(),
                run_ordinal: status.run_ordinal,
                status_sequence: status.sequence,
                wire_capture_sequence: message.capture_sequence,
                effect_id: status.effect_id,
                instance_id: status.instance_id,
                state: status_state(status.state),
                stacks: status.stacks,
                origin: status.origin,
                target_entity_uuid: status.target.entity_uuid.0,
                raw_source_entity_uuid: status.source.map(|value| value.entity_uuid.0),
                resolved_provider_entity_uuid: provider.resolved_entity_uuid,
                provider_resolution: provider.resolution,
                provider_display_name: provider
                    .snapshot
                    .and_then(|snapshot| snapshot.display_name.clone()),
                provider_is_target,
                before_final_value: final_value.before.value,
                after_final_value: final_value.after.value,
                before_intermediate_value: intermediate_value.before.value,
                after_intermediate_value: intermediate_value.after.value,
                before_base_add: base_add.before.value,
                after_base_add: base_add.after.value,
                before_raw_extra_add: extra_add.map(|value| value.before.value),
                after_raw_extra_add: extra_add.map(|value| value.after.value),
                before_raw_percent: raw_percent.before.value,
                after_raw_percent: raw_percent.after.value,
                before_raw_extra_percent: extra_percent.map(|value| value.before.value),
                after_raw_extra_percent: extra_percent.map(|value| value.after.value),
                observed_intermediate_delta,
                predicted_intermediate_delta,
                intermediate_residual_units: intermediate_residual,
                predicted_nearest_intermediate_delta,
                nearest_intermediate_residual_units: nearest_intermediate_residual,
                observed_final_delta,
                predicted_final_delta,
                final_residual_units: final_residual,
            });
        }
    }

    fn finish(mut self) -> PercentFamilyFormulaReport {
        self.report.aggregates = self
            .aggregates
            .into_iter()
            .map(|(key, value)| PercentFamilyFormulaAggregate {
                state: key.state,
                stacks: key.stacks,
                raw_percent_delta_units: key.raw_percent_delta_units,
                base_delta_units: key.base_delta_units,
                before_raw_extra_add: key.before_raw_extra_add,
                after_raw_extra_add: key.after_raw_extra_add,
                intermediate_delta_units: key.intermediate_delta_units,
                predicted_intermediate_delta_units: key.predicted_intermediate_delta_units,
                intermediate_residual_units: key.intermediate_residual_units,
                predicted_nearest_intermediate_delta_units: key
                    .predicted_nearest_intermediate_delta_units,
                nearest_intermediate_residual_units: key.nearest_intermediate_residual_units,
                before_raw_extra_percent: key.before_raw_extra_percent,
                after_raw_extra_percent: key.after_raw_extra_percent,
                final_delta_units: key.final_delta_units,
                predicted_final_delta_units: key.predicted_final_delta_units,
                final_residual_units: key.final_residual_units,
                provider_resolution: key.provider_resolution,
                provider_is_target: key.provider_is_target,
                count: value.count,
                examples: value.examples,
            })
            .collect();
        self.report
    }
}

fn scaled_percent_family(base_add: i64, raw_percent: i64) -> Option<i64> {
    let numerator =
        i128::from(base_add).checked_mul(i128::from(10_000_i64.checked_add(raw_percent)?))?;
    i64::try_from(numerator / 10_000).ok()
}

fn rounded_percent_family(base_add: i64, raw_percent: i64) -> Option<i64> {
    let numerator =
        i128::from(base_add).checked_mul(i128::from(10_000_i64.checked_add(raw_percent)?))?;
    let half = 5_000_i128;
    let rounded = if numerator >= 0 {
        numerator.checked_add(half)? / 10_000
    } else {
        numerator.checked_sub(half)? / 10_000
    };
    i64::try_from(rounded).ok()
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rDPS status attribute proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let mut sessions = Vec::new();
    for path in &args.rlogs {
        let bytes = fs::metadata(path)?.len();
        let sha256 = sha256_file(path)?;
        let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        let deployment_id = reader.header().region.identity.deployment_id.clone();
        let game_build = reader.header().region.client_build.clone();
        let protocol_pack_digest = reader.header().region.protocol_pack_digest.clone();
        if args
            .expected_deployment_id
            .as_deref()
            .is_some_and(|expected| expected != deployment_id)
        {
            return Err(format!(
                "{} contains deployment {deployment_id} but the proof requires {}",
                path.display(),
                args.expected_deployment_id.as_deref().unwrap_or_default()
            )
            .into());
        }
        if args
            .expected_game_build
            .as_deref()
            .is_some_and(|expected| expected != game_build)
        {
            return Err(format!(
                "{} contains client build {game_build} but the proof requires {}",
                path.display(),
                args.expected_game_build.as_deref().unwrap_or_default()
            )
            .into());
        }
        let mut session = SessionData::new(
            path,
            bytes,
            sha256,
            deployment_id,
            game_build,
            protocol_pack_digest,
        );
        while let Some(envelope) = reader.next_event()? {
            session.observe(
                &envelope,
                &args.effects,
                &args.attributes,
                &args.target_entities,
                args.expected_deployment_id.as_deref(),
                args.expected_game_build.as_deref(),
            )?;
        }
        session.finalize_status_wire_changes();
        sessions.push(session);
    }

    let mut selected_count_by_effect = BTreeMap::<i64, u64>::new();
    let mut selected_state_change_count_by_effect = BTreeMap::<i64, u64>::new();
    let mut reports = BTreeMap::<(i64, i32), AttributeAccumulator>::new();
    let selected_percent_family_specs = percent_family_specs()
        .into_iter()
        .filter(|spec| {
            [
                spec.final_attribute_id,
                spec.intermediate_attribute_id,
                spec.base_attribute_id,
                spec.raw_percent_attribute_id,
            ]
            .into_iter()
            .all(|attribute_id| args.attributes.contains(&attribute_id))
        })
        .collect::<Vec<_>>();
    let mut percent_family_reports = args
        .report_effects
        .iter()
        .copied()
        .map(|effect_id| {
            (
                effect_id,
                selected_percent_family_specs
                    .iter()
                    .copied()
                    .map(PercentFamilyFormulaAccumulator::new)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for effect_id in &args.report_effects {
        for attribute_id in &args.attributes {
            let mut accumulator = AttributeAccumulator::default();
            accumulator.report.attribute_id = *attribute_id;
            reports.insert((*effect_id, *attribute_id), accumulator);
        }
    }

    for session in &sessions {
        for status in &session.selected_statuses {
            *selected_count_by_effect
                .entry(status.effect_id)
                .or_default() += 1;
            if !args.report_effects.contains(&status.effect_id) {
                continue;
            }
            if !session.is_effective_status_transition(status) {
                continue;
            }
            *selected_state_change_count_by_effect
                .entry(status.effect_id)
                .or_default() += 1;
            if let Some(reports) = percent_family_reports.get_mut(&status.effect_id) {
                for report in reports {
                    report.observe(session, status, args.example_limit);
                }
            }
            for attribute_id in &args.attributes {
                let accumulator = reports
                    .get_mut(&(status.effect_id, *attribute_id))
                    .expect("selected report exists");
                accumulator.report.transitions_examined =
                    accumulator.report.transitions_examined.saturating_add(1);
                let Some(points) = session.attributes.get(&(
                    status.run_ordinal,
                    status.target.entity_uuid.0,
                    *attribute_id,
                )) else {
                    accumulator.report.missing_before =
                        accumulator.report.missing_before.saturating_add(1);
                    accumulator.report.missing_after_within_window = accumulator
                        .report
                        .missing_after_within_window
                        .saturating_add(1);
                    continue;
                };
                let split = points.partition_point(|point| point.sequence <= status.sequence);
                let before = match status.wire_message {
                    Some(message) => points[..split]
                        .iter()
                        .rev()
                        .find(|point| point.wire_message != Some(message)),
                    None => split.checked_sub(1).and_then(|index| points.get(index)),
                };
                let same_wire_after = status.wire_message.and_then(|message| {
                    points
                        .iter()
                        .rev()
                        .find(|point| point.wire_message == Some(message))
                });
                let after = same_wire_after.or_else(|| {
                    points.get(split).filter(|point| {
                        point.observed_micros.saturating_sub(status.observed_micros)
                            <= args.after_window_micros
                    })
                });
                let Some(before) = before else {
                    accumulator.report.missing_before =
                        accumulator.report.missing_before.saturating_add(1);
                    if after.is_none() {
                        accumulator.report.missing_after_within_window = accumulator
                            .report
                            .missing_after_within_window
                            .saturating_add(1);
                    }
                    continue;
                };
                let Some(after) = after else {
                    accumulator.report.missing_after_within_window = accumulator
                        .report
                        .missing_after_within_window
                        .saturating_add(1);
                    continue;
                };
                accumulator.report.complete_before_and_after = accumulator
                    .report
                    .complete_before_and_after
                    .saturating_add(1);

                let same_wire_attribute_update =
                    status.wire_message.is_some() && after.wire_message == status.wire_message;
                let competing = session
                    .statuses_by_target
                    .get(&(status.run_ordinal, status.target.entity_uuid.0))
                    .map(|points| {
                        points
                            .iter()
                            .filter(|other| {
                                other.sequence != status.sequence
                                    && other.effect_id != status.effect_id
                                    && session.is_effective_status_transition(other)
                                    && (status.wire_message.is_some()
                                        && other.wire_message == status.wire_message
                                        || other.sequence > before.sequence
                                            && other.sequence < after.sequence)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let isolated = competing.is_empty();
                if isolated {
                    accumulator.report.isolated_transitions =
                        accumulator.report.isolated_transitions.saturating_add(1);
                } else {
                    accumulator
                        .report
                        .transitions_with_competing_target_statuses = accumulator
                        .report
                        .transitions_with_competing_target_statuses
                        .saturating_add(1);
                }
                let raw_delta_units = after.value.saturating_sub(before.value);
                if args.exact_wire_isolated_aggregates_only
                    && (!isolated || !same_wire_attribute_update)
                {
                    continue;
                }
                let provider = session.provider_at(status);
                let provider_kind = provider.snapshot.map(|value| actor_kind(value.kind));
                let provider_class_id = provider.snapshot.and_then(|value| value.class_id);
                let provider_specialization_id =
                    provider.snapshot.and_then(|value| value.specialization_id);
                let provider_is_target = provider
                    .resolved_entity_uuid
                    .map(|entity_uuid| entity_uuid == status.target.entity_uuid.0);
                let target =
                    session.actor_snapshot_at(status.target.entity_uuid.0, status.sequence);
                let key = AggregateKey {
                    state: status_state(status.state),
                    raw_delta_units,
                    isolated,
                    provider_resolution: provider.resolution,
                    provider_kind,
                    provider_class_id,
                    provider_specialization_id,
                    provider_is_target,
                    same_wire_attribute_update,
                };
                let aggregate = accumulator.aggregates.entry(key).or_default();
                aggregate.count = aggregate.count.saturating_add(1);
                if aggregate.examples.len() < args.example_limit {
                    let competing_effect_ids = competing
                        .iter()
                        .map(|point| point.effect_id)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    let competing_status_transitions = competing
                        .iter()
                        .map(|point| CompetingStatusTransition {
                            sequence: point.sequence,
                            effect_id: point.effect_id,
                            instance_id: point.instance_id,
                            state: status_state(point.state),
                            stacks: point.stacks,
                            level: point.level,
                            part_id: point.part_id,
                            count: point.count,
                        })
                        .collect();
                    aggregate.examples.push(TransitionExample {
                        rlog: status.rlog.clone(),
                        session_id: status.session_id.clone(),
                        run_ordinal: status.run_ordinal,
                        status_sequence: status.sequence,
                        status_observed_micros: status.observed_micros,
                        wire_capture_sequence: status
                            .wire_message
                            .map(|message| message.capture_sequence),
                        effect_id: status.effect_id,
                        instance_id: status.instance_id,
                        origin: status.origin,
                        state: status_state(status.state),
                        stacks: status.stacks,
                        duration_millis: status.duration_millis,
                        level: status.level,
                        part_id: status.part_id,
                        count: status.count,
                        created_at_millis: status.created_at_millis,
                        raw_source_entity_uuid: status.source.map(|value| value.entity_uuid.0),
                        resolved_provider_entity_uuid: provider.resolved_entity_uuid,
                        provider_resolution: provider.resolution,
                        provider_kind,
                        provider_display_name: provider
                            .snapshot
                            .and_then(|value| value.display_name.clone()),
                        provider_class_id,
                        provider_specialization_id,
                        provider_is_target,
                        target_entity_uuid: status.target.entity_uuid.0,
                        target_actor_sequence: target.map(|value| value.sequence),
                        target_kind: target.map(|value| actor_kind(value.kind)),
                        target_display_name: target.and_then(|value| value.display_name.clone()),
                        target_class_id: target.and_then(|value| value.class_id),
                        target_specialization_id: target.and_then(|value| value.specialization_id),
                        attribute_id: *attribute_id,
                        before_sequence: before.sequence,
                        before_observed_micros: before.observed_micros,
                        before_age_micros: status
                            .observed_micros
                            .saturating_sub(before.observed_micros),
                        before_value: before.value,
                        before_decode: before.decode,
                        after_sequence: after.sequence,
                        after_observed_micros: after.observed_micros,
                        after_latency_micros: after
                            .observed_micros
                            .saturating_sub(status.observed_micros),
                        after_value: after.value,
                        after_decode: after.decode,
                        raw_delta_units,
                        same_wire_attribute_update,
                        isolated,
                        competing_status_transition_count: competing.len(),
                        competing_effect_ids,
                        competing_status_transitions,
                    });
                }
            }
        }
    }

    let mut effects = Vec::new();
    for effect_id in &args.report_effects {
        let mut attributes = Vec::new();
        for attribute_id in &args.attributes {
            let mut accumulator = reports
                .remove(&(*effect_id, *attribute_id))
                .expect("selected report exists");
            accumulator.report.aggregates = accumulator
                .aggregates
                .into_iter()
                .map(|(key, value)| TransitionAggregate {
                    state: key.state,
                    raw_delta_units: key.raw_delta_units,
                    isolated: key.isolated,
                    provider_resolution: key.provider_resolution,
                    provider_kind: key.provider_kind,
                    provider_class_id: key.provider_class_id,
                    provider_specialization_id: key.provider_specialization_id,
                    provider_is_target: key.provider_is_target,
                    same_wire_attribute_update: key.same_wire_attribute_update,
                    count: value.count,
                    examples: value.examples,
                })
                .collect();
            attributes.push(accumulator.report);
        }
        effects.push(EffectReport {
            effect_id: *effect_id,
            selected_status_events: selected_count_by_effect
                .get(effect_id)
                .copied()
                .unwrap_or_default(),
            selected_mechanic_state_changes: selected_state_change_count_by_effect
                .get(effect_id)
                .copied()
                .unwrap_or_default(),
            attributes,
            percent_family_formulas: percent_family_reports
                .remove(effect_id)
                .unwrap_or_default()
                .into_iter()
                .map(PercentFamilyFormulaAccumulator::finish)
                .collect(),
            active_stack_attribute_surfaces: if args.include_stack_surfaces {
                args.attributes
                    .iter()
                    .map(|attribute_id| {
                        active_stack_attribute_surface(
                            &sessions,
                            *effect_id,
                            *attribute_id,
                            args.example_limit,
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            },
        });
    }

    let wire_additive_equation_systems = args
        .attributes
        .iter()
        .map(|attribute_id| {
            wire_additive_attribute_report(
                &sessions,
                *attribute_id,
                &args.attributes,
                &args.report_effects,
                args.example_limit,
                args.include_source_status_context,
                args.include_target_status_context,
                args.include_selected_attribute_context,
            )
        })
        .collect::<Vec<_>>();
    let reversible_static_coefficient_proofs = reversible_static_coefficient_proofs(
        &wire_additive_equation_systems,
        &args.report_effects,
        &args.stateful_attributes,
    );
    let wire_stack_delta_equation_systems = args
        .attributes
        .iter()
        .map(|attribute_id| {
            wire_stack_delta_attribute_report(
                &sessions,
                *attribute_id,
                &args.report_effects,
                args.example_limit,
            )
        })
        .collect::<Vec<_>>();
    let reversible_per_stack_coefficient_proofs = reversible_per_stack_coefficient_proofs(
        &wire_stack_delta_equation_systems,
        &args.stateful_attributes,
    );
    let matched_lifecycle_coefficient_proofs = matched_lifecycle_coefficient_proofs(
        &sessions,
        &args.report_effects,
        &args.attributes,
        &args.stateful_attributes,
        args.example_limit,
    );
    let candidate_magnitude_proof_reports = candidate_magnitude_proof_reports(
        &args.watchlist_candidates,
        &reversible_static_coefficient_proofs,
        &reversible_per_stack_coefficient_proofs,
        &matched_lifecycle_coefficient_proofs,
        &args.non_attributable_context_attributes,
    );
    if let Some(output) = &args.transition_seed_output {
        let seeds = compact_transition_seed_bundle(&args, &wire_additive_equation_systems);
        write_pretty_json(output, &seeds)?;
    }
    if args.transition_seed_only {
        return Ok(());
    }
    let audit = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-status-attribute-proof",
        expected_deployment_id: args.expected_deployment_id.clone(),
        expected_game_build: args.expected_game_build.clone(),
        watchlist_source_inputs: args.watchlist_source_inputs.clone(),
        watchlist_buff_table: args.watchlist_buff_table.clone(),
        watchlist_candidates: args.watchlist_candidates.clone(),
        policy: AuditPolicy {
            runtime_use: "offline_research_only_not_loaded_by_live_parser",
            session_scope: "each_rlog_is_processed_independently",
            run_scope: "attributes_statuses_and_owner_links_never_cross_run_ordinals",
            provider_resolution: "exact_source_then_exact_owner_link_observed_within_same_run",
            actor_metadata_resolution: "fieldwise cumulative exact ActorEvent observations at or before the evidence sequence within the same recording; absent sparse fields never erase prior observations, and a class change invalidates specialization unless the same event re-observes it",
            before_value: "last_exact_attribute_value_before_the_status_wire_message_in_same_session_run_and_target",
            after_value: "last_exact_value_in_the_same_wire_message_when_present_otherwise_first_exact_value_within_configured_latency",
            isolation: "no_different_effect_transition_on_same_target_in_the_status_wire_message_or_between_attribute_samples",
            attribute_units: "raw_exact_attribute_units_only_no_percent_flat_or_multiplier_conversion",
            formula_inference: false,
            unresolved_evidence_is_hidden: false,
            wire_message_state: "same_capture_connection_and_stream_are_one_message_and_are_compared_as_message_start_to_message_end",
            duplicate_status_transitions: "same_effect_duplicates_do_not_make_a_transition_competing_but_remain_counted_as_packet_evidence",
            snapshot_status_rows: "a status row is causal for attribute isolation only when exact instance presence, stacks, level, part, count, origin, or source changes; unchanged re-emitted snapshots remain counted as selected status events",
            wire_net_state: "status rows sharing one wire message are replayed as a batch; only the final representative of an effect whose aggregate active mechanic state differs before versus after the complete message is causal, so apply-remove churn with zero net state cannot confound an attribute transition",
            active_stack_surface: "for each exact attribute sample, replay the selected effect lifecycle through the end of that wire message and retain the post-message stack count; this correlation is evidence only and never suppresses HP-dependent actions or attributes",
            active_stack_surfaces_generated: args.include_stack_surfaces,
            aggregate_scope: if args.exact_wire_isolated_aggregates_only {
                "detailed_aggregates_include_only_isolated_same-wire_attribute_updates; complete/missing/isolated/competing counters still cover every examined transition"
            } else {
                "detailed_aggregates_include_every_complete_before-and-after transition"
            },
            wire_additive_equations: "Every status wire message with an exact same-wire attribute update is replayed from its complete pre-message active state to its complete post-message active state. An additive equation is emitted only when every changed effect is an unambiguous zero-to-one or one-to-zero presence transition. Stack, refresh, multi-instance, and other non-binary mechanics are counted and retained outside the equation system. Variable identity retains effect, origin, level, part, stacks, and count; provider UUIDs remain in examples and are intentionally not part of the static-coefficient hypothesis.",
            reversible_static_coefficient_gate: "A coefficient is proven only from single-term wire equations when the normalized coefficient is constant, both application and removal are observed, and evidence spans at least two independent session-run contexts. Stateful pools are excluded. Exact external stat coefficients remain blocked from rDPS until a separate downstream damage counterfactual is proven.",
            wire_stack_delta_equations: "A stack equation is emitted only when one reported effect is the sole status mechanic changed in a wire message, it has at most one exact active instance, its before and after stack counts are present, all non-stack fingerprint fields remain identical, and the selected attribute is updated in that same wire message. Absence is exact stack zero. Missing counts and ambiguous transitions remain counted and visible.",
            reversible_per_stack_coefficient_gate: "A per-stack coefficient is proven only when every exact attribute delta divides by its signed stack delta to the same integer coefficient, both increasing and decreasing stack steps are observed, and evidence spans at least two independent session-run contexts. Stateful pools are excluded. Exact external coefficients remain blocked from rDPS until the downstream damage counterfactual is separately proven.",
            stateful_attribute_exclusions: args.stateful_attributes.iter().copied().collect(),
            non_attributable_context_attributes: args
                .non_attributable_context_attributes
                .iter()
                .copied()
                .collect(),
            selected_attributes_are_formula_context_not_credit_authority: true,
            matched_lifecycle_gate: "The same status instance must be observed as a single-effect application and single-effect removal with exact same-wire attribute updates whose deltas are opposites. At least two independently identified status instances must agree on one coefficient. Unpaired, ambiguous, and contradictory instances remain counted and visible.",
        },
        selected_effect_ids: args.effects.iter().copied().collect(),
        reported_effect_ids: args.report_effects.iter().copied().collect(),
        selected_attribute_ids: args.attributes.iter().copied().collect(),
        non_attributable_context_attribute_ids: args
            .non_attributable_context_attributes
            .iter()
            .copied()
            .collect(),
        selected_target_entity_uuids: args.target_entities.iter().copied().collect(),
        after_window_micros: args.after_window_micros,
        sessions: sessions.iter().map(SessionData::summary).collect(),
        effects,
        wire_additive_equation_systems,
        reversible_static_coefficient_proofs,
        wire_stack_delta_equation_systems,
        reversible_per_stack_coefficient_proofs,
        matched_lifecycle_coefficient_proofs,
        candidate_magnitude_proof_reports,
    };
    let output = args
        .output
        .as_deref()
        .ok_or("--output is required unless --transition-seed-only is used")?;
    write_pretty_json(output, &audit)?;
    Ok(())
}

fn compact_transition_seed_bundle(
    args: &Arguments,
    reports: &[WireAdditiveAttributeReport],
) -> CompactTransitionSeedBundle {
    let mut exact_single_term_equation_occurrences = 0_u64;
    let mut transitions = Vec::new();
    for report in reports {
        for equation in &report.equations {
            let [term] = equation.terms.as_slice() else {
                continue;
            };
            if !args.report_effects.contains(&term.effect_id) {
                continue;
            }
            exact_single_term_equation_occurrences =
                exact_single_term_equation_occurrences.saturating_add(equation.count);
            transitions.extend(
                equation
                    .examples
                    .iter()
                    .map(|example| CompactTransitionSeed {
                        effect_id: term.effect_id,
                        attribute_id: report.attribute_id,
                        term: term.clone(),
                        raw_attribute_delta: equation.raw_attribute_delta,
                        rlog: example.rlog.clone(),
                        session_id: example.session_id.clone(),
                        run_ordinal: example.run_ordinal,
                        target_entity_uuid: example.target_entity_uuid,
                        wire_capture_sequence: example.wire_capture_sequence,
                        wire_observed_micros: example.wire_observed_micros,
                        before_value: example.before_value,
                        after_value: example.after_value,
                        source_entity_uuids: example.source_entity_uuids.clone(),
                    }),
            );
        }
    }
    transitions.sort_by(|left, right| {
        left.rlog
            .cmp(&right.rlog)
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.run_ordinal.cmp(&right.run_ordinal))
            .then_with(|| left.wire_capture_sequence.cmp(&right.wire_capture_sequence))
            .then_with(|| left.attribute_id.cmp(&right.attribute_id))
            .then_with(|| left.effect_id.cmp(&right.effect_id))
    });
    CompactTransitionSeedBundle {
        schema_version: 1,
        generated_by: "rlogs-bpsr-rdps-status-attribute-proof",
        expected_deployment_id: args.expected_deployment_id.clone(),
        expected_game_build: args.expected_game_build.clone(),
        policy: "exact single-term same-wire recipient attribute transitions only; retained examples are locally observable counterfactual seeds, never a general formula, attribution, runtime, or UI authority",
        source_rlogs: args
            .rlogs
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        selected_effect_ids: args.report_effects.iter().copied().collect(),
        selected_attribute_ids: args.attributes.iter().copied().collect(),
        example_limit: args.example_limit,
        exact_single_term_equation_occurrences,
        retained_transition_seeds: transitions.len(),
        all_equation_occurrences_retained: u64::try_from(transitions.len()).ok()
            == Some(exact_single_term_equation_occurrences),
        transitions,
    }
}

fn write_pretty_json<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", path.display());
    Ok(())
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

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<(i64, &'static str)> {
    match attribute.decoded.as_ref() {
        Some(EntityAttributeValue::Integer(value)) => {
            return Some((*value, "canonical_decoded_integer"));
        }
        Some(EntityAttributeValue::Text(_)) | Some(EntityAttributeValue::Position { .. }) => {
            return None;
        }
        None => {}
    }
    match decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value) {
        Some(EntityAttributeValue::Integer(value)) => {
            Some((value, "current_exact_id_gated_raw_decode"))
        }
        Some(EntityAttributeValue::Text(_)) | Some(EntityAttributeValue::Position { .. }) => None,
        None => decode_varint(&attribute.raw_value)
            .map(|value| (value as i64, "raw_protobuf_varint_i64_bit_pattern")),
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

const fn status_state(state: StatusState) -> &'static str {
    match state {
        StatusState::Applied => "applied",
        StatusState::Refreshed => "refreshed",
        StatusState::Stacked => "stacked",
        StatusState::Consumed => "consumed",
        StatusState::Removed => "removed",
    }
}

const fn actor_kind(kind: ActorKind) -> &'static str {
    match kind {
        ActorKind::Player => "player",
        ActorKind::Monster => "monster",
        ActorKind::Npc => "npc",
        ActorKind::SceneObject => "scene_object",
        ActorKind::Zone => "zone",
        ActorKind::Projectile => "projectile",
        ActorKind::Pet => "pet",
        ActorKind::TrainingDummy => "training_dummy",
        ActorKind::Drop => "drop",
        ActorKind::Field => "field",
        ActorKind::Trap => "trap",
        ActorKind::Collection => "collection",
        ActorKind::StaticObject => "static_object",
        ActorKind::Vehicle => "vehicle",
        ActorKind::Toy => "toy",
        ActorKind::Housing => "housing",
        ActorKind::Unknown(_) => "unknown",
    }
}

struct Arguments {
    expected_deployment_id: Option<String>,
    expected_game_build: Option<String>,
    watchlist_source_inputs: Option<ProofInputs>,
    watchlist_buff_table: Option<ProofInputArtifact>,
    watchlist_candidates: Vec<ProofWatchlistCandidate>,
    effects: BTreeSet<i64>,
    report_effects: BTreeSet<i64>,
    attributes: BTreeSet<i32>,
    target_entities: BTreeSet<i64>,
    stateful_attributes: BTreeSet<i32>,
    non_attributable_context_attributes: BTreeSet<i32>,
    rlogs: Vec<PathBuf>,
    output: Option<PathBuf>,
    transition_seed_output: Option<PathBuf>,
    transition_seed_only: bool,
    after_window_micros: u64,
    example_limit: usize,
    include_stack_surfaces: bool,
    exact_wire_isolated_aggregates_only: bool,
    include_source_status_context: bool,
    include_target_status_context: bool,
    include_selected_attribute_context: bool,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let watchlist = take_optional_value(&mut values, "--watchlist")?
        .map(|path| load_watchlist(Path::new(&path)))
        .transpose()?;
    let explicit_deployment_id = take_optional_value(&mut values, "--deployment")?
        .map(|value| value.to_string_lossy().into_owned());
    let explicit_game_build = take_optional_value(&mut values, "--build")?
        .map(|value| value.to_string_lossy().into_owned());
    if let (Some(explicit), Some(watchlist)) = (&explicit_deployment_id, &watchlist)
        && explicit != &watchlist.deployment_id
    {
        return Err(format!(
            "--deployment {explicit} conflicts with watchlist deployment {}",
            watchlist.deployment_id
        ));
    }
    if let (Some(explicit), Some(watchlist)) = (&explicit_game_build, &watchlist)
        && explicit != &watchlist.game_build
    {
        return Err(format!(
            "--build {explicit} conflicts with watchlist build {}",
            watchlist.game_build
        ));
    }
    let mut effects: BTreeSet<i64> = watchlist
        .as_ref()
        .map(|watchlist| watchlist.selected_effect_ids.iter().copied().collect())
        .unwrap_or_default();
    effects.extend(take_repeated::<i64>(&mut values, "--effect")?);
    let mut report_effects = take_repeated::<i64>(&mut values, "--report-effect")?;
    if report_effects.is_empty()
        && let Some(watchlist) = &watchlist
    {
        report_effects.extend(watchlist.reported_effect_ids.iter().copied());
    }
    let mut attributes: BTreeSet<i32> = watchlist
        .as_ref()
        .map(|watchlist| watchlist.selected_attribute_ids.iter().copied().collect())
        .unwrap_or_default();
    attributes.extend(take_repeated::<i32>(&mut values, "--attribute")?);
    let target_entities = take_repeated::<i64>(&mut values, "--target-entity")?;
    let mut stateful_attributes: BTreeSet<i32> = watchlist
        .as_ref()
        .map(|watchlist| watchlist.stateful_attribute_ids.iter().copied().collect())
        .unwrap_or_default();
    stateful_attributes.extend(take_repeated::<i32>(&mut values, "--stateful-attribute")?);
    if stateful_attributes.is_empty() {
        stateful_attributes.extend([11_310, 20_010]);
    }
    let non_attributable_context_attributes: BTreeSet<i32> = watchlist
        .as_ref()
        .map(|watchlist| {
            watchlist
                .non_attributable_context_attribute_ids
                .iter()
                .copied()
                .collect()
        })
        .unwrap_or_default();
    let rlogs = take_paths(&mut values, "--rlog")?;
    let output = take_optional_value(&mut values, "--output")?.map(PathBuf::from);
    let transition_seed_output =
        take_optional_value(&mut values, "--transition-seed-output")?.map(PathBuf::from);
    let transition_seed_only = take_flag(&mut values, "--transition-seed-only");
    let after_window_micros = take_optional_value(&mut values, "--after-window-micros")?
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<u64>()
                .map_err(|_| "--after-window-micros requires a non-negative integer".to_owned())
        })
        .transpose()?
        .or_else(|| {
            watchlist
                .as_ref()
                .and_then(|value| value.after_window_micros)
        })
        .unwrap_or(DEFAULT_AFTER_WINDOW_MICROS);
    let example_limit = take_optional_value(&mut values, "--example-limit")?
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<usize>()
                .map_err(|_| "--example-limit requires a non-negative integer".to_owned())
        })
        .transpose()?
        .or_else(|| watchlist.as_ref().and_then(|value| value.example_limit))
        .unwrap_or(DEFAULT_EXAMPLE_LIMIT);
    let include_stack_surfaces = !take_flag(&mut values, "--omit-stack-surfaces");
    let exact_wire_isolated_aggregates_only =
        take_flag(&mut values, "--exact-wire-isolated-aggregates-only");
    let include_source_status_context = take_flag(&mut values, "--source-status-context");
    let include_target_status_context = take_flag(&mut values, "--target-status-context");
    let include_selected_attribute_context = take_flag(&mut values, "--selected-attribute-context");
    if effects.is_empty() || attributes.is_empty() || rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    if report_effects.is_empty() {
        report_effects = effects.clone();
    }
    if !report_effects.is_subset(&effects) {
        return Err("every --report-effect must also be selected with --effect".to_owned());
    }
    if transition_seed_only && transition_seed_output.is_none() {
        return Err("--transition-seed-only requires --transition-seed-output".to_owned());
    }
    if !transition_seed_only && output.is_none() {
        return Err(usage());
    }
    Ok(Arguments {
        expected_deployment_id: explicit_deployment_id.or_else(|| {
            watchlist
                .as_ref()
                .map(|watchlist| watchlist.deployment_id.clone())
        }),
        expected_game_build: explicit_game_build.or_else(|| {
            watchlist
                .as_ref()
                .map(|watchlist| watchlist.game_build.clone())
        }),
        watchlist_source_inputs: watchlist
            .as_ref()
            .map(|watchlist| watchlist.source_inputs.clone()),
        watchlist_buff_table: watchlist
            .as_ref()
            .map(|watchlist| watchlist.buff_table.clone()),
        watchlist_candidates: watchlist
            .as_ref()
            .map(|watchlist| watchlist.candidates.clone())
            .unwrap_or_default(),
        effects,
        report_effects,
        attributes,
        target_entities,
        stateful_attributes,
        non_attributable_context_attributes,
        rlogs,
        output,
        transition_seed_output,
        transition_seed_only,
        after_window_micros,
        example_limit,
        include_stack_surfaces,
        exact_wire_isolated_aggregates_only,
        include_source_status_context,
        include_target_status_context,
        include_selected_attribute_context,
    })
}

fn load_watchlist(path: &Path) -> Result<ProofWatchlist, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read proof watchlist {}: {error}", path.display()))?;
    parse_watchlist(&bytes, &path.display().to_string())
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

fn valid_input_artifact(artifact: &ProofInputArtifact) -> bool {
    !artifact.file.trim().is_empty()
        && artifact.bytes > 0
        && artifact.sha256.len() == 64
        && artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_watchlist(bytes: &[u8], display: &str) -> Result<ProofWatchlist, String> {
    let watchlist: ProofWatchlist = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid proof watchlist {display}: {error}"))?;
    let lifecycle_effect_ids = watchlist
        .candidates
        .iter()
        .flat_map(|candidate| candidate.lifecycle_effects.iter())
        .map(|effect| effect.effect_id)
        .collect::<BTreeSet<_>>();
    if watchlist.schema_version != WATCHLIST_SCHEMA_VERSION
        || watchlist.deployment_id.trim().is_empty()
        || watchlist.game_build.trim().is_empty()
        || !valid_input_artifact(&watchlist.buff_table)
        || !valid_input_artifact(&watchlist.source_inputs.classification)
        || !valid_input_artifact(&watchlist.source_inputs.contribution)
        || !valid_input_artifact(&watchlist.source_inputs.recount)
        || !valid_input_artifact(&watchlist.source_inputs.value_proof)
        || watchlist.selected_effect_ids.is_empty()
        || watchlist.reported_effect_ids.is_empty()
        || watchlist.selected_attribute_ids.is_empty()
        || watchlist.non_attributable_context_attribute_ids.is_empty()
        || watchlist.candidates.is_empty()
        || watchlist
            .reported_effect_ids
            .iter()
            .any(|effect_id| !watchlist.selected_effect_ids.contains(effect_id))
        || watchlist
            .reported_effect_ids
            .iter()
            .any(|effect_id| !lifecycle_effect_ids.contains(effect_id))
        || watchlist
            .non_attributable_context_attribute_ids
            .iter()
            .any(|attribute_id| !watchlist.selected_attribute_ids.contains(attribute_id))
        || watchlist.candidates.iter().any(|candidate| {
            let declared = candidate
                .declared_effect_references
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            let verified = candidate
                .effect_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            let rejected = candidate
                .rejected_effect_references
                .iter()
                .map(|reference| reference.effect_id)
                .collect::<BTreeSet<_>>();
            candidate.declared_effect_references.is_empty()
                || candidate.effect_ids.is_empty()
                || candidate.lifecycle_effects.is_empty()
                || candidate.selected_attribute_ids.is_empty()
                || candidate.static_value_state.trim().is_empty()
                || candidate
                    .rejected_effect_references
                    .iter()
                    .any(|reference| reference.reason.trim().is_empty())
                || !verified.is_disjoint(&rejected)
                || declared != verified.union(&rejected).copied().collect()
                || candidate
                    .selected_attribute_ids
                    .iter()
                    .any(|attribute_id| !watchlist.selected_attribute_ids.contains(attribute_id))
                || candidate.effect_ids.iter().any(|effect_id| {
                    !watchlist.selected_effect_ids.contains(effect_id)
                        || !candidate
                            .lifecycle_effects
                            .iter()
                            .any(|effect| effect.effect_id == *effect_id)
                })
                || candidate.lifecycle_effects.iter().any(|effect| {
                    !matches!(
                        effect.proof_model.as_str(),
                        "exact-binary-presence" | "exact-stack-delta"
                    ) || effect.declared_max_stacks.is_some_and(|value| value < 1)
                })
        })
    {
        return Err(format!(
            "proof watchlist {display} has an unsupported or incomplete shape"
        ));
    }
    Ok(watchlist)
}

fn take_repeated<T>(values: &mut Vec<OsString>, flag: &str) -> Result<BTreeSet<T>, String>
where
    T: std::str::FromStr + Ord,
{
    let mut result = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == flag) {
        if position + 1 >= values.len() {
            return Err(format!("{flag} requires a numeric value"));
        }
        let raw = values.remove(position + 1);
        values.remove(position);
        result.insert(
            raw.to_string_lossy()
                .parse::<T>()
                .map_err(|_| format!("{flag} requires a numeric value"))?,
        );
    }
    Ok(result)
}

fn take_paths(values: &mut Vec<OsString>, flag: &str) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    while let Some(position) = values.iter().position(|value| value == flag) {
        if position + 1 >= values.len() {
            return Err(format!("{flag} requires a path"));
        }
        result.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    Ok(result)
}

fn take_flag(values: &mut Vec<OsString>, flag: &str) -> bool {
    let mut found = false;
    while let Some(position) = values.iter().position(|value| value == flag) {
        values.remove(position);
        found = true;
    }
    found
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(Some(value))
}

fn usage() -> String {
    "usage: rlogs-bpsr-rdps-status-attribute-proof [--watchlist <build-locked-watchlist.json>] [--deployment <deployment-id>] [--build <client-build>] [--effect <observed-id> ...] [--report-effect <reported-id> ...] [--attribute <id> ...] [--target-entity <uuid> ...] [--stateful-attribute <id> ...] --rlog <current-decoder.rlog> [--rlog <current-decoder.rlog> ...] [--output <audit.json> | --transition-seed-only --transition-seed-output <compact.json>] [--after-window-micros <micros>] [--example-limit <count>] [--omit-stack-surfaces] [--exact-wire-isolated-aggregates-only] [--source-status-context] [--target-status-context] [--selected-attribute-context]".to_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
    };

    use rlogs_events::{
        ActorEvent, ActorId, ActorKind, ActorState, EntityRef, EntityUuid, StatusState,
    };

    use super::{
        ActiveStatusMechanicState, Arguments, SessionData, StackTransitionExclusion, StatusPoint,
        WireAdditiveAttributeReport, WireAdditiveEquation, WireAdditiveEquationExample,
        WireAdditiveTerm, WireMessageKey, compact_transition_seed_bundle,
        exact_coefficient_per_stack, exact_single_instance_stack_transition, merge_actor_snapshot,
        parse_watchlist, percent_family_specs, scaled_percent_family,
        update_active_effect_instance,
    };

    fn actor_event(class_id: Option<i32>, specialization_id: Option<i32>) -> ActorEvent {
        ActorEvent {
            actor: EntityRef {
                actor_id: ActorId(2),
                entity_uuid: EntityUuid(40_581_726_848),
            },
            state: ActorState::Updated,
            entity_type_id: 10,
            kind: ActorKind::Player,
            monster_id: None,
            character_id: None,
            display_name: None,
            class_id,
            specialization_id,
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: Default::default(),
        }
    }

    #[test]
    fn sparse_actor_updates_retain_exact_prior_class_and_specialization() {
        let first = merge_actor_snapshot(None, 10, &actor_event(Some(5), Some(110)));
        let sparse = merge_actor_snapshot(Some(&first), 20, &actor_event(None, None));
        assert_eq!(sparse.sequence, 20);
        assert_eq!(sparse.class_id, Some(5));
        assert_eq!(sparse.specialization_id, Some(110));
    }

    #[test]
    fn exact_class_change_invalidates_unobserved_specialization() {
        let first = merge_actor_snapshot(None, 10, &actor_event(Some(5), Some(110)));
        let changed = merge_actor_snapshot(Some(&first), 20, &actor_event(Some(9), None));
        assert_eq!(changed.class_id, Some(9));
        assert_eq!(changed.specialization_id, None);
    }

    #[test]
    fn compact_transition_seed_requires_all_retained_single_term_occurrences() {
        let args = Arguments {
            expected_deployment_id: Some("global".to_owned()),
            expected_game_build: Some("24687926".to_owned()),
            watchlist_source_inputs: None,
            watchlist_buff_table: None,
            watchlist_candidates: Vec::new(),
            effects: BTreeSet::from([2_207_252]),
            report_effects: BTreeSet::from([2_207_252]),
            attributes: BTreeSet::from([11_030]),
            target_entities: BTreeSet::new(),
            stateful_attributes: BTreeSet::new(),
            non_attributable_context_attributes: BTreeSet::new(),
            rlogs: vec![PathBuf::from("one.rlog")],
            output: None,
            transition_seed_output: Some(PathBuf::from("seeds.json")),
            transition_seed_only: true,
            after_window_micros: 250_000,
            example_limit: 2,
            include_stack_surfaces: false,
            exact_wire_isolated_aggregates_only: true,
            include_source_status_context: false,
            include_target_status_context: false,
            include_selected_attribute_context: false,
        };
        let example =
            |wire_capture_sequence, before_value, after_value| WireAdditiveEquationExample {
                rlog: "one.rlog".to_owned(),
                session_id: "session".to_owned(),
                run_ordinal: 1,
                target_entity_uuid: 11,
                target_actor_sequence: None,
                target_kind: None,
                target_display_name: None,
                target_class_id: None,
                target_specialization_id: None,
                wire_capture_sequence,
                wire_observed_micros: 99,
                status_instances: Vec::new(),
                before_value,
                after_value,
                source_entity_uuids: vec![22],
                source_attribute_values_before: Vec::new(),
                source_attribute_values_nearest: Vec::new(),
                source_selected_attribute_values_before: Vec::new(),
                target_selected_attribute_values_before: Vec::new(),
                source_active_statuses: Vec::new(),
                target_active_statuses: Vec::new(),
                reported_effect_lifecycle_starts: Vec::new(),
            };
        let reports = vec![WireAdditiveAttributeReport {
            attribute_id: 11_030,
            equations: vec![WireAdditiveEquation {
                terms: vec![WireAdditiveTerm {
                    effect_id: 2_207_252,
                    origin: None,
                    level: None,
                    part_id: None,
                    stacks: Some(1),
                    count: None,
                    signed_presence_delta: 1,
                }],
                raw_attribute_delta: 798,
                count: 2,
                independent_run_contexts: 1,
                target_entity_count: 1,
                source_entity_count: 1,
                cross_actor_occurrences: 2,
                self_source_occurrences: 0,
                missing_source_occurrences: 0,
                examples: vec![example(7, 1_000, 1_798), example(8, 2_000, 2_798)],
            }],
            ..WireAdditiveAttributeReport::default()
        }];

        let bundle = compact_transition_seed_bundle(&args, &reports);
        assert_eq!(bundle.exact_single_term_equation_occurrences, 2);
        assert_eq!(bundle.retained_transition_seeds, 2);
        assert!(bundle.all_equation_occurrences_retained);
        assert_eq!(bundle.transitions[0].effect_id, 2_207_252);
        assert_eq!(bundle.transitions[0].attribute_id, 11_030);
        assert_eq!(bundle.transitions[0].source_entity_uuids, vec![22]);
    }

    #[test]
    fn defense_percent_families_follow_the_six_attribute_layout() {
        let specs = percent_family_specs();
        let physical = specs
            .iter()
            .find(|spec| spec.family == "physical_defense")
            .unwrap();
        assert_eq!(physical.final_attribute_id, 11_350);
        assert_eq!(physical.intermediate_attribute_id, 11_351);
        assert_eq!(physical.base_attribute_id, 11_352);
        assert_eq!(physical.raw_extra_add_attribute_id, 11_353);
        assert_eq!(physical.raw_percent_attribute_id, 11_354);
        assert_eq!(physical.raw_extra_percent_attribute_id, 11_355);

        let magic = specs
            .iter()
            .find(|spec| spec.family == "magic_defense")
            .unwrap();
        assert_eq!(magic.final_attribute_id, 11_360);
        assert_eq!(magic.intermediate_attribute_id, 11_361);
        assert_eq!(magic.base_attribute_id, 11_362);
        assert_eq!(magic.raw_extra_add_attribute_id, 11_363);
        assert_eq!(magic.raw_percent_attribute_id, 11_364);
        assert_eq!(magic.raw_extra_percent_attribute_id, 11_365);
    }

    #[test]
    fn build_locked_watchlist_keeps_effect_and_attribute_authority_separate() {
        let watchlist = parse_watchlist(
            br#"{
              "schema_version": 3,
              "deployment_id": "global",
              "game_build": "24568685",
              "source_inputs": {
                "classification": {"file":"ModifierClassificationRuntime.json","bytes":1,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                "contribution": {"file":"ModifierContributionRuntime.json","bytes":1,"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
                "recount": {"file":"ModifierRecountTable.json","bytes":1,"sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"},
                "value_proof": {"file":"ModifierValueProofRuntime.json","bytes":1,"sha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}
              },
              "buff_table": {"file":"BuffTable.json","bytes":1,"sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"},
              "selected_effect_ids": [2110143, 2110154],
              "reported_effect_ids": [2110143],
              "selected_attribute_ids": [11330, 11334, 11860, 11930],
              "non_attributable_context_attribute_ids": [11860],
              "stateful_attribute_ids": [11310, 20010],
              "after_window_micros": 250000,
              "example_limit": 8,
              "candidates": [{
                "source_rule_id": "mrs:test",
                "source_id": "buff-source:2110143",
                "declared_effect_references": [2110143, 3003300],
                "effect_ids": [2110143],
                "rejected_effect_references": [{
                  "effect_id": 3003300,
                  "reason": "not an exact current-build BuffTable row"
                }],
                "formula_terms": ["primaryAttack"],
                "selected_attribute_ids": [11330, 11334],
                "required_runtime_evidence": [],
                "static_value_state": "missing-value-proof",
                "static_value_proofs": [],
                "static_blockers": [],
                "lifecycle_effects": [{
                  "effect_id": 2110143,
                  "name": "Functional Amp",
                  "icon": "",
                  "repeat_add_rule": [3, 1],
                  "declared_max_stacks": 1,
                  "proof_model": "exact-binary-presence",
                  "destroy_param": [[0, 1]]
                }]
              }, {
                "source_rule_id": "mrs:context-only",
                "source_id": "buff-source:2110154",
                "declared_effect_references": [2110154],
                "effect_ids": [2110154],
                "rejected_effect_references": [],
                "formula_terms": ["primaryAttack"],
                "selected_attribute_ids": [11930],
                "required_runtime_evidence": [],
                "static_value_state": "missing-value-proof",
                "static_value_proofs": [],
                "static_blockers": [],
                "lifecycle_effects": [{
                  "effect_id": 2110154,
                  "name": null,
                  "icon": null,
                  "repeat_add_rule": [0, 1],
                  "declared_max_stacks": 1,
                  "proof_model": "exact-binary-presence",
                  "destroy_param": null
                }]
              }],
              "candidate_notes": "ignored by the analyzer"
            }"#,
            "test-watchlist.json",
        )
        .unwrap();

        assert_eq!(watchlist.game_build, "24568685");
        assert_eq!(watchlist.selected_effect_ids.len(), 2);
        assert_eq!(watchlist.reported_effect_ids, [2_110_143]);
        assert_eq!(
            watchlist.selected_attribute_ids,
            [11_330, 11_334, 11_860, 11_930]
        );
        assert_eq!(watchlist.non_attributable_context_attribute_ids, [11_860]);
    }

    fn status_point(
        sequence: u64,
        effect_id: i64,
        instance_id: i64,
        state: StatusState,
        stacks: u32,
        wire_message: WireMessageKey,
    ) -> StatusPoint {
        StatusPoint {
            rlog: "test.rlog".to_owned(),
            session_id: "test-session".to_owned(),
            run_ordinal: 1,
            sequence,
            observed_micros: sequence,
            effect_id,
            instance_id: Some(instance_id),
            origin: None,
            state,
            stacks: Some(stacks),
            duration_millis: None,
            level: Some(1),
            part_id: None,
            count: None,
            created_at_millis: None,
            source: Some(EntityRef {
                actor_id: ActorId(10),
                entity_uuid: EntityUuid(100),
            }),
            target: EntityRef {
                actor_id: ActorId(20),
                entity_uuid: EntityUuid(200),
            },
            wire_message: Some(wire_message),
            mechanic_state_changed: true,
        }
    }

    #[test]
    fn max_hp_percent_counterfactual_uses_raw_basis_points() {
        let before = scaled_percent_family(505_735, 5_894).unwrap();
        let after = scaled_percent_family(505_735, 6_144).unwrap();
        assert_eq!(after - before, 12_643);
    }

    #[test]
    fn max_hp_percent_counterfactual_preserves_integer_truncation() {
        assert_eq!(scaled_percent_family(505_735, 6_144), Some(816_458));
        assert_eq!(scaled_percent_family(505_735, 5_894), Some(803_815));
    }

    #[test]
    fn extra_percent_scales_the_intermediate_max_hp_delta() {
        let before_intermediate = scaled_percent_family(473_072, 2_450).unwrap();
        let after_intermediate = scaled_percent_family(473_072, 2_700).unwrap();
        assert_eq!(after_intermediate - before_intermediate, 11_827);

        let before_final = scaled_percent_family(588_980, 5_185).unwrap();
        let after_final = scaled_percent_family(600_807, 5_185).unwrap();
        assert_eq!(after_final - before_final, 17_959);
    }

    #[test]
    fn attack_percent_counterfactual_uses_raw_basis_points() {
        let before = scaled_percent_family(5_000, 1_600).unwrap();
        let after = scaled_percent_family(5_000, 1_960).unwrap();
        assert_eq!(after - before, 180);
    }

    #[test]
    fn consumed_status_keeps_its_post_change_stack_count_until_zero() {
        let mut active = BTreeMap::new();
        update_active_effect_instance(&mut active, 7, StatusState::Applied, Some(10));
        update_active_effect_instance(&mut active, 7, StatusState::Consumed, Some(9));
        assert_eq!(active.get(&7), Some(&Some(9)));

        update_active_effect_instance(&mut active, 7, StatusState::Consumed, Some(0));
        assert!(!active.contains_key(&7));
    }

    #[test]
    fn missing_stack_refresh_preserves_the_last_exact_count() {
        let mut active = BTreeMap::new();
        update_active_effect_instance(&mut active, 7, StatusState::Applied, Some(4));
        update_active_effect_instance(&mut active, 7, StatusState::Refreshed, None);
        assert_eq!(active.get(&7), Some(&Some(4)));
    }

    fn active_stack(stacks: Option<u32>) -> ActiveStatusMechanicState {
        ActiveStatusMechanicState {
            stacks,
            level: Some(1),
            part_id: None,
            count: None,
            origin: None,
            source_entity_uuid: Some(100),
        }
    }

    #[test]
    fn stack_coefficient_requires_exact_integer_units() {
        assert_eq!(exact_coefficient_per_stack(520, 10), Some(52));
        assert_eq!(exact_coefficient_per_stack(-156, -3), Some(52));
        assert_eq!(exact_coefficient_per_stack(521, 10), None);
        assert_eq!(exact_coefficient_per_stack(0, 0), None);
    }

    #[test]
    fn stack_transition_treats_absence_as_exact_zero() {
        let after = active_stack(Some(10));
        let (_, before_stacks, after_stacks, source) =
            exact_single_instance_stack_transition(2_110_077, &[], &[after])
                .unwrap()
                .unwrap();
        assert_eq!((before_stacks, after_stacks), (0, 10));
        assert_eq!(source, Some(100));
    }

    #[test]
    fn stack_transition_rejects_missing_or_changed_non_stack_identity() {
        assert_eq!(
            exact_single_instance_stack_transition(
                2_110_077,
                &[active_stack(Some(1))],
                &[active_stack(None)],
            ),
            Err(StackTransitionExclusion::MissingStackCount)
        );
        let mut changed_source = active_stack(Some(2));
        changed_source.source_entity_uuid = Some(101);
        assert_eq!(
            exact_single_instance_stack_transition(
                2_110_077,
                &[active_stack(Some(1))],
                &[changed_source],
            ),
            Err(StackTransitionExclusion::AmbiguousEffectOrInstance)
        );
    }

    #[test]
    fn same_wire_apply_remove_churn_is_not_a_mechanic_transition() {
        let wire = WireMessageKey {
            capture_sequence: 1,
            connection_id: 2,
            stream_id: 3,
        };
        let applied = status_point(10, 2302121, 7, StatusState::Applied, 1, wire);
        let removed = status_point(11, 2302121, 7, StatusState::Removed, 0, wire);
        let mut session = SessionData::default();
        session
            .statuses_by_target
            .insert((1, 200), vec![applied.clone(), removed.clone()]);

        session.finalize_status_wire_changes();

        assert!(!session.is_effective_status_transition(&applied));
        assert!(!session.is_effective_status_transition(&removed));
        assert!(session.wire_effect_transition_sequence.is_empty());
    }

    #[test]
    fn same_wire_identical_instance_replacement_preserves_prior_state() {
        let initial_wire = WireMessageKey {
            capture_sequence: 1,
            connection_id: 2,
            stream_id: 3,
        };
        let snapshot_wire = WireMessageKey {
            capture_sequence: 2,
            connection_id: 2,
            stream_id: 3,
        };
        let initial = status_point(10, 2302121, 7, StatusState::Applied, 1, initial_wire);
        let removed = status_point(20, 2302121, 7, StatusState::Removed, 0, snapshot_wire);
        let replacement = status_point(21, 2302121, 8, StatusState::Applied, 1, snapshot_wire);
        let mut session = SessionData::default();
        session.statuses_by_target.insert(
            (1, 200),
            vec![initial.clone(), removed.clone(), replacement.clone()],
        );

        session.finalize_status_wire_changes();

        assert!(session.is_effective_status_transition(&initial));
        assert!(!session.is_effective_status_transition(&removed));
        assert!(!session.is_effective_status_transition(&replacement));
        assert_eq!(session.wire_effect_transition_sequence.len(), 1);
    }
}
