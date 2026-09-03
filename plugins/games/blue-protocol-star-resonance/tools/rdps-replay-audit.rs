#![allow(clippy::redundant_guards, clippy::too_many_arguments)]

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use rlogs_combat::{
    ContributionDamageEvent, ContributionStatusEvent, ContributionStatusState,
    DamageContributionReducer, DamageContributionSummary, ExactDamageContributionEvent,
    ExactDamageContributionProjector, ExactRationalDamageContributionEvent,
};
use rlogs_events::{
    ActorKind, ActorState, CanonicalEvent, DamageEvent, DungeonEventKind, EncounterState,
    EvidenceSource, RunState, StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::{
    BpsrRemoteFactorLearner, BpsrStateDamageContributionProjector,
    HarmonyGraceFamilyRoundingDiagnostic, HarmonyGraceFormulaTrace,
    InspirationCombinedFormulaTrace, InspirationCombinedPipelineAudit,
    confirmed_damage_contribution_deployment_id, confirmed_damage_contribution_game_build,
    confirmed_damage_contribution_rules, proven_state_damage_contribution_effect_ids,
    state_damage_contribution_deployment_id, state_damage_contribution_formula_target_matches,
    state_damage_contribution_game_build, state_damage_contribution_target_matches,
    target_vulnerability_candidate_effect_ids,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RDPS_REPLAY_AUDIT_SCHEMA_VERSION: u16 = 31;
const COMPACT_REPLAY_RECEIPT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Serialize)]
struct ReplayAuditBundle {
    schema_version: u16,
    proof_scope: &'static str,
    attribution_mode: &'static str,
    inspiration_candidate_audit_enabled: bool,
    harmony_grace_candidate_audit_enabled: bool,
    mechanical_power_candidate_audit_enabled: bool,
    mechanical_power_tier0_candidate_audit_enabled: bool,
    target_vulnerability_candidate_audit_enabled: bool,
    runtime_rule_deployment: &'static str,
    runtime_rule_build: &'static str,
    rule_effect_ids: Vec<i64>,
    target_vulnerability_candidate_effect_ids: Vec<i64>,
    total_events: u64,
    elapsed_micros: u128,
    events_per_second: f64,
    relationship_catalog: Vec<CrossRunInfluenceRelationshipSummary>,
    reports: Vec<ReplayAuditReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inspiration_combined_reconciliation: Option<InspirationCombinedReconciliationReceipt>,
}

#[derive(Debug, Serialize)]
struct CompactReplayReceiptBundle {
    schema_version: u16,
    generated_by: &'static str,
    proof_scope: &'static str,
    attribution_mode: &'static str,
    runtime_rule_deployment: &'static str,
    runtime_rule_build: &'static str,
    rule_effect_ids: Vec<i64>,
    policy: CompactReplayReceiptPolicy,
    total_events: u64,
    total_damage_events: u64,
    total_attributed_damage_events: u64,
    total_attributed_bonus_damage: String,
    all_runtime_targets_match: bool,
    all_reports_conserved: bool,
    reports: Vec<CompactReplayReceipt>,
    content_sha256: String,
}

#[derive(Debug, Serialize)]
struct CompactReplayReceiptPolicy {
    canonical_integrity_seal_required: bool,
    exact_runtime_identity_required: bool,
    production_promoted_rules_only: bool,
    exact_party_conservation_required: bool,
    incomplete_actor_count_reported: bool,
    raw_packet_payloads_included: bool,
    source_paths_included: bool,
    runtime_authority_changed: bool,
}

#[derive(Debug, Serialize)]
struct CompactReplayReceipt {
    session_id: String,
    canonical_content_sha256: String,
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
    runtime_target_match: bool,
    event_count: u64,
    damage_event_count: u64,
    attributed_damage_event_count: u64,
    attributed_bonus_damage: i64,
    missing_source_status_count: u64,
    incomplete_rdps_actor_count: usize,
    emitted_contribution_events_by_effect: BTreeMap<i64, u64>,
    raw_damage: String,
    rdps_damage: String,
    contribution_given: String,
    contribution_received: String,
    conserved: bool,
}

#[derive(Debug, Deserialize)]
struct InspirationCombinedAuthorityArtifact {
    rlogs: Vec<InspirationCombinedAuthorityRlog>,
    integer_stage_counterfactual_coverage: InspirationCombinedAuthorityCoverage,
}

#[derive(Debug, Deserialize)]
struct InspirationCombinedAuthorityRlog {
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct InspirationCombinedAuthorityCoverage {
    critical_factor_event_records: Vec<InspirationCombinedAuthorityRecord>,
}

#[derive(Clone, Debug, Deserialize)]
struct InspirationCombinedAuthorityRecord {
    protocol_pack_digest: String,
    session_id: String,
    damage_sequence: u64,
    damage_observed_micros: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    type_flags: Option<i32>,
    reported_critical: Option<bool>,
    observed_damage: i64,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    path: String,
    critical_damage: Option<InspirationCombinedAuthorityFactor>,
    lucky_damage: Option<InspirationCombinedAuthorityFactor>,
    provider_entity_uuid: i64,
    provider_instance_id: Option<i64>,
    provider_level: i32,
    provider_origin_source_type_id: i32,
    provider_origin_source_config_id: i64,
    provider_critical_raw_delta: i64,
    provider_lucky_raw_delta: i64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct InspirationCombinedAuthorityFactor {
    value: i64,
}

#[derive(Debug)]
struct InspirationCombinedReconciliationAccumulator {
    source_path: String,
    authority_by_event: BTreeMap<(String, u64), InspirationCombinedAuthorityRecord>,
    decisions: Vec<InspirationCombinedDecisionTrace>,
    seen_events: BTreeSet<(String, u64)>,
    new_route_events: Vec<InspirationCombinedNewRouteEvent>,
}

#[derive(Debug, Serialize)]
struct InspirationCombinedReconciliationReceipt {
    schema_version: u16,
    effect_id: i64,
    full_locale_name: &'static str,
    affected_damage_id: i64,
    source_authority_path: String,
    identity_namespace: &'static str,
    authorized_event_count: u64,
    newly_authorized_event_count: u64,
    extended_authorized_event_count: u64,
    new_route_decision_count: u64,
    general_route_decision_count: u64,
    decision_event_count: u64,
    emitted_event_count: u64,
    suppressed_event_count: u64,
    missing_decision_count: u64,
    duplicate_decision_count: u64,
    identity_set_equal: bool,
    formula_trace_complete_count: u64,
    formula_trace_mismatch_count: u64,
    all_route_formula_trace_complete_count: u64,
    runtime_target_match_rlogs: u64,
    conserved_rlogs: u64,
    rational_projection_overflow_count: u64,
    runtime_authority: bool,
    runtime_authority_blockers: Vec<String>,
    decisions: Vec<InspirationCombinedDecisionTrace>,
    newly_authorized_events: Vec<InspirationCombinedNewRouteEvent>,
}

#[derive(Debug, Serialize)]
struct InspirationCombinedDecisionTrace {
    session_id: String,
    damage_sequence: u64,
    observed_micros: u64,
    decision: String,
    decision_gate: String,
    authority_identity_matches: bool,
    formula_trace_complete: bool,
    emitted_contribution_matches_formula_trace: bool,
    later_candidate_signature: Option<String>,
    pipeline_audit: Option<InspirationCombinedPipelineAuditReport>,
    emitted_exact_effect_ids: Vec<i64>,
    emitted_rational_effect_ids: Vec<i64>,
    authority: InspirationCombinedAuthorityRecordReport,
    formula_trace: Option<InspirationCombinedFormulaTraceReport>,
}

#[derive(Debug, Serialize)]
struct InspirationCombinedNewRouteEvent {
    session_id: String,
    damage_sequence: u64,
    observed_micros: u64,
    decision: String,
    decision_gate: String,
    formula_trace_complete: bool,
    emitted_contribution_matches_formula_trace: bool,
    pipeline_audit: Option<InspirationCombinedPipelineAuditReport>,
    formula_trace: InspirationCombinedFormulaTraceReport,
}

#[derive(Debug, Serialize)]
struct InspirationCombinedPipelineAuditReport {
    exact_candidate_count: usize,
    attack_contribution_count: usize,
    unresolved_attack_overlap: bool,
    team_luck_candidate: bool,
    inspiration_occurrence_candidate: bool,
    critical_cold_candidate: bool,
    thunderwind_candidate: bool,
    target_vulnerability_candidate_count: usize,
    remote_harmony_candidate: bool,
    fatal_spiral_candidate: bool,
    later_candidate_count: usize,
}

impl From<InspirationCombinedPipelineAudit> for InspirationCombinedPipelineAuditReport {
    fn from(audit: InspirationCombinedPipelineAudit) -> Self {
        Self {
            exact_candidate_count: audit.exact_candidate_count,
            attack_contribution_count: audit.attack_contribution_count,
            unresolved_attack_overlap: audit.unresolved_attack_overlap,
            team_luck_candidate: audit.team_luck_candidate,
            inspiration_occurrence_candidate: audit.inspiration_occurrence_candidate,
            critical_cold_candidate: audit.critical_cold_candidate,
            thunderwind_candidate: audit.thunderwind_candidate,
            target_vulnerability_candidate_count: audit.target_vulnerability_candidate_count,
            remote_harmony_candidate: audit.remote_harmony_candidate,
            fatal_spiral_candidate: audit.fatal_spiral_candidate,
            later_candidate_count: audit.later_candidate_count(),
        }
    }
}

#[derive(Debug, Serialize)]
struct InspirationCombinedAuthorityRecordReport {
    protocol_pack_digest: String,
    source_entity_uuid: String,
    target_entity_uuid: String,
    ability_id: i64,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    type_flags: Option<i32>,
    reported_critical: Option<bool>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    observed_damage: i64,
    provider_entity_uuid: String,
    provider_instance_id: Option<i64>,
    provider_level: i32,
    provider_origin_source_type_id: i32,
    provider_origin_source_config_id: i64,
    current_critical_damage_raw: i64,
    current_lucky_damage_raw: i64,
    provider_critical_chance_raw_delta: i64,
    provider_lucky_chance_raw_delta: i64,
}

#[derive(Debug, Serialize)]
struct InspirationCombinedFormulaTraceReport {
    effect_id: i64,
    provider_actor_id: u64,
    provider_entity_uuid: String,
    provider_instance_id: Option<i64>,
    provider_effect_level: i32,
    provider_origin_source_type_id: i32,
    provider_origin_source_config_id: i64,
    recipient_actor_id: u64,
    recipient_entity_uuid: String,
    ability_id: i64,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    packet_type_flags: Option<i32>,
    packet_reported_critical: Option<bool>,
    packet_normal_value: Option<i64>,
    packet_lucky_value: Option<i64>,
    observed_damage: i64,
    current_critical_chance_raw: i64,
    current_lucky_chance_raw: i64,
    current_critical_damage_raw: i64,
    current_lucky_damage_raw: i64,
    provider_chance_raw_delta: i64,
    provider_critical_damage_raw_delta: i64,
    provider_lucky_damage_raw_delta: i64,
    contribution_scope: String,
    contribution_numerator: String,
    contribution_denominator: String,
    ordered_prior_contributions: Vec<InspirationCombinedPriorContributionReport>,
    final_contribution_numerator: Option<String>,
    final_contribution_denominator: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspirationCombinedPriorContributionReport {
    effect_id: i64,
    provider_actor_id: u64,
    contribution_scope: String,
    numerator: String,
    denominator: String,
}

#[derive(Debug, Serialize)]
struct ReplayAuditReport {
    source_path: String,
    session_id: String,
    canonical_content_sha256: String,
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
    runtime_target_match: bool,
    candidate_audit_target_match: bool,
    event_count: u64,
    conserved: bool,
    /// Union of actors whose exact rDPS was known incomplete at any point in
    /// the run. The projector clears encounter state at terminal boundaries,
    /// so the audit retains the monotonic run receipt separately.
    incomplete_rdps_actor_ids: Vec<u64>,
    emitted_contribution_events_by_effect: BTreeMap<i64, u64>,
    harmony_grace_audit_gates: BTreeMap<String, u64>,
    harmony_grace_audit_examples: BTreeMap<String, Vec<String>>,
    harmony_grace_family_rounding_diagnostics: Vec<HarmonyGraceFamilyRoundingDiagnosticCount>,
    team_luck_audit_gates: BTreeMap<String, u64>,
    team_luck_audit_examples: BTreeMap<String, Vec<String>>,
    team_luck_selector_rows_by_source_actor: BTreeMap<u64, BTreeMap<String, u64>>,
    team_luck_suppressed_examples_by_source_actor: BTreeMap<u64, Vec<String>>,
    team_luck_candidate_projection_by_source_actor: BTreeMap<u64, i64>,
    functional_amp_audit_gates: BTreeMap<String, u64>,
    functional_amp_audit_examples: BTreeMap<String, Vec<String>>,
    stat_resonance_audit_gates: BTreeMap<String, u64>,
    stat_resonance_audit_examples: BTreeMap<String, Vec<String>>,
    fiery_battle_will_audit_gates: BTreeMap<String, u64>,
    fiery_battle_will_audit_examples: BTreeMap<String, Vec<String>>,
    mechanical_power_audit_gates: BTreeMap<String, u64>,
    mechanical_power_audit_examples: BTreeMap<String, Vec<String>>,
    mechanical_power_audit_actions: BTreeMap<String, Vec<MechanicalPowerAuditActionSummary>>,
    inspire_haste_audit_gates: BTreeMap<String, u64>,
    inspire_haste_audit_examples: BTreeMap<String, Vec<String>>,
    inspiration_audit_gates: BTreeMap<String, u64>,
    inspiration_audit_examples: BTreeMap<String, Vec<String>>,
    inspiration_occurrence_audit_gates: BTreeMap<String, u64>,
    inspiration_occurrence_audit_examples: BTreeMap<String, Vec<String>>,
    critical_cold_occurrence_audit_gates: BTreeMap<String, u64>,
    critical_cold_occurrence_audit_examples: BTreeMap<String, Vec<String>>,
    critical_cold_direct_candidate_count: u64,
    critical_cold_direct_projected_credit: i64,
    critical_cold_simultaneous_candidate_histogram: BTreeMap<String, u64>,
    critical_cold_joint_audit_gates: BTreeMap<String, u64>,
    critical_cold_joint_audit_examples: BTreeMap<String, Vec<String>>,
    critical_cold_suppressed_context_histogram: BTreeMap<String, u64>,
    critical_cold_suppressed_standalone_examples: Vec<String>,
    thunderwind_audit_gates: BTreeMap<String, u64>,
    thunderwind_audit_examples: BTreeMap<String, Vec<String>>,
    fatal_spiral_audit_gates: BTreeMap<String, u64>,
    fatal_spiral_audit_examples: BTreeMap<String, Vec<String>>,
    target_vulnerability_audit_gates: BTreeMap<String, u64>,
    target_vulnerability_audit_examples: BTreeMap<String, Vec<String>>,
    influence_relationships: Vec<InfluenceRelationshipSummary>,
    emitted_contribution_ledger: Vec<EmittedContributionLedgerEntry>,
    summary: DamageContributionSummary,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MechanicalPowerAuditActionKey {
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    attributed_source_actor_id: u64,
    attributed_source_entity_uuid: i64,
    direct_source_actor_id: Option<u64>,
    direct_source_entity_uuid: Option<i64>,
    packet_attacker_uuid: Option<i64>,
    packet_top_summoner_uuid: Option<i64>,
    packet_owner_id: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    target_actor_id: u64,
    target_entity_uuid: i64,
}

#[derive(Debug, Default)]
struct MechanicalPowerAuditActionAccumulator {
    event_count: u64,
    observed_amount_sum: i128,
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
    first_capture_sequence: Option<u64>,
    last_capture_sequence: Option<u64>,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
}

#[derive(Debug, Serialize)]
struct MechanicalPowerAuditActionSummary {
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    attributed_source_actor_id: u64,
    attributed_source_entity_uuid: i64,
    direct_source_actor_id: Option<u64>,
    direct_source_entity_uuid: Option<i64>,
    packet_attacker_uuid: Option<i64>,
    packet_top_summoner_uuid: Option<i64>,
    packet_owner_id: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    target_actor_id: u64,
    target_entity_uuid: i64,
    event_count: u64,
    observed_amount_sum: String,
    first_sequence: u64,
    last_sequence: u64,
    first_capture_sequence: Option<u64>,
    last_capture_sequence: Option<u64>,
    first_observed_micros: u64,
    last_observed_micros: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct InfluenceRelationshipKey {
    effect_id: i64,
    provider_actor_id: u64,
    provider_entity_uuid: Option<i64>,
    recipient_actor_id: u64,
    recipient_entity_uuid: Option<i64>,
    affected_damage_id: Option<i64>,
    damage_source_actor_id: Option<u64>,
    damage_source_entity_uuid: Option<i64>,
    target_actor_id: Option<u64>,
    target_entity_uuid: Option<i64>,
    damage_context_complete: bool,
}

#[derive(Debug, Default)]
struct InfluenceRelationshipAccumulator {
    last_sequence: Option<u64>,
    damage_event_count: u64,
    observed_damage: i128,
    exact_integer_delta: i128,
    rational_by_denominator: BTreeMap<i128, (i128, u64)>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CrossRunInfluenceRelationshipKey {
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
    effect_id: i64,
    affected_damage_id: Option<i64>,
    damage_context_complete: bool,
}

#[derive(Debug, Default)]
struct CrossRunInfluenceRelationshipAccumulator {
    sessions: BTreeSet<String>,
    last_damage_event: Option<(usize, u64)>,
    damage_event_count: u64,
    observed_damage: i128,
    exact_integer_delta: i128,
    rational_by_denominator: BTreeMap<i128, (i128, u64)>,
}

#[derive(Debug, Serialize)]
struct CrossRunInfluenceRelationshipSummary {
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
    effect_id: i64,
    affected_damage_id: Option<i64>,
    session_count: u64,
    damage_event_count: u64,
    observed_damage: String,
    exact_integer_delta: String,
    exact_rational_deltas: Vec<ExactRationalDeltaSummary>,
    damage_context_complete: bool,
    proof_status: &'static str,
}

#[derive(Debug, Serialize)]
struct InfluenceRelationshipSummary {
    effect_id: i64,
    provider_actor_id: String,
    provider_entity_uuid: Option<String>,
    recipient_actor_id: String,
    recipient_entity_uuid: Option<String>,
    affected_damage_id: Option<i64>,
    damage_source_actor_id: Option<String>,
    damage_source_entity_uuid: Option<String>,
    target_actor_id: Option<String>,
    target_entity_uuid: Option<String>,
    damage_event_count: u64,
    observed_damage: String,
    exact_integer_delta: String,
    exact_rational_deltas: Vec<ExactRationalDeltaSummary>,
    damage_context_complete: bool,
}

#[derive(Debug, Serialize)]
struct ExactRationalDeltaSummary {
    numerator: String,
    denominator: String,
    contribution_count: u64,
}

#[derive(Debug, Serialize)]
struct EmittedContributionLedgerEntry {
    sequence: u64,
    capture_sequence: Option<u64>,
    observed_micros: u64,
    effect_id: i64,
    provider_actor_id: u64,
    provider_entity_uuid: Option<String>,
    recipient_actor_id: u64,
    recipient_entity_uuid: Option<String>,
    affected_damage_id: Option<i64>,
    damage_source_actor_id: Option<String>,
    damage_source_entity_uuid: Option<String>,
    target_actor_id: Option<String>,
    target_entity_uuid: Option<String>,
    numerator: String,
    denominator: String,
    observed_damage: String,
    damage_context_complete: bool,
    formula_trace: Option<HarmonyGraceFormulaTraceReport>,
}

#[derive(Debug, Serialize)]
struct HarmonyGraceFormulaTraceReport {
    effect_id: i64,
    provider_actor_id: String,
    recipient_actor_id: String,
    recipient_class_id: i32,
    attack_lane: &'static str,
    ability_id: i64,
    hit_event_id: Option<i32>,
    damage_attr_id: i64,
    observed_damage: String,
    primary_final: String,
    primary_intermediate: String,
    primary_base_add: String,
    primary_extra_add: String,
    primary_raw_percent: String,
    primary_family_rounding: &'static str,
    provider_primary_raw_percent: String,
    primary_provider_marginal_basis: &'static str,
    primary_transition_connection_id: u64,
    primary_transition_stream_id: u64,
    primary_transition_capture_sequence: u64,
    primary_transition_instance_id: Option<i64>,
    primary_provider_marginal: String,
    primary_without_provider: String,
    primary_to_attack_numerator: String,
    primary_to_attack_denominator: String,
    attack_component_with_provider: String,
    attack_component_without_provider: String,
    provider_attack_base_add: String,
    attack_final: String,
    attack_intermediate: String,
    attack_base_add: String,
    attack_extra_add: String,
    attack_raw_percent: String,
    provider_attack_marginal: String,
    attack_without_provider: String,
    coefficient_basis_points: String,
    fixed_parameter: String,
    active_coefficient_term: String,
    active_stage_body: String,
    without_provider_coefficient_term: String,
    coefficient_stage_marginal: String,
    contribution_numerator: String,
    contribution_denominator: String,
}

#[derive(Debug, Serialize)]
struct HarmonyGraceFamilyRoundingDiagnosticCount {
    damage_rows: u64,
    first_damage_sequence: u64,
    last_damage_sequence: u64,
    sample_damage_sequences: Vec<u64>,
    diagnostic: HarmonyGraceFamilyRoundingDiagnostic,
}

#[derive(Debug, Default)]
struct HarmonyGraceFamilyRoundingDiagnosticAccumulator {
    damage_rows: u64,
    first_damage_sequence: Option<u64>,
    last_damage_sequence: Option<u64>,
    sample_damage_sequences: Vec<u64>,
}

impl HarmonyGraceFamilyRoundingDiagnosticAccumulator {
    fn observe(&mut self, sequence: u64) {
        self.damage_rows = self.damage_rows.saturating_add(1);
        self.first_damage_sequence.get_or_insert(sequence);
        self.last_damage_sequence = Some(sequence);
        if self.sample_damage_sequences.len() < 8 {
            self.sample_damage_sequences.push(sequence);
        }
    }
}

impl From<HarmonyGraceFormulaTrace> for HarmonyGraceFormulaTraceReport {
    fn from(trace: HarmonyGraceFormulaTrace) -> Self {
        Self {
            effect_id: trace.effect_id,
            provider_actor_id: trace.provider_actor_id.to_string(),
            recipient_actor_id: trace.recipient_actor_id.to_string(),
            recipient_class_id: trace.recipient_class_id,
            attack_lane: trace.attack_lane,
            ability_id: trace.ability_id,
            hit_event_id: trace.hit_event_id,
            damage_attr_id: trace.damage_attr_id,
            observed_damage: trace.observed_damage.to_string(),
            primary_final: trace.primary_final.to_string(),
            primary_intermediate: trace.primary_intermediate.to_string(),
            primary_base_add: trace.primary_base_add.to_string(),
            primary_extra_add: trace.primary_extra_add.to_string(),
            primary_raw_percent: trace.primary_raw_percent.to_string(),
            primary_family_rounding: trace.primary_family_rounding,
            provider_primary_raw_percent: trace.provider_primary_raw_percent.to_string(),
            primary_provider_marginal_basis: trace.primary_provider_marginal_basis,
            primary_transition_connection_id: trace.primary_transition_connection_id,
            primary_transition_stream_id: trace.primary_transition_stream_id,
            primary_transition_capture_sequence: trace.primary_transition_capture_sequence,
            primary_transition_instance_id: trace.primary_transition_instance_id,
            primary_provider_marginal: trace.primary_provider_marginal.to_string(),
            primary_without_provider: trace.primary_without_provider.to_string(),
            primary_to_attack_numerator: trace.primary_to_attack_numerator.to_string(),
            primary_to_attack_denominator: trace.primary_to_attack_denominator.to_string(),
            attack_component_with_provider: trace.attack_component_with_provider.to_string(),
            attack_component_without_provider: trace.attack_component_without_provider.to_string(),
            provider_attack_base_add: trace.provider_attack_base_add.to_string(),
            attack_final: trace.attack_final.to_string(),
            attack_intermediate: trace.attack_intermediate.to_string(),
            attack_base_add: trace.attack_base_add.to_string(),
            attack_extra_add: trace.attack_extra_add.to_string(),
            attack_raw_percent: trace.attack_raw_percent.to_string(),
            provider_attack_marginal: trace.provider_attack_marginal.to_string(),
            attack_without_provider: trace.attack_without_provider.to_string(),
            coefficient_basis_points: trace.coefficient_basis_points.to_string(),
            fixed_parameter: trace.fixed_parameter.to_string(),
            active_coefficient_term: trace.active_coefficient_term.to_string(),
            active_stage_body: trace.active_stage_body.to_string(),
            without_provider_coefficient_term: trace.without_provider_coefficient_term.to_string(),
            coefficient_stage_marginal: trace.coefficient_stage_marginal.to_string(),
            contribution_numerator: trace.contribution_numerator.to_string(),
            contribution_denominator: trace.contribution_denominator.to_string(),
        }
    }
}

impl From<&InspirationCombinedAuthorityRecord> for InspirationCombinedAuthorityRecordReport {
    fn from(record: &InspirationCombinedAuthorityRecord) -> Self {
        Self {
            protocol_pack_digest: record.protocol_pack_digest.clone(),
            source_entity_uuid: record.source_entity_uuid.to_string(),
            target_entity_uuid: record.target_entity_uuid.to_string(),
            ability_id: record.ability_id,
            hit_event_id: record.hit_event_id,
            damage_source: record.damage_source,
            type_flags: record.type_flags,
            reported_critical: record.reported_critical,
            normal_value: record.normal_value,
            lucky_value: record.lucky_value,
            observed_damage: record.observed_damage,
            provider_entity_uuid: record.provider_entity_uuid.to_string(),
            provider_instance_id: record.provider_instance_id,
            provider_level: record.provider_level,
            provider_origin_source_type_id: record.provider_origin_source_type_id,
            provider_origin_source_config_id: record.provider_origin_source_config_id,
            current_critical_damage_raw: record
                .critical_damage
                .expect("combined authority requires Critical DMG")
                .value,
            current_lucky_damage_raw: record
                .lucky_damage
                .expect("combined authority requires Lucky DMG")
                .value,
            provider_critical_chance_raw_delta: record.provider_critical_raw_delta,
            provider_lucky_chance_raw_delta: record.provider_lucky_raw_delta,
        }
    }
}

impl From<InspirationCombinedFormulaTrace> for InspirationCombinedFormulaTraceReport {
    fn from(trace: InspirationCombinedFormulaTrace) -> Self {
        Self {
            effect_id: trace.effect_id,
            provider_actor_id: trace.provider_actor_id,
            provider_entity_uuid: trace.provider_entity_uuid.to_string(),
            provider_instance_id: trace.provider_instance_id,
            provider_effect_level: trace.provider_effect_level,
            provider_origin_source_type_id: trace.provider_origin_source_type_id,
            provider_origin_source_config_id: trace.provider_origin_source_config_id,
            recipient_actor_id: trace.recipient_actor_id,
            recipient_entity_uuid: trace.recipient_entity_uuid.to_string(),
            ability_id: trace.ability_id,
            hit_event_id: trace.hit_event_id,
            damage_source: trace.damage_source,
            packet_type_flags: trace.packet_type_flags,
            packet_reported_critical: trace.packet_reported_critical,
            packet_normal_value: trace.packet_normal_value,
            packet_lucky_value: trace.packet_lucky_value,
            observed_damage: trace.observed_damage,
            current_critical_chance_raw: trace.current_critical_chance_raw,
            current_lucky_chance_raw: trace.current_lucky_chance_raw,
            current_critical_damage_raw: trace.current_critical_damage_raw,
            current_lucky_damage_raw: trace.current_lucky_damage_raw,
            provider_chance_raw_delta: trace.provider_chance_raw_delta,
            provider_critical_damage_raw_delta: trace.provider_critical_damage_raw_delta,
            provider_lucky_damage_raw_delta: trace.provider_lucky_damage_raw_delta,
            contribution_scope: trace
                .contribution_scope
                .component_key()
                .unwrap_or("complete-effect")
                .to_owned(),
            contribution_numerator: trace.contribution_numerator.to_string(),
            contribution_denominator: trace.contribution_denominator.to_string(),
            ordered_prior_contributions: Vec::new(),
            final_contribution_numerator: None,
            final_contribution_denominator: None,
        }
    }
}

fn load_inspiration_combined_authority(
    path: &Path,
) -> Result<(InspirationCombinedReconciliationAccumulator, Vec<PathBuf>), Box<dyn std::error::Error>>
{
    let artifact: InspirationCombinedAuthorityArtifact =
        serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let mut authority_by_event = BTreeMap::new();
    for record in artifact
        .integer_stage_counterfactual_coverage
        .critical_factor_event_records
        .into_iter()
        .filter(|record| {
            record.path == "combined_lucky_occurrence_and_critical_bonus"
                && record.ability_id == 2_031_109
        })
    {
        if record.hit_event_id != Some(3)
            || record.damage_source != Some(2)
            || record.type_flags != Some(1)
            || record.reported_critical.is_some()
            || record.normal_value.is_some()
            || record.lucky_value != Some(record.observed_damage)
            || record.provider_origin_source_type_id != 1
            || record.provider_origin_source_config_id != 2_202_040
            || !matches!(record.provider_level, 2 | 5)
            || record.critical_damage.is_none()
            || record.lucky_damage.is_none()
            || record.provider_critical_raw_delta != record.provider_lucky_raw_delta
        {
            return Err(format!(
                "combined authority row has an unreviewed shape: {}:{}",
                record.session_id, record.damage_sequence
            )
            .into());
        }
        let key = (record.session_id.clone(), record.damage_sequence);
        if authority_by_event.insert(key.clone(), record).is_some() {
            return Err(format!(
                "combined authority contains a duplicate identity: {}:{}",
                key.0, key.1
            )
            .into());
        }
    }
    if authority_by_event.len() != 61 {
        return Err(format!(
            "combined authority must contain exactly 61 reviewed rows, found {}",
            authority_by_event.len()
        )
        .into());
    }
    Ok((
        InspirationCombinedReconciliationAccumulator {
            source_path: path.display().to_string(),
            authority_by_event,
            decisions: Vec::with_capacity(61),
            seen_events: BTreeSet::new(),
            new_route_events: Vec::new(),
        },
        artifact.rlogs.into_iter().map(|rlog| rlog.path).collect(),
    ))
}

fn inspiration_combined_trace_matches_authority(
    trace: &InspirationCombinedFormulaTrace,
    authority: &InspirationCombinedAuthorityRecord,
) -> bool {
    trace.effect_id == 2_202_041
        && trace.provider_entity_uuid == authority.provider_entity_uuid
        && trace.provider_instance_id == authority.provider_instance_id
        && trace.provider_effect_level == authority.provider_level
        && trace.provider_origin_source_type_id == authority.provider_origin_source_type_id
        && trace.provider_origin_source_config_id == authority.provider_origin_source_config_id
        && trace.recipient_entity_uuid == authority.source_entity_uuid
        && trace.ability_id == authority.ability_id
        && trace.hit_event_id == authority.hit_event_id
        && trace.damage_source == authority.damage_source
        && trace.packet_type_flags == authority.type_flags
        && trace.packet_reported_critical == authority.reported_critical
        && trace.packet_normal_value == authority.normal_value
        && trace.packet_lucky_value == authority.lucky_value
        && trace.observed_damage == authority.observed_damage
        && trace.current_critical_damage_raw
            == authority
                .critical_damage
                .expect("validated combined authority Critical DMG")
                .value
        && trace.current_lucky_damage_raw
            == authority
                .lucky_damage
                .expect("validated combined authority Lucky DMG")
                .value
        && trace.provider_chance_raw_delta == authority.provider_critical_raw_delta
        && trace.provider_chance_raw_delta == authority.provider_lucky_raw_delta
}

fn inspiration_combined_trace_is_complete_static_route(
    trace: &InspirationCombinedFormulaTrace,
) -> bool {
    let allowed_chance_deltas: &[i64] = match trace.provider_effect_level {
        2 => &[150, 180],
        5 => &[300, 360],
        _ => return false,
    };
    let expected_chance_delta = trace.provider_chance_raw_delta;
    if !allowed_chance_deltas.contains(&expected_chance_delta) {
        return false;
    }
    let expected_lucky_damage_delta = trace
        .current_lucky_chance_raw
        .checked_sub(expected_chance_delta)
        .filter(|without_provider| *without_provider >= 0)
        .map(|without_provider| {
            trace.current_lucky_chance_raw.div_euclid(4) - without_provider.div_euclid(4)
        });
    trace.effect_id == 2_202_041
        && trace.provider_actor_id != trace.recipient_actor_id
        && trace.provider_entity_uuid != trace.recipient_entity_uuid
        && trace.provider_instance_id.is_some()
        && trace.provider_origin_source_type_id == 1
        && trace.provider_origin_source_config_id == 2_202_040
        && trace.ability_id == 2_031_109
        && trace.hit_event_id == Some(3)
        && trace.damage_source == Some(2)
        && trace.packet_type_flags == Some(1)
        && trace.packet_reported_critical.is_none()
        && trace.packet_normal_value.is_none()
        && trace.packet_lucky_value == Some(trace.observed_damage)
        && trace.observed_damage > 0
        && trace.current_critical_chance_raw >= expected_chance_delta
        && trace.current_lucky_chance_raw >= expected_chance_delta
        && trace.current_critical_damage_raw > 0
        && trace.current_lucky_damage_raw > 0
        && trace.provider_chance_raw_delta == expected_chance_delta
        && matches!(
            trace.provider_critical_damage_raw_delta,
            0 | 75 | 90 | 150 | 180
        )
        && expected_lucky_damage_delta == Some(trace.provider_lucky_damage_raw_delta)
        && trace.contribution_numerator > 0
        && trace.contribution_denominator > 0
        && trace.contribution_numerator
            <= i128::from(trace.observed_damage) * trace.contribution_denominator
}

fn inspiration_combined_emitted_matches_formula_trace(
    candidate: &ExactRationalDamageContributionEvent,
    emitted: &ExactRationalDamageContributionEvent,
    ordered_prior: &[ExactRationalDamageContributionEvent],
) -> bool {
    if emitted.numerator == candidate.numerator && emitted.denominator == candidate.denominator {
        return ordered_prior.is_empty();
    }
    if ordered_prior.is_empty() {
        return false;
    }
    let mut sum_numerator = 0_i128;
    let mut sum_denominator = 1_i128;
    for contribution in ordered_prior {
        if contribution.observed_damage != candidate.observed_damage
            || contribution.recipient_actor_id != candidate.recipient_actor_id
            || contribution.numerator <= 0
            || contribution.denominator <= 0
        {
            return false;
        }
        let shared = gcd_i128(sum_denominator, contribution.denominator);
        let Some(left_scale) = contribution.denominator.checked_div(shared) else {
            return false;
        };
        let Some(right_scale) = sum_denominator.checked_div(shared) else {
            return false;
        };
        let Some(next_numerator) = sum_numerator.checked_mul(left_scale).and_then(|left| {
            contribution
                .numerator
                .checked_mul(right_scale)
                .and_then(|right| left.checked_add(right))
        }) else {
            return false;
        };
        let Some(next_denominator) = sum_denominator.checked_mul(left_scale) else {
            return false;
        };
        let reduce = gcd_i128(next_numerator, next_denominator);
        sum_numerator = next_numerator / reduce;
        sum_denominator = next_denominator / reduce;
    }
    let Some(observed_scaled) = i128::from(candidate.observed_damage).checked_mul(sum_denominator)
    else {
        return false;
    };
    let Some(remaining_scaled) = observed_scaled.checked_sub(sum_numerator) else {
        return false;
    };
    let Some(numerator) = candidate.numerator.checked_mul(remaining_scaled) else {
        return false;
    };
    let Some(denominator) = candidate.denominator.checked_mul(observed_scaled) else {
        return false;
    };
    if remaining_scaled <= 0 || denominator <= 0 {
        return false;
    }
    let divisor = gcd_i128(numerator, denominator);
    emitted.numerator == numerator / divisor && emitted.denominator == denominator / divisor
}

fn inspiration_combined_formula_trace_report(
    trace: InspirationCombinedFormulaTrace,
    emitted: Option<(usize, ExactRationalDamageContributionEvent)>,
    rational_contributions: &[ExactRationalDamageContributionEvent],
) -> InspirationCombinedFormulaTraceReport {
    let mut report = InspirationCombinedFormulaTraceReport::from(trace);
    let Some((emitted_index, emitted)) = emitted else {
        return report;
    };
    report.ordered_prior_contributions = rational_contributions[..emitted_index]
        .iter()
        .map(|contribution| InspirationCombinedPriorContributionReport {
            effect_id: contribution.effect_id,
            provider_actor_id: contribution.provider_actor_id,
            contribution_scope: contribution
                .scope
                .component_key()
                .unwrap_or("complete-effect")
                .to_owned(),
            numerator: contribution.numerator.to_string(),
            denominator: contribution.denominator.to_string(),
        })
        .collect();
    report.final_contribution_numerator = Some(emitted.numerator.to_string());
    report.final_contribution_denominator = Some(emitted.denominator.to_string());
    report
}

fn inspiration_combined_damage_matches_authority(
    envelope: &rlogs_events::EventEnvelope,
    damage: &DamageEvent,
    authority: &InspirationCombinedAuthorityRecord,
) -> bool {
    envelope.time.observed_micros == authority.damage_observed_micros
        && damage.source.entity_uuid.0 == authority.source_entity_uuid
        && damage.target.entity_uuid.0 == authority.target_entity_uuid
        && damage.ability.map(|ability| ability.0) == Some(authority.ability_id)
        && damage.hit_event_id == authority.hit_event_id
        && damage.damage_source == authority.damage_source
        && damage.packet.type_flags == authority.type_flags
        && damage.packet.reported_critical == authority.reported_critical
        && damage.packet.normal_value == authority.normal_value
        && damage.packet.lucky_value == authority.lucky_value
        && damage.amount == authority.observed_damage
        && damage.flags.critical == Some(true)
        && damage.flags.lucky == Some(true)
}

fn record_inspiration_combined_decision(
    accumulator: &mut InspirationCombinedReconciliationAccumulator,
    session_id: &str,
    envelope: &rlogs_events::EventEnvelope,
    damage: &DamageEvent,
    projector: &BpsrStateDamageContributionProjector,
    exact_contributions: &[ExactDamageContributionEvent],
    rational_contributions: &[ExactRationalDamageContributionEvent],
) {
    let key = (session_id.to_owned(), envelope.sequence);
    let authority = accumulator.authority_by_event.get(&key);
    let occurrence = projector.inspiration_combined_occurrence_audit_decision(damage);
    let occurrence_output = |candidate: &ExactRationalDamageContributionEvent| {
        let (base_index, base) =
            rational_contributions
                .iter()
                .enumerate()
                .find(|(_, emitted)| {
                    emitted.effect_id == candidate.effect_id
                        && emitted.provider_actor_id == candidate.provider_actor_id
                        && emitted.recipient_actor_id == candidate.recipient_actor_id
                        && emitted.scope == candidate.scope
                })?;
        if base.numerator == candidate.numerator && base.denominator == candidate.denominator {
            return Some((base_index, *base));
        }
        let full_bloom = rational_contributions.iter().find(|emitted| {
            emitted.effect_id == 2_404_271
                && emitted.recipient_actor_id == candidate.recipient_actor_id
                && emitted.observed_damage == candidate.observed_damage
                && emitted
                    .scope
                    .component_key()
                    .is_some_and(|scope| scope.starts_with("full-bloom-inspiration-"))
        })?;
        let shared = gcd_i128(base.denominator, full_bloom.denominator);
        let left_scale = full_bloom.denominator.checked_div(shared)?;
        let right_scale = base.denominator.checked_div(shared)?;
        let numerator = base
            .numerator
            .checked_mul(left_scale)?
            .checked_add(full_bloom.numerator.checked_mul(right_scale)?)?;
        let denominator = base.denominator.checked_mul(left_scale)?;
        let divisor = gcd_i128(numerator, denominator);
        let mut aggregate = *base;
        aggregate.numerator = numerator.checked_div(divisor)?;
        aggregate.denominator = denominator.checked_div(divisor)?;
        Some((base_index, aggregate))
    };
    if let Some(authority) = authority {
        let inserted = accumulator.seen_events.insert(key.clone());
        let damage_identity_matches =
            inspiration_combined_damage_matches_authority(envelope, damage, authority);
        let exact_effect_ids = exact_contributions
            .iter()
            .map(|contribution| contribution.effect_id)
            .collect::<Vec<_>>();
        let rational_effect_ids = rational_contributions
            .iter()
            .map(|contribution| contribution.effect_id)
            .collect::<Vec<_>>();
        let later_candidate_signature =
            projector.critical_cold_simultaneous_later_candidate_signature(envelope, damage);
        let pipeline_audit = projector.inspiration_combined_pipeline_audit();
        let (
            decision,
            decision_gate,
            authority_identity_matches,
            formula_trace_complete,
            emitted_contribution_matches_formula_trace,
            formula_trace,
        ) = match occurrence {
            Ok((candidate, trace)) => {
                let emitted = occurrence_output(&candidate);
                let trace_matches = damage_identity_matches
                    && inspiration_combined_trace_matches_authority(&trace, authority);
                let emitted_matches_trace = emitted.is_some_and(|(index, emitted)| {
                    inspiration_combined_emitted_matches_formula_trace(
                        &candidate,
                        &emitted,
                        &rational_contributions[..index],
                    )
                });
                let decision = if emitted.is_some() {
                    "emitted"
                } else {
                    "suppressed"
                };
                let gate = if emitted.is_some() {
                    "emitted_exact_formula"
                } else {
                    pipeline_audit
                        .map(InspirationCombinedPipelineAudit::suppression_gate)
                        .unwrap_or("suppressed_downstream_pipeline_unclassified")
                };
                (
                    decision.to_owned(),
                    gate.to_owned(),
                    trace_matches,
                    trace_matches,
                    emitted_matches_trace,
                    Some(inspiration_combined_formula_trace_report(
                        trace,
                        emitted,
                        rational_contributions,
                    )),
                )
            }
            Err(gate) => (
                "suppressed".to_owned(),
                gate.to_owned(),
                damage_identity_matches,
                false,
                false,
                None,
            ),
        };
        accumulator
            .decisions
            .push(InspirationCombinedDecisionTrace {
                session_id: key.0,
                damage_sequence: key.1,
                observed_micros: envelope.time.observed_micros,
                decision,
                decision_gate: if inserted {
                    decision_gate
                } else {
                    format!("duplicate_decision:{decision_gate}")
                },
                authority_identity_matches,
                formula_trace_complete,
                emitted_contribution_matches_formula_trace,
                later_candidate_signature,
                pipeline_audit: pipeline_audit.map(InspirationCombinedPipelineAuditReport::from),
                emitted_exact_effect_ids: exact_effect_ids,
                emitted_rational_effect_ids: rational_effect_ids,
                authority: InspirationCombinedAuthorityRecordReport::from(authority),
                formula_trace,
            });
    } else if let Ok((candidate, trace)) = occurrence {
        let emitted = occurrence_output(&candidate);
        let pipeline_audit = projector.inspiration_combined_pipeline_audit();
        let emitted_matches_trace = emitted.is_some_and(|(index, emitted)| {
            inspiration_combined_emitted_matches_formula_trace(
                &candidate,
                &emitted,
                &rational_contributions[..index],
            )
        });
        let decision = if emitted.is_some() {
            "emitted"
        } else {
            "suppressed"
        };
        let decision_gate = if emitted.is_some() {
            "emitted_exact_formula"
        } else {
            pipeline_audit
                .map(InspirationCombinedPipelineAudit::suppression_gate)
                .unwrap_or("suppressed_downstream_pipeline_unclassified")
        };
        let formula_trace_complete = inspiration_combined_trace_is_complete_static_route(&trace);
        accumulator
            .new_route_events
            .push(InspirationCombinedNewRouteEvent {
                session_id: session_id.to_owned(),
                damage_sequence: envelope.sequence,
                observed_micros: envelope.time.observed_micros,
                decision: decision.to_owned(),
                decision_gate: decision_gate.to_owned(),
                formula_trace_complete,
                emitted_contribution_matches_formula_trace: emitted_matches_trace,
                pipeline_audit: pipeline_audit.map(InspirationCombinedPipelineAuditReport::from),
                formula_trace: inspiration_combined_formula_trace_report(
                    trace,
                    emitted,
                    rational_contributions,
                ),
            });
    }
}

fn finish_inspiration_combined_reconciliation(
    mut accumulator: InspirationCombinedReconciliationAccumulator,
    reports: &[ReplayAuditReport],
) -> InspirationCombinedReconciliationReceipt {
    accumulator.decisions.sort_by(|left, right| {
        (&left.session_id, left.damage_sequence).cmp(&(&right.session_id, right.damage_sequence))
    });
    accumulator.new_route_events.sort_by(|left, right| {
        (&left.session_id, left.damage_sequence).cmp(&(&right.session_id, right.damage_sequence))
    });
    let authorized_event_count = accumulator.authority_by_event.len() as u64;
    let new_route_decision_count = accumulator.new_route_events.len() as u64;
    let newly_authorized_event_count = accumulator
        .new_route_events
        .iter()
        .filter(|event| event.decision == "emitted")
        .count() as u64;
    let extended_authorized_event_count =
        authorized_event_count.saturating_add(newly_authorized_event_count);
    let general_route_decision_count =
        authorized_event_count.saturating_add(new_route_decision_count);
    let decision_event_count = accumulator.decisions.len() as u64;
    let emitted_event_count = (accumulator
        .decisions
        .iter()
        .filter(|decision| decision.decision == "emitted")
        .count()
        + accumulator
            .new_route_events
            .iter()
            .filter(|decision| decision.decision == "emitted")
            .count()) as u64;
    let suppressed_event_count = (accumulator
        .decisions
        .iter()
        .filter(|decision| decision.decision == "suppressed")
        .count()
        + accumulator
            .new_route_events
            .iter()
            .filter(|decision| decision.decision == "suppressed")
            .count()) as u64;
    let missing_decision_count = accumulator
        .authority_by_event
        .keys()
        .filter(|key| !accumulator.seen_events.contains(*key))
        .count() as u64;
    let duplicate_decision_count =
        decision_event_count.saturating_sub(accumulator.seen_events.len() as u64);
    let identity_set_equal = authorized_event_count == 61
        && decision_event_count == authorized_event_count
        && missing_decision_count == 0
        && duplicate_decision_count == 0
        && accumulator
            .decisions
            .iter()
            .all(|decision| decision.authority_identity_matches);
    let formula_trace_complete_count = accumulator
        .decisions
        .iter()
        .filter(|decision| decision.formula_trace_complete)
        .count() as u64;
    let all_route_formula_trace_complete_count = formula_trace_complete_count
        + accumulator
            .new_route_events
            .iter()
            .filter(|event| event.formula_trace_complete)
            .count() as u64;
    let formula_trace_mismatch_count = (accumulator
        .decisions
        .iter()
        .filter(|decision| {
            decision.decision == "emitted"
                && (!decision.formula_trace_complete
                    || !decision.emitted_contribution_matches_formula_trace)
        })
        .count()
        + accumulator
            .new_route_events
            .iter()
            .filter(|event| {
                !event.formula_trace_complete
                    || (event.decision == "emitted"
                        && !event.emitted_contribution_matches_formula_trace)
            })
            .count()) as u64;
    let new_route_identity_count = accumulator
        .new_route_events
        .iter()
        .map(|event| (&event.session_id, event.damage_sequence))
        .collect::<BTreeSet<_>>()
        .len() as u64;
    let runtime_target_match_rlogs = reports
        .iter()
        .filter(|report| report.runtime_target_match)
        .count() as u64;
    let conserved_rlogs = reports.iter().filter(|report| report.conserved).count() as u64;
    let rational_projection_overflow_count = reports
        .iter()
        .map(|report| report.summary.rational_projection_overflow_count)
        .sum();
    let runtime_authority = identity_set_equal
        && newly_authorized_event_count == 51
        && new_route_decision_count == 265
        && new_route_identity_count == new_route_decision_count
        && extended_authorized_event_count == 112
        && general_route_decision_count == 326
        && emitted_event_count == 53
        && suppressed_event_count == 273
        && all_route_formula_trace_complete_count == 326
        && formula_trace_mismatch_count == 0
        && reports.len() == 26
        && runtime_target_match_rlogs == 26
        && conserved_rlogs == 26
        && rational_projection_overflow_count == 0;
    let mut runtime_authority_blockers = Vec::new();
    if !identity_set_equal {
        runtime_authority_blockers.push(
            "the 61 authorized event identities do not each have one exact replay decision"
                .to_owned(),
        );
    }
    if newly_authorized_event_count != 51
        || new_route_decision_count != 265
        || new_route_identity_count != new_route_decision_count
        || extended_authorized_event_count != 112
        || general_route_decision_count != 326
    {
        runtime_authority_blockers.push(format!(
            "the generalized static route produced {newly_authorized_event_count} newly authorized emissions, {new_route_decision_count} unique new decisions, {extended_authorized_event_count} authorized cohort rows, and {general_route_decision_count} total decisions; expected 51, 265, 112, and 326"
        ));
    }
    if emitted_event_count != 53 || suppressed_event_count != 273 {
        runtime_authority_blockers.push(format!(
            "the generalized static route produced {emitted_event_count} emitted and {suppressed_event_count} suppressed decisions, expected 53 and 273"
        ));
    }
    if all_route_formula_trace_complete_count != 326 {
        runtime_authority_blockers.push(format!(
            "only {all_route_formula_trace_complete_count}/326 generalized route decisions have complete packet/static formula traces"
        ));
    }
    if formula_trace_mismatch_count != 0 {
        runtime_authority_blockers.push(format!(
            "{formula_trace_mismatch_count} emitted decisions lack an exact authority-matching final rational formula trace"
        ));
    }
    if reports.len() != 26 || runtime_target_match_rlogs != 26 {
        runtime_authority_blockers.push(format!(
            "current-build runtime identity matched {runtime_target_match_rlogs}/{} rlogs, expected 26/26",
            reports.len()
        ));
    }
    if reports.len() != 26 || conserved_rlogs != 26 {
        runtime_authority_blockers.push(format!(
            "rDPS conservation held for {conserved_rlogs}/{} rlogs, expected 26/26",
            reports.len()
        ));
    }
    if rational_projection_overflow_count != 0 {
        runtime_authority_blockers.push(format!(
            "rational projection overflow count is {rational_projection_overflow_count}, expected zero"
        ));
    }
    InspirationCombinedReconciliationReceipt {
        schema_version: 1,
        effect_id: 2_202_041,
        full_locale_name: "Inspiration",
        affected_damage_id: 2_031_109,
        source_authority_path: accumulator.source_path,
        identity_namespace: "session_id_plus_canonical_damage_sequence",
        authorized_event_count,
        newly_authorized_event_count,
        extended_authorized_event_count,
        new_route_decision_count,
        general_route_decision_count,
        decision_event_count,
        emitted_event_count,
        suppressed_event_count,
        missing_decision_count,
        duplicate_decision_count,
        identity_set_equal,
        formula_trace_complete_count,
        formula_trace_mismatch_count,
        all_route_formula_trace_complete_count,
        runtime_target_match_rlogs,
        conserved_rlogs,
        rational_projection_overflow_count,
        runtime_authority,
        runtime_authority_blockers,
        decisions: accumulator.decisions,
        newly_authorized_events: accumulator.new_route_events,
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rDPS replay audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = arguments()?;
    let (mut inspiration_combined_reconciliation, authority_rlogs) = arguments
        .inspiration_combined_authority
        .as_deref()
        .map(load_inspiration_combined_authority)
        .transpose()?
        .map(|(accumulator, rlogs)| (Some(accumulator), rlogs))
        .unwrap_or((None, Vec::new()));
    if !authority_rlogs.is_empty() {
        let authority_rlogs = authority_rlogs.into_iter().collect::<BTreeSet<_>>();
        if arguments.rlogs.is_empty() {
            arguments.rlogs = authority_rlogs.into_iter().collect();
        } else if arguments.rlogs.iter().cloned().collect::<BTreeSet<_>>() != authority_rlogs {
            return Err(
                "the focused combined receipt must replay exactly the authority artifact's rlogs"
                    .into(),
            );
        }
    }
    let rules = confirmed_damage_contribution_rules()?;
    let mut rule_effect_ids = rules.iter().map(|rule| rule.effect_id).collect::<Vec<_>>();
    rule_effect_ids.extend(proven_state_damage_contribution_effect_ids()?);
    rule_effect_ids.sort_unstable();
    rule_effect_ids.dedup();
    let target_vulnerability_candidate_effect_ids =
        if arguments.target_vulnerability_candidate_audit_enabled {
            target_vulnerability_candidate_effect_ids()?
        } else {
            Vec::new()
        };
    let started = Instant::now();
    let mut total_events = 0_u64;
    let mut reports = Vec::with_capacity(arguments.rlogs.len());
    let mut relationship_catalog = BTreeMap::<
        CrossRunInfluenceRelationshipKey,
        CrossRunInfluenceRelationshipAccumulator,
    >::new();
    for (report_index, path) in arguments.rlogs.into_iter().enumerate() {
        let mut reader =
            RlogReader::new(BufReader::new(File::open(&path)?), RlogLimits::default())?;
        let session_id = reader.header().session_id.clone();
        let deployment_id = reader.header().region.identity.deployment_id.clone();
        let client_build = reader.header().region.client_build.clone();
        let protocol_pack_digest = reader.header().region.protocol_pack_digest.clone();
        let runtime_target_match = state_damage_contribution_target_matches(
            &deployment_id,
            &client_build,
            &protocol_pack_digest,
        )?;
        let candidate_audit_target_match = (arguments.inspiration_candidate_audit_enabled
            || arguments.harmony_grace_candidate_audit_enabled
            || arguments.mechanical_power_candidate_audit_enabled
            || arguments.mechanical_power_tier0_candidate_audit_enabled
            || arguments.target_vulnerability_candidate_audit_enabled)
            && state_damage_contribution_formula_target_matches(
                &deployment_id,
                &client_build,
                &protocol_pack_digest,
            )?;
        let mut reducer = DamageContributionReducer::new(contribution_rules_for_target(
            &rules,
            &deployment_id,
            &client_build,
        ))?;
        let candidate_audit_enabled = arguments.inspiration_candidate_audit_enabled
            || arguments.harmony_grace_candidate_audit_enabled
            || arguments.mechanical_power_candidate_audit_enabled
            || arguments.mechanical_power_tier0_candidate_audit_enabled
            || arguments.target_vulnerability_candidate_audit_enabled;
        let remote_factors = if runtime_target_match && !candidate_audit_enabled {
            let mut inference_reader =
                RlogReader::new(BufReader::new(File::open(&path)?), RlogLimits::default())?;
            let mut learner = BpsrRemoteFactorLearner::new()?;
            while let Some(envelope) = inference_reader.next_event()? {
                learner.observe(&envelope);
            }
            if inference_reader.summary().is_none() {
                return Err(format!(
                    "sealed rDPS inference replay has no validated integrity summary: {}",
                    path.display()
                )
                .into());
            }
            Some(learner.finish())
        } else {
            None
        };
        let mut state_projector = if inspiration_combined_reconciliation.is_some() {
            BpsrStateDamageContributionProjector::new_inspiration_combined_receipt_audit(
                remote_factors.ok_or("combined receipt requires the sealed remote-factor pass")?,
            )?
        } else if arguments.inspiration_candidate_audit_enabled {
            BpsrStateDamageContributionProjector::new_inspiration_candidate_audit()?
        } else if arguments.harmony_grace_candidate_audit_enabled {
            BpsrStateDamageContributionProjector::new_harmony_grace_candidate_audit()?
        } else if arguments.mechanical_power_candidate_audit_enabled {
            BpsrStateDamageContributionProjector::new_mechanical_power_candidate_audit()?
        } else if arguments.mechanical_power_tier0_candidate_audit_enabled {
            BpsrStateDamageContributionProjector::new_mechanical_power_tier0_candidate_audit()?
        } else if arguments.target_vulnerability_candidate_audit_enabled {
            BpsrStateDamageContributionProjector::new_target_vulnerability_candidate_audit()?
        } else if let Some(remote_factors) = remote_factors {
            BpsrStateDamageContributionProjector::new_with_remote_factor_timeline(remote_factors)?
        } else {
            BpsrStateDamageContributionProjector::new()?
        };
        let mut exact_contributions = Vec::<ExactDamageContributionEvent>::new();
        let mut rational_contributions = Vec::<ExactRationalDamageContributionEvent>::new();
        let mut emitted_contribution_events_by_effect = BTreeMap::<i64, u64>::new();
        let mut harmony_grace_audit_gates = BTreeMap::<String, u64>::new();
        let mut harmony_grace_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut harmony_grace_family_rounding_diagnostics = BTreeMap::<
            HarmonyGraceFamilyRoundingDiagnostic,
            HarmonyGraceFamilyRoundingDiagnosticAccumulator,
        >::new();
        let mut team_luck_audit_gates = BTreeMap::<String, u64>::new();
        let mut team_luck_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut team_luck_selector_rows_by_source_actor =
            BTreeMap::<u64, BTreeMap<String, u64>>::new();
        let mut team_luck_suppressed_examples_by_source_actor = BTreeMap::<u64, Vec<String>>::new();
        let mut team_luck_candidate_reducer = DamageContributionReducer::default();
        let mut functional_amp_audit_gates = BTreeMap::<String, u64>::new();
        let mut functional_amp_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut stat_resonance_audit_gates = BTreeMap::<String, u64>::new();
        let mut stat_resonance_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut fiery_battle_will_audit_gates = BTreeMap::<String, u64>::new();
        let mut fiery_battle_will_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut mechanical_power_audit_gates = BTreeMap::<String, u64>::new();
        let mut mechanical_power_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut mechanical_power_audit_actions = BTreeMap::<
            String,
            BTreeMap<MechanicalPowerAuditActionKey, MechanicalPowerAuditActionAccumulator>,
        >::new();
        let mut inspire_haste_audit_gates = BTreeMap::<String, u64>::new();
        let mut inspire_haste_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut inspiration_audit_gates = BTreeMap::<String, u64>::new();
        let mut inspiration_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut inspiration_occurrence_audit_gates = BTreeMap::<String, u64>::new();
        let mut inspiration_occurrence_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut critical_cold_occurrence_audit_gates = BTreeMap::<String, u64>::new();
        let mut critical_cold_occurrence_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut critical_cold_direct_candidate_count = 0_u64;
        let mut critical_cold_direct_reducer = DamageContributionReducer::default();
        let mut critical_cold_simultaneous_candidate_histogram = BTreeMap::<String, u64>::new();
        let mut critical_cold_joint_audit_gates = BTreeMap::<String, u64>::new();
        let mut critical_cold_joint_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut critical_cold_suppressed_context_histogram = BTreeMap::<String, u64>::new();
        let mut critical_cold_suppressed_standalone_examples = Vec::<String>::new();
        let mut thunderwind_audit_gates = BTreeMap::<String, u64>::new();
        let mut thunderwind_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut fatal_spiral_audit_gates = BTreeMap::<String, u64>::new();
        let mut fatal_spiral_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut target_vulnerability_audit_gates = BTreeMap::<String, u64>::new();
        let mut target_vulnerability_audit_examples = BTreeMap::<String, Vec<String>>::new();
        let mut emitted_contribution_ledger = Vec::<EmittedContributionLedgerEntry>::new();
        let mut influence_relationships =
            BTreeMap::<InfluenceRelationshipKey, InfluenceRelationshipAccumulator>::new();
        let mut actor_entities = BTreeMap::<u64, i64>::new();
        let mut incomplete_rdps_actor_ids = BTreeSet::<u64>::new();
        let mut event_count = 0_u64;
        while let Some(envelope) = reader.next_event()? {
            event_count = event_count.saturating_add(1);
            observe_actor_identity(&mut actor_entities, &envelope.event);
            let damage = damage_context(&envelope.event);
            exact_contributions.clear();
            rational_contributions.clear();
            state_projector.observe(
                &envelope,
                &mut exact_contributions,
                &mut rational_contributions,
            );
            incomplete_rdps_actor_ids.extend(state_projector.incomplete_rdps_actor_ids());
            if let (Some(reconciliation), Some(damage)) =
                (inspiration_combined_reconciliation.as_mut(), damage)
            {
                record_inspiration_combined_decision(
                    reconciliation,
                    &session_id,
                    &envelope,
                    damage,
                    &state_projector,
                    &exact_contributions,
                    &rational_contributions,
                );
            }
            if let Some(damage) = damage {
                if let Some(candidate) = state_projector.team_luck_audit_contribution(damage) {
                    let source_actor_id = damage.source.actor_id.0;
                    team_luck_candidate_reducer.observe_exact_rational_contribution(candidate);
                    let emitted = rational_contributions.iter().any(|contribution| {
                        contribution.effect_id == 2_302_121
                            && contribution.recipient_actor_id == source_actor_id
                    });
                    let selector_rows = team_luck_selector_rows_by_source_actor
                        .entry(source_actor_id)
                        .or_default();
                    *selector_rows.entry("candidate".into()).or_default() += 1;
                    *selector_rows
                        .entry(if emitted { "emitted" } else { "suppressed" }.into())
                        .or_default() += 1;
                    if !emitted {
                        let selector = state_projector.target_vulnerability_audit();
                        let signature = state_projector
                            .team_luck_simultaneous_later_candidate_signature(&envelope, damage)
                            .unwrap_or_else(|| "signature-unavailable".into());
                        *selector_rows
                            .entry(format!(
                                "suppressed:pipeline={:?}; exact={:?}; rational={:?}; unresolved_attack_overlap={:?}; {signature}",
                                state_projector.critical_cold_pipeline_audit_detail(),
                                selector.map(|audit| audit.exact_candidate_count),
                                selector.map(|audit| audit.rational_candidate_count),
                                selector.map(|audit| audit.unresolved_attack_overlap),
                            ))
                            .or_default() += 1;
                        let examples = team_luck_suppressed_examples_by_source_actor
                            .entry(source_actor_id)
                            .or_default();
                        if examples.len() < 5 {
                            examples.push(format!(
                                "sequence={} ability={:?} hit={:?} critical={:?} lucky={:?} amount={} exact_output=[{}] rational_output=[{}] signature={signature}",
                                envelope.sequence,
                                damage.ability.map(|ability| ability.0),
                                damage.hit_event_id,
                                damage.flags.critical,
                                damage.flags.lucky,
                                damage.amount,
                                exact_contributions
                                    .iter()
                                    .map(|contribution| format!(
                                        "{}:{}:{}",
                                        contribution.effect_id,
                                        contribution.provider_actor_id,
                                        contribution.amount,
                                    ))
                                    .collect::<Vec<_>>()
                                    .join(" | "),
                                rational_contributions
                                    .iter()
                                    .map(|contribution| format!(
                                        "{}:{}:{}/{}",
                                        contribution.effect_id,
                                        contribution.provider_actor_id,
                                        contribution.numerator,
                                        contribution.denominator,
                                    ))
                                    .collect::<Vec<_>>()
                                    .join(" | "),
                            ));
                        }
                    }
                }
                if let Some(candidate) =
                    state_projector.critical_cold_occurrence_audit_contribution(damage)
                {
                    critical_cold_direct_candidate_count =
                        critical_cold_direct_candidate_count.saturating_add(1);
                    critical_cold_direct_reducer.observe_exact_rational_contribution(candidate);
                    let emitted = rational_contributions
                        .iter()
                        .any(|contribution| contribution.effect_id == 2_204_471);
                    let signature = state_projector
                        .critical_cold_simultaneous_later_candidate_signature(&envelope, damage)
                        .unwrap_or_else(|| "signature-unavailable".into());
                    *critical_cold_simultaneous_candidate_histogram
                        .entry(format!(
                            "{}; {signature}",
                            if emitted { "emitted" } else { "suppressed" }
                        ))
                        .or_default() += 1;
                    if !emitted && signature.contains(" | 2302121:") {
                        record_audit_gate(
                            &mut critical_cold_joint_audit_gates,
                            &mut critical_cold_joint_audit_examples,
                            state_projector.critical_cold_team_luck_joint_audit_gate(damage),
                            damage,
                            envelope.sequence,
                            capture_sequence(&envelope.provenance.source),
                            envelope.time.observed_micros,
                        );
                    }
                    if !emitted {
                        let exact_effect_ids = exact_contributions
                            .iter()
                            .map(|contribution| contribution.effect_id)
                            .collect::<Vec<_>>();
                        let rational_effect_ids = rational_contributions
                            .iter()
                            .map(|contribution| contribution.effect_id)
                            .collect::<Vec<_>>();
                        *critical_cold_suppressed_context_histogram
                            .entry(format!(
                                "ability={:?}; exact={exact_effect_ids:?}; rational={rational_effect_ids:?}; pipeline={:?}; {signature}",
                                damage.ability.map(|ability| ability.0),
                                state_projector.critical_cold_pipeline_audit_detail(),
                            ))
                            .or_default() += 1;
                    }
                    if !emitted
                        && !signature.contains(" | ")
                        && critical_cold_suppressed_standalone_examples.len() < 8
                    {
                        let selector = state_projector.target_vulnerability_audit();
                        critical_cold_suppressed_standalone_examples.push(format!(
                            "run={} sequence={} capture_sequence={:?} observed_micros={} recipient_actor={} recipient_entity={} ability={:?} hit={:?} exact_candidates={:?} rational_candidates={:?} unresolved_attack_overlap={:?} emitted_exact={:?} emitted_rational={:?}",
                            path.display(),
                            envelope.sequence,
                            capture_sequence(&envelope.provenance.source),
                            envelope.time.observed_micros,
                            damage.source.actor_id.0,
                            damage.source.entity_uuid.0,
                            damage.ability.map(|ability| ability.0),
                            damage.hit_event_id,
                            selector.map(|audit| audit.exact_candidate_count),
                            selector.map(|audit| audit.rational_candidate_count),
                            selector.map(|audit| audit.unresolved_attack_overlap),
                            exact_contributions
                                .iter()
                                .map(|contribution| (contribution.effect_id, contribution.provider_actor_id))
                                .collect::<Vec<_>>(),
                            rational_contributions
                                .iter()
                                .map(|contribution| (contribution.effect_id, contribution.provider_actor_id, contribution.scope))
                                .collect::<Vec<_>>(),
                        ));
                    }
                }
                let gate = state_projector.harmony_grace_audit_gate(damage);
                *harmony_grace_audit_gates.entry(gate.into()).or_default() += 1;
                let examples = harmony_grace_audit_examples.entry(gate.into()).or_default();
                if examples.len() < 3 {
                    examples.push(state_projector.harmony_grace_audit_detail(damage));
                }
                if let Some(diagnostic) =
                    state_projector.harmony_grace_family_rounding_diagnostic(damage)
                {
                    harmony_grace_family_rounding_diagnostics
                        .entry(diagnostic)
                        .or_default()
                        .observe(envelope.sequence);
                }
                record_audit_gate(
                    &mut team_luck_audit_gates,
                    &mut team_luck_audit_examples,
                    state_projector.team_luck_audit_gate(damage),
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                record_audit_gate(
                    &mut functional_amp_audit_gates,
                    &mut functional_amp_audit_examples,
                    state_projector.functional_amp_audit_gate(damage),
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                let stat_resonance_gate = state_projector.stat_resonance_audit_gate(damage);
                let stat_resonance_example_count = stat_resonance_audit_examples
                    .get(stat_resonance_gate)
                    .map_or(0, Vec::len);
                record_audit_gate(
                    &mut stat_resonance_audit_gates,
                    &mut stat_resonance_audit_examples,
                    stat_resonance_gate,
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                if let Some(examples) = stat_resonance_audit_examples.get_mut(stat_resonance_gate)
                    && examples.len() > stat_resonance_example_count
                    && let Some(example) = examples.last_mut()
                {
                    example.push(' ');
                    example.push_str(&state_projector.stat_resonance_audit_detail(damage));
                }
                record_audit_gate(
                    &mut fiery_battle_will_audit_gates,
                    &mut fiery_battle_will_audit_examples,
                    state_projector.fiery_battle_will_audit_gate(damage),
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                record_mechanical_power_audit_gate(
                    &mut mechanical_power_audit_gates,
                    &mut mechanical_power_audit_examples,
                    &mut mechanical_power_audit_actions,
                    state_projector.mechanical_power_audit_gate(damage),
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                let inspire_haste_gate = state_projector.inspire_haste_audit_gate(damage);
                record_audit_gate(
                    &mut inspire_haste_audit_gates,
                    &mut inspire_haste_audit_examples,
                    inspire_haste_gate,
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                record_audit_gate(
                    &mut inspiration_audit_gates,
                    &mut inspiration_audit_examples,
                    state_projector.inspiration_audit_gate(damage),
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                record_audit_gate(
                    &mut inspiration_occurrence_audit_gates,
                    &mut inspiration_occurrence_audit_examples,
                    state_projector.inspiration_occurrence_audit_gate(damage),
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                record_audit_gate(
                    &mut critical_cold_occurrence_audit_gates,
                    &mut critical_cold_occurrence_audit_examples,
                    state_projector.critical_cold_occurrence_audit_gate(damage),
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                record_audit_gate(
                    &mut thunderwind_audit_gates,
                    &mut thunderwind_audit_examples,
                    state_projector.thunderwind_audit_gate(damage),
                    damage,
                    envelope.sequence,
                    capture_sequence(&envelope.provenance.source),
                    envelope.time.observed_micros,
                );
                let fatal_spiral_gate = state_projector.fatal_spiral_audit_gate(damage);
                *fatal_spiral_audit_gates
                    .entry(fatal_spiral_gate.into())
                    .or_default() += 1;
                let examples = fatal_spiral_audit_examples
                    .entry(fatal_spiral_gate.into())
                    .or_default();
                if examples.len() < 3 {
                    examples.push(state_projector.fatal_spiral_audit_detail(damage));
                }
                if let Some(audit) = state_projector.target_vulnerability_audit() {
                    *target_vulnerability_audit_gates
                        .entry(audit.gate.into())
                        .or_default() += 1;
                    let examples = target_vulnerability_audit_examples
                        .entry(audit.gate.into())
                        .or_default();
                    if examples.len() < 3 {
                        examples.push(format!(
                            "ability={:?} hit={:?} critical={:?} lucky={:?} source={} target={} exact_candidates={} rational_candidates={} unresolved_attack_overlap={}; {}",
                            damage.ability.map(|id| id.0),
                            damage.hit_event_id,
                            damage.flags.critical,
                            damage.flags.lucky,
                            damage.source.actor_id.0,
                            damage.target.actor_id.0,
                            audit.exact_candidate_count,
                            audit.rational_candidate_count,
                            audit.unresolved_attack_overlap,
                            state_projector.target_vulnerability_audit_detail(damage),
                        ));
                    }
                }
            }
            for contribution in exact_contributions.iter().copied() {
                *emitted_contribution_events_by_effect
                    .entry(contribution.effect_id)
                    .or_default() += 1;
                let ledger_entry = EmittedContributionLedgerEntry {
                    sequence: envelope.sequence,
                    capture_sequence: capture_sequence(&envelope.provenance.source),
                    observed_micros: contribution.observed_micros,
                    effect_id: contribution.effect_id,
                    provider_actor_id: contribution.provider_actor_id,
                    provider_entity_uuid: actor_entities
                        .get(&contribution.provider_actor_id)
                        .map(ToString::to_string),
                    recipient_actor_id: contribution.recipient_actor_id,
                    recipient_entity_uuid: actor_entities
                        .get(&contribution.recipient_actor_id)
                        .map(ToString::to_string),
                    affected_damage_id: damage.and_then(|event| event.ability.map(|id| id.0)),
                    damage_source_actor_id: damage.map(|event| event.source.actor_id.0.to_string()),
                    damage_source_entity_uuid: damage
                        .map(|event| event.source.entity_uuid.0.to_string()),
                    target_actor_id: damage.map(|event| event.target.actor_id.0.to_string()),
                    target_entity_uuid: damage.map(|event| event.target.entity_uuid.0.to_string()),
                    numerator: contribution.amount.to_string(),
                    denominator: "1".into(),
                    observed_damage: contribution.observed_damage.to_string(),
                    damage_context_complete: damage.is_some(),
                    formula_trace: None,
                };
                observe_relationship(
                    &mut influence_relationships,
                    envelope.sequence,
                    damage,
                    contribution.effect_id,
                    contribution.provider_actor_id,
                    contribution.recipient_actor_id,
                    &actor_entities,
                    contribution.amount as i128,
                    1,
                    contribution.observed_damage,
                );
                observe_cross_run_relationship(
                    &mut relationship_catalog,
                    report_index,
                    envelope.sequence,
                    &session_id,
                    &deployment_id,
                    &client_build,
                    &protocol_pack_digest,
                    damage,
                    contribution.effect_id,
                    contribution.amount as i128,
                    1,
                    contribution.observed_damage,
                );
                if !arguments.summary_only {
                    emitted_contribution_ledger.push(ledger_entry);
                }
                reducer.observe_exact_contribution(contribution);
            }
            for contribution in rational_contributions.iter().copied() {
                *emitted_contribution_events_by_effect
                    .entry(contribution.effect_id)
                    .or_default() += 1;
                let ledger_entry = EmittedContributionLedgerEntry {
                    sequence: envelope.sequence,
                    capture_sequence: capture_sequence(&envelope.provenance.source),
                    observed_micros: contribution.observed_micros,
                    effect_id: contribution.effect_id,
                    provider_actor_id: contribution.provider_actor_id,
                    provider_entity_uuid: actor_entities
                        .get(&contribution.provider_actor_id)
                        .map(ToString::to_string),
                    recipient_actor_id: contribution.recipient_actor_id,
                    recipient_entity_uuid: actor_entities
                        .get(&contribution.recipient_actor_id)
                        .map(ToString::to_string),
                    affected_damage_id: damage.and_then(|event| event.ability.map(|id| id.0)),
                    damage_source_actor_id: damage.map(|event| event.source.actor_id.0.to_string()),
                    damage_source_entity_uuid: damage
                        .map(|event| event.source.entity_uuid.0.to_string()),
                    target_actor_id: damage.map(|event| event.target.actor_id.0.to_string()),
                    target_entity_uuid: damage.map(|event| event.target.entity_uuid.0.to_string()),
                    numerator: contribution.numerator.to_string(),
                    denominator: contribution.denominator.to_string(),
                    observed_damage: contribution.observed_damage.to_string(),
                    damage_context_complete: damage.is_some(),
                    formula_trace: damage
                        .and_then(|damage| state_projector.harmony_grace_formula_trace(damage))
                        .filter(|trace| trace.effect_id == contribution.effect_id)
                        .map(HarmonyGraceFormulaTraceReport::from),
                };
                observe_relationship(
                    &mut influence_relationships,
                    envelope.sequence,
                    damage,
                    contribution.effect_id,
                    contribution.provider_actor_id,
                    contribution.recipient_actor_id,
                    &actor_entities,
                    contribution.numerator,
                    contribution.denominator,
                    contribution.observed_damage,
                );
                observe_cross_run_relationship(
                    &mut relationship_catalog,
                    report_index,
                    envelope.sequence,
                    &session_id,
                    &deployment_id,
                    &client_build,
                    &protocol_pack_digest,
                    damage,
                    contribution.effect_id,
                    contribution.numerator,
                    contribution.denominator,
                    contribution.observed_damage,
                );
                if !arguments.summary_only {
                    emitted_contribution_ledger.push(ledger_entry);
                }
                reducer.observe_exact_rational_contribution(contribution);
            }
            observe(&mut reducer, &envelope.event, envelope.time.observed_micros);
        }
        let canonical_content_sha256 = reader
            .summary()
            .ok_or_else(|| format!("{} is not a sealed canonical rlog", path.display()))?
            .content_sha256
            .clone();
        let summary = reducer.summary();
        let critical_cold_direct_projected_credit = critical_cold_direct_reducer
            .summary()
            .rational_effect_projections
            .iter()
            .filter(|projection| projection.effect_id == 2_204_471)
            .map(|projection| projection.amount)
            .sum();
        let team_luck_candidate_projection_by_source_actor = team_luck_candidate_reducer
            .summary()
            .rational_effect_projections
            .iter()
            .filter(|projection| projection.effect_id == 2_302_121)
            .fold(BTreeMap::<u64, i64>::new(), |mut projected, projection| {
                *projected.entry(projection.recipient_actor_id).or_default() += projection.amount;
                projected
            });
        let conserved = summary.is_conserved();
        if !conserved {
            return Err(format!("rDPS conservation failed for {}", path.display()).into());
        }
        total_events = total_events.saturating_add(event_count);
        reports.push(ReplayAuditReport {
            source_path: path.display().to_string(),
            session_id,
            canonical_content_sha256,
            deployment_id,
            client_build,
            protocol_pack_digest,
            runtime_target_match,
            candidate_audit_target_match,
            event_count,
            conserved,
            incomplete_rdps_actor_ids: incomplete_rdps_actor_ids.into_iter().collect(),
            emitted_contribution_events_by_effect,
            harmony_grace_audit_gates,
            harmony_grace_audit_examples,
            harmony_grace_family_rounding_diagnostics: harmony_grace_family_rounding_diagnostics
                .into_iter()
                .map(
                    |(diagnostic, accumulator)| HarmonyGraceFamilyRoundingDiagnosticCount {
                        damage_rows: accumulator.damage_rows,
                        first_damage_sequence: accumulator
                            .first_damage_sequence
                            .expect("a rounding diagnostic has at least one damage row"),
                        last_damage_sequence: accumulator
                            .last_damage_sequence
                            .expect("a rounding diagnostic has at least one damage row"),
                        sample_damage_sequences: accumulator.sample_damage_sequences,
                        diagnostic,
                    },
                )
                .collect(),
            team_luck_audit_gates,
            team_luck_audit_examples,
            team_luck_selector_rows_by_source_actor,
            team_luck_suppressed_examples_by_source_actor,
            team_luck_candidate_projection_by_source_actor,
            functional_amp_audit_gates,
            functional_amp_audit_examples,
            stat_resonance_audit_gates,
            stat_resonance_audit_examples,
            fiery_battle_will_audit_gates,
            fiery_battle_will_audit_examples,
            mechanical_power_audit_gates,
            mechanical_power_audit_examples,
            mechanical_power_audit_actions: finish_mechanical_power_audit_actions(
                mechanical_power_audit_actions,
            ),
            inspire_haste_audit_gates,
            inspire_haste_audit_examples,
            inspiration_audit_gates,
            inspiration_audit_examples,
            inspiration_occurrence_audit_gates,
            inspiration_occurrence_audit_examples,
            critical_cold_occurrence_audit_gates,
            critical_cold_occurrence_audit_examples,
            critical_cold_direct_candidate_count,
            critical_cold_direct_projected_credit,
            critical_cold_simultaneous_candidate_histogram,
            critical_cold_joint_audit_gates,
            critical_cold_joint_audit_examples,
            critical_cold_suppressed_context_histogram,
            critical_cold_suppressed_standalone_examples,
            thunderwind_audit_gates,
            thunderwind_audit_examples,
            fatal_spiral_audit_gates,
            fatal_spiral_audit_examples,
            target_vulnerability_audit_gates,
            target_vulnerability_audit_examples,
            influence_relationships: finish_relationships(influence_relationships),
            emitted_contribution_ledger,
            summary,
        });
    }
    let elapsed = started.elapsed();
    let elapsed_micros = elapsed.as_micros();
    let events_per_second = if elapsed.as_secs_f64() == 0.0 {
        0.0
    } else {
        total_events as f64 / elapsed.as_secs_f64()
    };
    let inspiration_combined_reconciliation = inspiration_combined_reconciliation
        .map(|accumulator| finish_inspiration_combined_reconciliation(accumulator, &reports));
    let attribution_mode = if inspiration_combined_reconciliation.is_some() {
        "focused_inspiration_combined_receipt_not_production_attribution"
    } else if arguments.inspiration_candidate_audit_enabled
        || arguments.harmony_grace_candidate_audit_enabled
        || arguments.mechanical_power_candidate_audit_enabled
        || arguments.mechanical_power_tier0_candidate_audit_enabled
        || arguments.target_vulnerability_candidate_audit_enabled
    {
        "offline_candidate_gate_audit_not_production_attribution"
    } else {
        "production_promoted_rules"
    };
    if arguments.compact {
        let mut receipt = compact_replay_receipt(
            attribution_mode,
            state_damage_contribution_deployment_id()?,
            state_damage_contribution_game_build()?,
            rule_effect_ids,
            total_events,
            &reports,
        );
        receipt.content_sha256 = compact_receipt_digest(&receipt)?;
        let mut writer = BufWriter::new(File::create(arguments.output)?);
        serde_json::to_writer_pretty(&mut writer, &receipt)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        return Ok(());
    }
    let bundle = ReplayAuditBundle {
        schema_version: RDPS_REPLAY_AUDIT_SCHEMA_VERSION,
        proof_scope: "packet_proven_rules_only_partial_game_coverage",
        attribution_mode,
        inspiration_candidate_audit_enabled: arguments.inspiration_candidate_audit_enabled,
        harmony_grace_candidate_audit_enabled: arguments.harmony_grace_candidate_audit_enabled,
        mechanical_power_candidate_audit_enabled: arguments
            .mechanical_power_candidate_audit_enabled,
        mechanical_power_tier0_candidate_audit_enabled: arguments
            .mechanical_power_tier0_candidate_audit_enabled,
        target_vulnerability_candidate_audit_enabled: arguments
            .target_vulnerability_candidate_audit_enabled,
        runtime_rule_deployment: state_damage_contribution_deployment_id()?,
        runtime_rule_build: state_damage_contribution_game_build()?,
        rule_effect_ids,
        target_vulnerability_candidate_effect_ids,
        total_events,
        elapsed_micros,
        events_per_second,
        relationship_catalog: finish_cross_run_relationships(relationship_catalog),
        reports,
        inspiration_combined_reconciliation,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn observe_cross_run_relationship(
    relationships: &mut BTreeMap<
        CrossRunInfluenceRelationshipKey,
        CrossRunInfluenceRelationshipAccumulator,
    >,
    report_index: usize,
    sequence: u64,
    session_id: &str,
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
    damage: Option<&DamageEvent>,
    effect_id: i64,
    numerator: i128,
    denominator: i128,
    observed_damage: i64,
) {
    let key = CrossRunInfluenceRelationshipKey {
        deployment_id: deployment_id.to_owned(),
        client_build: client_build.to_owned(),
        protocol_pack_digest: protocol_pack_digest.to_owned(),
        effect_id,
        affected_damage_id: damage.and_then(|event| event.ability.map(|id| id.0)),
        damage_context_complete: damage.is_some(),
    };
    let accumulator = relationships.entry(key).or_default();
    accumulator.sessions.insert(session_id.to_owned());
    let damage_event = (report_index, sequence);
    if accumulator.last_damage_event != Some(damage_event) {
        accumulator.last_damage_event = Some(damage_event);
        accumulator.damage_event_count = accumulator.damage_event_count.saturating_add(1);
        accumulator.observed_damage = accumulator
            .observed_damage
            .saturating_add(observed_damage.max(0) as i128);
    }
    observe_exact_delta(
        &mut accumulator.exact_integer_delta,
        &mut accumulator.rational_by_denominator,
        numerator,
        denominator,
    );
}

#[allow(clippy::too_many_arguments)]
fn observe_relationship(
    relationships: &mut BTreeMap<InfluenceRelationshipKey, InfluenceRelationshipAccumulator>,
    sequence: u64,
    damage: Option<&DamageEvent>,
    effect_id: i64,
    provider_actor_id: u64,
    recipient_actor_id: u64,
    actor_entities: &BTreeMap<u64, i64>,
    numerator: i128,
    denominator: i128,
    observed_damage: i64,
) {
    let key = InfluenceRelationshipKey {
        effect_id,
        provider_actor_id,
        provider_entity_uuid: actor_entities.get(&provider_actor_id).copied(),
        recipient_actor_id,
        recipient_entity_uuid: actor_entities.get(&recipient_actor_id).copied(),
        affected_damage_id: damage.and_then(|event| event.ability.map(|id| id.0)),
        damage_source_actor_id: damage.map(|event| event.source.actor_id.0),
        damage_source_entity_uuid: damage.map(|event| event.source.entity_uuid.0),
        target_actor_id: damage.map(|event| event.target.actor_id.0),
        target_entity_uuid: damage.map(|event| event.target.entity_uuid.0),
        damage_context_complete: damage.is_some(),
    };
    let accumulator = relationships.entry(key).or_default();
    if accumulator.last_sequence != Some(sequence) {
        accumulator.last_sequence = Some(sequence);
        accumulator.damage_event_count = accumulator.damage_event_count.saturating_add(1);
        accumulator.observed_damage = accumulator
            .observed_damage
            .saturating_add(observed_damage.max(0) as i128);
    }
    observe_exact_delta(
        &mut accumulator.exact_integer_delta,
        &mut accumulator.rational_by_denominator,
        numerator,
        denominator,
    );
}

fn observe_exact_delta(
    exact_integer_delta: &mut i128,
    rational_by_denominator: &mut BTreeMap<i128, (i128, u64)>,
    numerator: i128,
    denominator: i128,
) {
    if denominator == 1 {
        *exact_integer_delta = exact_integer_delta.saturating_add(numerator);
        return;
    }
    let (numerator, denominator) = reduce_fraction(numerator, denominator);
    let term = rational_by_denominator.entry(denominator).or_default();
    term.0 = term.0.saturating_add(numerator);
    term.1 = term.1.saturating_add(1);
}

fn finish_cross_run_relationships(
    relationships: BTreeMap<
        CrossRunInfluenceRelationshipKey,
        CrossRunInfluenceRelationshipAccumulator,
    >,
) -> Vec<CrossRunInfluenceRelationshipSummary> {
    relationships
        .into_iter()
        .map(|(key, accumulator)| CrossRunInfluenceRelationshipSummary {
            deployment_id: key.deployment_id,
            client_build: key.client_build,
            protocol_pack_digest: key.protocol_pack_digest,
            effect_id: key.effect_id,
            affected_damage_id: key.affected_damage_id,
            session_count: accumulator.sessions.len() as u64,
            damage_event_count: accumulator.damage_event_count,
            observed_damage: accumulator.observed_damage.to_string(),
            exact_integer_delta: accumulator.exact_integer_delta.to_string(),
            exact_rational_deltas: finish_rational_deltas(accumulator.rational_by_denominator),
            damage_context_complete: key.damage_context_complete,
            proof_status: "packet_replay_proven_exact_target",
        })
        .collect()
}

fn finish_relationships(
    relationships: BTreeMap<InfluenceRelationshipKey, InfluenceRelationshipAccumulator>,
) -> Vec<InfluenceRelationshipSummary> {
    relationships
        .into_iter()
        .map(|(key, accumulator)| InfluenceRelationshipSummary {
            effect_id: key.effect_id,
            provider_actor_id: key.provider_actor_id.to_string(),
            provider_entity_uuid: key.provider_entity_uuid.map(|value| value.to_string()),
            recipient_actor_id: key.recipient_actor_id.to_string(),
            recipient_entity_uuid: key.recipient_entity_uuid.map(|value| value.to_string()),
            affected_damage_id: key.affected_damage_id,
            damage_source_actor_id: key.damage_source_actor_id.map(|value| value.to_string()),
            damage_source_entity_uuid: key.damage_source_entity_uuid.map(|value| value.to_string()),
            target_actor_id: key.target_actor_id.map(|value| value.to_string()),
            target_entity_uuid: key.target_entity_uuid.map(|value| value.to_string()),
            damage_event_count: accumulator.damage_event_count,
            observed_damage: accumulator.observed_damage.to_string(),
            exact_integer_delta: accumulator.exact_integer_delta.to_string(),
            exact_rational_deltas: finish_rational_deltas(accumulator.rational_by_denominator),
            damage_context_complete: key.damage_context_complete,
        })
        .collect()
}

fn finish_rational_deltas(
    rational_by_denominator: BTreeMap<i128, (i128, u64)>,
) -> Vec<ExactRationalDeltaSummary> {
    rational_by_denominator
        .into_iter()
        .filter(|(_, (numerator, _))| *numerator != 0)
        .map(|(denominator, (numerator, contribution_count))| {
            let (numerator, denominator) = reduce_fraction(numerator, denominator);
            ExactRationalDeltaSummary {
                numerator: numerator.to_string(),
                denominator: denominator.to_string(),
                contribution_count,
            }
        })
        .collect()
}

fn reduce_fraction(numerator: i128, denominator: i128) -> (i128, i128) {
    debug_assert_ne!(denominator, 0);
    let sign = if denominator < 0 { -1 } else { 1 };
    let numerator = numerator.saturating_mul(sign);
    let denominator = denominator.saturating_mul(sign);
    let divisor = gcd_i128(numerator, denominator);
    (numerator / divisor, denominator / divisor)
}

fn gcd_i128(mut left: i128, mut right: i128) -> i128 {
    left = left.saturating_abs();
    right = right.saturating_abs();
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn damage_context(event: &CanonicalEvent) -> Option<&DamageEvent> {
    match event {
        CanonicalEvent::Timeline(timeline) => match &timeline.kind {
            TimelineEventKind::Damage(damage) => Some(damage),
            _ => None,
        },
        _ => None,
    }
}

fn record_audit_gate(
    gates: &mut BTreeMap<String, u64>,
    examples: &mut BTreeMap<String, Vec<String>>,
    gate: &'static str,
    damage: &DamageEvent,
    sequence: u64,
    capture_sequence: Option<u64>,
    observed_micros: u64,
) {
    *gates.entry(gate.into()).or_default() += 1;
    let gate_examples = examples.entry(gate.into()).or_default();
    if gate_examples.len() < 3 {
        gate_examples.push(format!(
            "sequence={sequence} capture_sequence={capture_sequence:?} observed_micros={observed_micros} ability={:?} hit={:?} critical={:?} lucky={:?} source={} target={} amount={}",
            damage.ability.map(|id| id.0),
            damage.hit_event_id,
            damage.flags.critical,
            damage.flags.lucky,
            damage.source.actor_id.0,
            damage.target.actor_id.0,
            damage.amount,
        ));
    }
}

fn record_mechanical_power_audit_gate(
    gates: &mut BTreeMap<String, u64>,
    examples: &mut BTreeMap<String, Vec<String>>,
    actions: &mut BTreeMap<
        String,
        BTreeMap<MechanicalPowerAuditActionKey, MechanicalPowerAuditActionAccumulator>,
    >,
    gate: &'static str,
    damage: &DamageEvent,
    sequence: u64,
    capture_sequence: Option<u64>,
    observed_micros: u64,
) {
    record_audit_gate(
        gates,
        examples,
        gate,
        damage,
        sequence,
        capture_sequence,
        observed_micros,
    );

    let key = MechanicalPowerAuditActionKey {
        ability_id: damage.ability.map(|id| id.0),
        hit_event_id: damage.hit_event_id,
        attributed_source_actor_id: damage.source.actor_id.0,
        attributed_source_entity_uuid: damage.source.entity_uuid.0,
        direct_source_actor_id: damage.direct_source.map(|source| source.actor_id.0),
        direct_source_entity_uuid: damage.direct_source.map(|source| source.entity_uuid.0),
        packet_attacker_uuid: damage.packet.attacker_uuid,
        packet_top_summoner_uuid: damage.packet.top_summoner_uuid,
        packet_owner_id: damage.packet.owner_id,
        owner_level: damage.packet.owner_level,
        owner_stage: damage.packet.owner_stage,
        target_actor_id: damage.target.actor_id.0,
        target_entity_uuid: damage.target.entity_uuid.0,
    };
    let accumulator = actions
        .entry(gate.into())
        .or_default()
        .entry(key)
        .or_default();
    accumulator.event_count = accumulator.event_count.saturating_add(1);
    accumulator.observed_amount_sum = accumulator
        .observed_amount_sum
        .saturating_add(i128::from(damage.amount));
    accumulator.first_sequence.get_or_insert(sequence);
    accumulator.last_sequence = Some(sequence);
    if let Some(capture_sequence) = capture_sequence {
        accumulator
            .first_capture_sequence
            .get_or_insert(capture_sequence);
        accumulator.last_capture_sequence = Some(capture_sequence);
    }
    accumulator
        .first_observed_micros
        .get_or_insert(observed_micros);
    accumulator.last_observed_micros = Some(observed_micros);
}

fn finish_mechanical_power_audit_actions(
    actions: BTreeMap<
        String,
        BTreeMap<MechanicalPowerAuditActionKey, MechanicalPowerAuditActionAccumulator>,
    >,
) -> BTreeMap<String, Vec<MechanicalPowerAuditActionSummary>> {
    actions
        .into_iter()
        .map(|(gate, rows)| {
            let rows = rows
                .into_iter()
                .map(|(key, accumulator)| MechanicalPowerAuditActionSummary {
                    ability_id: key.ability_id,
                    hit_event_id: key.hit_event_id,
                    attributed_source_actor_id: key.attributed_source_actor_id,
                    attributed_source_entity_uuid: key.attributed_source_entity_uuid,
                    direct_source_actor_id: key.direct_source_actor_id,
                    direct_source_entity_uuid: key.direct_source_entity_uuid,
                    packet_attacker_uuid: key.packet_attacker_uuid,
                    packet_top_summoner_uuid: key.packet_top_summoner_uuid,
                    packet_owner_id: key.packet_owner_id,
                    owner_level: key.owner_level,
                    owner_stage: key.owner_stage,
                    target_actor_id: key.target_actor_id,
                    target_entity_uuid: key.target_entity_uuid,
                    event_count: accumulator.event_count,
                    observed_amount_sum: accumulator.observed_amount_sum.to_string(),
                    first_sequence: accumulator
                        .first_sequence
                        .expect("a mechanical-power action has at least one event"),
                    last_sequence: accumulator
                        .last_sequence
                        .expect("a mechanical-power action has at least one event"),
                    first_capture_sequence: accumulator.first_capture_sequence,
                    last_capture_sequence: accumulator.last_capture_sequence,
                    first_observed_micros: accumulator
                        .first_observed_micros
                        .expect("a mechanical-power action has at least one event"),
                    last_observed_micros: accumulator
                        .last_observed_micros
                        .expect("a mechanical-power action has at least one event"),
                })
                .collect();
            (gate, rows)
        })
        .collect()
}

fn contribution_rules_for_target(
    rules: &[rlogs_combat::DamageContributionRule],
    deployment_id: &str,
    client_build: &str,
) -> Vec<rlogs_combat::DamageContributionRule> {
    if deployment_id == confirmed_damage_contribution_deployment_id()
        && client_build == confirmed_damage_contribution_game_build()
    {
        rules.to_vec()
    } else {
        Vec::new()
    }
}

fn observe_actor_identity(actor_entities: &mut BTreeMap<u64, i64>, event: &CanonicalEvent) {
    if let CanonicalEvent::Timeline(timeline) = event {
        if let TimelineEventKind::Actor(actor) = &timeline.kind {
            actor_entities.insert(actor.actor.actor_id.0, actor.actor.entity_uuid.0);
        }
    }
}

fn capture_sequence(source: &EvidenceSource) -> Option<u64> {
    match source {
        EvidenceSource::Wire {
            capture_sequence, ..
        } => Some(*capture_sequence),
        _ => None,
    }
}

fn observe(reducer: &mut DamageContributionReducer, event: &CanonicalEvent, observed_micros: u64) {
    match event {
        CanonicalEvent::Dungeon(dungeon) => {
            if dungeon.kind == DungeonEventKind::Entered {
                reducer.reset_statuses();
            }
        }
        CanonicalEvent::Timeline(timeline) => match &timeline.kind {
            TimelineEventKind::Actor(actor) => reducer.set_provider_eligible(
                actor.actor.actor_id.0,
                actor.state != ActorState::Despawned && actor.kind == ActorKind::Player,
            ),
            TimelineEventKind::Status(status) => {
                reducer.observe_status(ContributionStatusEvent {
                    observed_micros,
                    source_actor_id: status.source.map(|source| source.actor_id.0),
                    target_actor_id: status.target.actor_id.0,
                    effect_id: status.effect.0,
                    instance_id: status.instance_id.map(|instance| instance.0),
                    state: match status.state {
                        StatusState::Applied => ContributionStatusState::Applied,
                        StatusState::Refreshed => ContributionStatusState::Refreshed,
                        StatusState::Stacked => ContributionStatusState::Stacked,
                        StatusState::Consumed => ContributionStatusState::Consumed,
                        StatusState::Removed => ContributionStatusState::Removed,
                    },
                    stacks: status.stacks,
                    duration_millis: status.duration_millis,
                });
            }
            TimelineEventKind::Damage(damage) => {
                reducer.observe_damage(ContributionDamageEvent {
                    observed_micros,
                    source_actor_id: damage.source.actor_id.0,
                    target_actor_id: damage.target.actor_id.0,
                    amount: damage.amount.max(0),
                    included: true,
                });
            }
            TimelineEventKind::EncounterBoundary { state, .. }
                if matches!(
                    state,
                    EncounterState::Cleared | EncounterState::Wiped | EncounterState::Ended
                ) =>
            {
                reducer.reset_statuses();
            }
            TimelineEventKind::RunBoundary { state, .. }
                if matches!(
                    state,
                    RunState::Entered
                        | RunState::Completed
                        | RunState::Failed
                        | RunState::Ended
                        | RunState::Exited
                ) =>
            {
                reducer.reset_statuses();
            }
            _ => {}
        },
        _ => {}
    }
}

fn compact_replay_receipt(
    attribution_mode: &'static str,
    runtime_rule_deployment: &'static str,
    runtime_rule_build: &'static str,
    rule_effect_ids: Vec<i64>,
    total_events: u64,
    reports: &[ReplayAuditReport],
) -> CompactReplayReceiptBundle {
    let compact_reports = reports
        .iter()
        .map(|report| {
            let contribution_given = report
                .summary
                .actors
                .values()
                .map(|actor| i128::from(actor.contribution_given))
                .sum::<i128>();
            let contribution_received = report
                .summary
                .actors
                .values()
                .map(|actor| i128::from(actor.contribution_received))
                .sum::<i128>();
            CompactReplayReceipt {
                session_id: report.session_id.clone(),
                canonical_content_sha256: report.canonical_content_sha256.clone(),
                deployment_id: report.deployment_id.clone(),
                client_build: report.client_build.clone(),
                protocol_pack_digest: report.protocol_pack_digest.clone(),
                runtime_target_match: report.runtime_target_match,
                event_count: report.event_count,
                damage_event_count: report.summary.damage_event_count,
                attributed_damage_event_count: report.summary.attributed_damage_event_count,
                attributed_bonus_damage: report.summary.attributed_bonus_damage,
                missing_source_status_count: report.summary.missing_source_status_count,
                incomplete_rdps_actor_count: report.incomplete_rdps_actor_ids.len(),
                emitted_contribution_events_by_effect: report
                    .emitted_contribution_events_by_effect
                    .clone(),
                raw_damage: report.summary.raw_damage_total().to_string(),
                rdps_damage: report.summary.rdps_damage_total().to_string(),
                contribution_given: contribution_given.to_string(),
                contribution_received: contribution_received.to_string(),
                conserved: report.conserved,
            }
        })
        .collect::<Vec<_>>();
    CompactReplayReceiptBundle {
        schema_version: COMPACT_REPLAY_RECEIPT_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-replay-audit",
        proof_scope: "packet_proven_rules_only_partial_game_coverage",
        attribution_mode,
        runtime_rule_deployment,
        runtime_rule_build,
        rule_effect_ids,
        policy: CompactReplayReceiptPolicy {
            canonical_integrity_seal_required: true,
            exact_runtime_identity_required: true,
            production_promoted_rules_only: attribution_mode == "production_promoted_rules",
            exact_party_conservation_required: true,
            incomplete_actor_count_reported: true,
            raw_packet_payloads_included: false,
            source_paths_included: false,
            runtime_authority_changed: false,
        },
        total_events,
        total_damage_events: compact_reports
            .iter()
            .map(|report| report.damage_event_count)
            .sum(),
        total_attributed_damage_events: compact_reports
            .iter()
            .map(|report| report.attributed_damage_event_count)
            .sum(),
        total_attributed_bonus_damage: compact_reports
            .iter()
            .map(|report| i128::from(report.attributed_bonus_damage))
            .sum::<i128>()
            .to_string(),
        all_runtime_targets_match: compact_reports
            .iter()
            .all(|report| report.runtime_target_match),
        all_reports_conserved: compact_reports.iter().all(|report| report.conserved),
        reports: compact_reports,
        content_sha256: String::new(),
    }
}

fn compact_receipt_digest(
    receipt: &CompactReplayReceiptBundle,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(receipt)?;
    value
        .as_object_mut()
        .expect("serialized compact receipt must be an object")
        .remove("content_sha256");
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value)?)
    ))
}

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    inspiration_combined_authority: Option<PathBuf>,
    summary_only: bool,
    compact: bool,
    inspiration_candidate_audit_enabled: bool,
    harmony_grace_candidate_audit_enabled: bool,
    mechanical_power_candidate_audit_enabled: bool,
    mechanical_power_tier0_candidate_audit_enabled: bool,
    target_vulnerability_candidate_audit_enabled: bool,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let output = take_value(&mut values, "--output")?;
    let inspiration_combined_authority =
        take_optional_value(&mut values, "--inspiration-combined-authority")?;
    let compact = take_switch(&mut values, "--compact");
    let summary_only = take_switch(&mut values, "--summary-only") || compact;
    let inspiration_candidate_audit_enabled =
        take_switch(&mut values, "--audit-enable-inspiration-candidate");
    let harmony_grace_candidate_audit_enabled =
        take_switch(&mut values, "--audit-enable-harmony-grace-candidate");
    let mechanical_power_candidate_audit_enabled =
        take_switch(&mut values, "--audit-enable-mechanical-power-candidate");
    let mechanical_power_tier0_candidate_audit_enabled = take_switch(
        &mut values,
        "--audit-enable-mechanical-power-tier0-candidate",
    );
    let target_vulnerability_candidate_audit_enabled =
        take_switch(&mut values, "--audit-enable-target-vulnerability-candidate");
    if [
        inspiration_candidate_audit_enabled,
        harmony_grace_candidate_audit_enabled,
        mechanical_power_candidate_audit_enabled,
        mechanical_power_tier0_candidate_audit_enabled,
        target_vulnerability_candidate_audit_enabled,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count()
        > 1
    {
        return Err("select only one offline candidate audit mode".into());
    }
    if compact
        && (inspiration_combined_authority.is_some()
            || inspiration_candidate_audit_enabled
            || harmony_grace_candidate_audit_enabled
            || mechanical_power_candidate_audit_enabled
            || mechanical_power_tier0_candidate_audit_enabled
            || target_vulnerability_candidate_audit_enabled)
    {
        return Err("--compact is limited to production-promoted-rule replay receipts".into());
    }
    let mut rlogs = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".into());
        }
        values.remove(position);
        rlogs.insert(PathBuf::from(values.remove(position)));
    }
    while let Some(position) = values.iter().position(|value| value == "--rlog-dir") {
        if position + 1 >= values.len() {
            return Err("--rlog-dir requires a path".into());
        }
        values.remove(position);
        collect_rlogs(Path::new(&values.remove(position)), &mut rlogs)?;
    }
    if (rlogs.is_empty() && inspiration_combined_authority.is_none()) || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlogs: rlogs.into_iter().collect(),
        output: output.into(),
        inspiration_combined_authority: inspiration_combined_authority.map(PathBuf::from),
        summary_only,
        compact,
        inspiration_candidate_audit_enabled,
        harmony_grace_candidate_audit_enabled,
        mechanical_power_candidate_audit_enabled,
        mechanical_power_tier0_candidate_audit_enabled,
        target_vulnerability_candidate_audit_enabled,
    })
}

fn collect_rlogs(directory: &Path, output: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "cannot read rlog directory {}: {error}",
            directory.display()
        )
    })?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("cannot read entry under {}: {error}", directory.display()))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            format!("cannot inspect rlog candidate {}: {error}", path.display())
        })?;
        if file_type.is_dir() {
            collect_rlogs(&path, output)?;
        } else if file_type.is_file() && is_sealed_rlog_candidate(&path) {
            output.insert(path);
        }
    }
    Ok(())
}

fn is_sealed_rlog_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.ends_with(".rlog") && !name.ends_with(".partial.rlog")
}

fn take_switch(values: &mut Vec<OsString>, flag: &str) -> bool {
    if let Some(position) = values.iter().position(|value| value == flag) {
        values.remove(position);
        true
    } else {
        false
    }
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

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    values.remove(position);
    Ok(Some(values.remove(position)))
}

fn usage() -> String {
    "usage: rlogs-bpsr-rdps-replay-audit ((--rlog <sealed.rlog> | --rlog-dir <directory>)... | --inspiration-combined-authority <v19.json>) [--summary-only | --compact] [--audit-enable-inspiration-candidate | --audit-enable-harmony-grace-candidate | --audit-enable-mechanical-power-candidate | --audit-enable-mechanical-power-tier0-candidate | --audit-enable-target-vulnerability-candidate] --output <report.json>".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_events::{
        AbilityId, ActorId, DamageFlags, DamagePacketDetail, EntityRef, EntityUuid,
    };

    #[test]
    fn mechanical_power_action_audit_preserves_exact_summon_topology() {
        let mut gates = BTreeMap::new();
        let mut examples = BTreeMap::new();
        let mut actions = BTreeMap::new();
        let damage = DamageEvent {
            source: EntityRef {
                actor_id: ActorId(7),
                entity_uuid: EntityUuid(216_009_015_936),
            },
            direct_source: Some(EntityRef {
                actor_id: ActorId(194),
                entity_uuid: EntityUuid(753_728),
            }),
            target: EntityRef {
                actor_id: ActorId(10),
                entity_uuid: EntityUuid(900_001),
            },
            ability: Some(AbilityId(2_900_840)),
            amount: 111_019,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(3),
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail {
                attacker_uuid: Some(753_728),
                top_summoner_uuid: Some(216_009_015_936),
                owner_id: Some(2_900_840),
                owner_level: Some(80),
                owner_stage: Some(1),
                ..DamagePacketDetail::default()
            },
        };

        record_mechanical_power_audit_gate(
            &mut gates,
            &mut examples,
            &mut actions,
            "damage_stage_missing",
            &damage,
            35_500,
            Some(35_400),
            457_913_572,
        );
        record_mechanical_power_audit_gate(
            &mut gates,
            &mut examples,
            &mut actions,
            "damage_stage_missing",
            &damage,
            35_501,
            Some(35_401),
            457_913_600,
        );

        assert_eq!(gates["damage_stage_missing"], 2);
        let finished = finish_mechanical_power_audit_actions(actions);
        let rows = &finished["damage_stage_missing"];
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.ability_id, Some(2_900_840));
        assert_eq!(row.hit_event_id, Some(3));
        assert_eq!(row.attributed_source_actor_id, 7);
        assert_eq!(row.direct_source_actor_id, Some(194));
        assert_eq!(row.packet_attacker_uuid, Some(753_728));
        assert_eq!(row.packet_top_summoner_uuid, Some(216_009_015_936));
        assert_eq!(row.target_actor_id, 10);
        assert_eq!(row.event_count, 2);
        assert_eq!(row.observed_amount_sum, "222038");
        assert_eq!(row.first_sequence, 35_500);
        assert_eq!(row.last_sequence, 35_501);
    }

    #[test]
    fn relationship_summary_keeps_exact_terms_and_counts_damage_once_per_sequence() {
        let mut relationships = BTreeMap::new();
        let actor_entities = BTreeMap::from([(1, 101), (2, 202)]);
        observe_relationship(
            &mut relationships,
            10,
            None,
            300,
            1,
            2,
            &actor_entities,
            5,
            1,
            100,
        );
        observe_relationship(
            &mut relationships,
            10,
            None,
            300,
            1,
            2,
            &actor_entities,
            1,
            3,
            100,
        );
        observe_relationship(
            &mut relationships,
            11,
            None,
            300,
            1,
            2,
            &actor_entities,
            2,
            3,
            200,
        );

        let rows = finish_relationships(relationships);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.damage_event_count, 2);
        assert_eq!(row.provider_entity_uuid.as_deref(), Some("101"));
        assert_eq!(row.recipient_entity_uuid.as_deref(), Some("202"));
        assert_eq!(row.observed_damage, "300");
        assert_eq!(row.exact_integer_delta, "5");
        assert_eq!(row.exact_rational_deltas.len(), 1);
        assert_eq!(row.exact_rational_deltas[0].numerator, "1");
        assert_eq!(row.exact_rational_deltas[0].denominator, "1");
        assert_eq!(row.exact_rational_deltas[0].contribution_count, 2);
    }

    #[test]
    fn cross_run_catalog_keeps_builds_separate_and_counts_each_damage_event_once() {
        let mut relationships = BTreeMap::new();
        observe_cross_run_relationship(
            &mut relationships,
            0,
            10,
            "session-a",
            "global",
            "24252055",
            "digest-a",
            None,
            300,
            5,
            1,
            100,
        );
        observe_cross_run_relationship(
            &mut relationships,
            0,
            10,
            "session-a",
            "global",
            "24252055",
            "digest-a",
            None,
            300,
            1,
            3,
            100,
        );
        observe_cross_run_relationship(
            &mut relationships,
            1,
            10,
            "session-b",
            "global",
            "24252055",
            "digest-a",
            None,
            300,
            2,
            3,
            200,
        );
        observe_cross_run_relationship(
            &mut relationships,
            2,
            10,
            "session-c",
            "global",
            "24609362",
            "digest-b",
            None,
            300,
            9,
            1,
            300,
        );

        let rows = finish_cross_run_relationships(relationships);
        assert_eq!(rows.len(), 2);
        let historical = &rows[0];
        assert_eq!(historical.client_build, "24252055");
        assert_eq!(historical.session_count, 2);
        assert_eq!(historical.damage_event_count, 2);
        assert_eq!(historical.observed_damage, "300");
        assert_eq!(historical.exact_integer_delta, "5");
        assert_eq!(historical.exact_rational_deltas.len(), 1);
        assert_eq!(historical.exact_rational_deltas[0].numerator, "1");
        assert_eq!(historical.exact_rational_deltas[0].denominator, "1");
        assert_eq!(historical.exact_rational_deltas[0].contribution_count, 2);
        assert_eq!(rows[1].client_build, "24609362");
        assert_eq!(rows[1].exact_integer_delta, "9");
    }

    #[test]
    fn rlog_directory_filter_rejects_partial_and_unrelated_files() {
        assert!(is_sealed_rlog_candidate(Path::new("run-0001.rlog")));
        assert!(!is_sealed_rlog_candidate(Path::new(
            "run-0001.partial.rlog"
        )));
        assert!(!is_sealed_rlog_candidate(Path::new("run-0001.jsonl")));
    }

    #[test]
    fn rational_reduction_normalizes_sign_and_common_factors() {
        assert_eq!(reduce_fraction(12, -18), (-2, 3));
        assert_eq!(reduce_fraction(0, 15), (0, 1));
    }

    #[test]
    fn runtime_rules_fail_closed_for_every_other_deployment_or_client_build() {
        let rules = vec![rlogs_combat::DamageContributionRule {
            effect_id: 1,
            kind: rlogs_combat::DamageContributionKind::DirectDamageAmplification,
            magnitude_basis_points: 500,
            stacking: rlogs_combat::DamageContributionStacking::Fixed,
        }];
        assert_eq!(
            contribution_rules_for_target(
                &rules,
                confirmed_damage_contribution_deployment_id(),
                confirmed_damage_contribution_game_build()
            ),
            rules
        );
        assert!(contribution_rules_for_target(&rules, "global", "24609362").is_empty());
        assert!(
            contribution_rules_for_target(&rules, "cn", confirmed_damage_contribution_game_build())
                .is_empty()
        );
    }

    #[test]
    fn compact_receipt_is_self_hashing_and_path_free() {
        let mut receipt = CompactReplayReceiptBundle {
            schema_version: COMPACT_REPLAY_RECEIPT_SCHEMA_VERSION,
            generated_by: "rlogs-bpsr-rdps-replay-audit",
            proof_scope: "packet_proven_rules_only_partial_game_coverage",
            attribution_mode: "production_promoted_rules",
            runtime_rule_deployment: "global",
            runtime_rule_build: "24687926",
            rule_effect_ids: vec![31_602],
            policy: CompactReplayReceiptPolicy {
                canonical_integrity_seal_required: true,
                exact_runtime_identity_required: true,
                production_promoted_rules_only: true,
                exact_party_conservation_required: true,
                incomplete_actor_count_reported: true,
                raw_packet_payloads_included: false,
                source_paths_included: false,
                runtime_authority_changed: false,
            },
            total_events: 1,
            total_damage_events: 1,
            total_attributed_damage_events: 1,
            total_attributed_bonus_damage: "1".into(),
            all_runtime_targets_match: true,
            all_reports_conserved: true,
            reports: Vec::new(),
            content_sha256: String::new(),
        };
        let digest = compact_receipt_digest(&receipt).unwrap();
        receipt.content_sha256 = digest.clone();
        assert_eq!(compact_receipt_digest(&receipt).unwrap(), digest);
        let serialized = serde_json::to_string(&receipt).unwrap();
        assert!(!serialized.contains("\"source_path\":"));
        assert!(!serialized.contains("\"packet_payload\":"));
    }
}
