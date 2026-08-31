use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use rlogs_events::{
    CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, EvidenceSource, RunState,
    StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::{
    PacketDamageScriptFamily, exact_external_attack_and_damage_bonus_fraction,
    exact_external_attack_coefficient_stage_fraction, exact_external_composite_damage_fraction,
    specialization_identity_from_observed_abilities,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 47;
const DEFAULT_MAX_GAP_MICROS: u64 = 3_000_000;
const DEFAULT_EXAMPLE_LIMIT: usize = 16;
const RECENT_PER_CONTEXT: usize = 48;
const FIXED_POINT_SCALE: i128 = 10_000;
const CRITICAL_DAMAGE_ATTRIBUTE_ID: i32 = 12_510;
const LUCKY_DAMAGE_ATTRIBUTE_ID: i32 = 12_530;
const EXTERNAL_DAMAGE_ATTRIBUTE_ID: i32 = 11_840;
const MASTERY_ATTRIBUTE_ID: i32 = 11_940;
const VERSATILITY_ATTRIBUTE_ID: i32 = 11_950;
const DEFENSE_ATTRIBUTE_ID: i32 = 11_350;
const MAGIC_DEFENSE_ATTRIBUTE_ID: i32 = 11_360;
const SEASON_LEVEL_ATTRIBUTE_ID: i32 = 10_070;
const SEASON_STRENGTH_ATTRIBUTE_ID: i32 = 11_440;
const SEASON_WEAKNESS_ATTRIBUTE_ID: i32 = 11_450;
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const MAX_HP_ATTRIBUTE_ID: i32 = 11_320;
// AttrExtDamInc (11840) is packet-proven to be derived from AttrVersatilityPct
// (11950) in this build: floor(versatility * 35 / 100). They are alternative
// representations of one state axis, not independent damage multipliers.
const OFFENSIVE_VECTOR_FORMULAS: [(&str, bool, bool, bool); 6] = [
    ("attack_x_hit_outcome", false, false, false),
    ("attack_x_hit_outcome_x_mastery_bonus", true, false, false),
    (
        "attack_x_hit_outcome_x_derived_ext_damage_bonus",
        false,
        true,
        false,
    ),
    (
        "attack_x_hit_outcome_x_raw_versatility_bonus_hypothesis",
        false,
        false,
        true,
    ),
    (
        "attack_x_hit_outcome_x_mastery_bonus_x_derived_ext_damage_bonus",
        true,
        true,
        false,
    ),
    (
        "attack_x_hit_outcome_x_mastery_bonus_x_raw_versatility_bonus_hypothesis",
        true,
        false,
        true,
    ),
];

#[derive(Debug)]
struct Arguments {
    effect_id: i64,
    source_config_id: i64,
    attack_attribute_id: i32,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    max_gap_micros: u64,
    example_limit: usize,
    diagnostic_ignored_status_ids: BTreeSet<i64>,
    damage_surface: Option<PathBuf>,
    expected_game_build: Option<String>,
    attack_provider_delta: Option<AttackProviderDelta>,
    provider_external_damage_raw_delta: Option<i64>,
    provider_property_damage_attribute_id: Option<i32>,
    provider_property_damage_raw_delta: Option<i64>,
    required_damage_property: Option<i32>,
    required_provider_status: Option<RequiredProviderStatus>,
    source_entity_uuid: Option<i64>,
    transition_seeds: Option<PathBuf>,
    transition_window_micros: u64,
    effective_stat_windows: Option<PathBuf>,
    pair_proof_only: bool,
}

#[derive(Debug, Deserialize)]
struct TransitionSeedBundle {
    schema_version: u16,
    source_rlogs: Vec<String>,
    selected_effect_ids: Vec<i64>,
    exact_single_term_equation_occurrences: u64,
    retained_transition_seeds: u64,
    all_equation_occurrences_retained: bool,
    transitions: Vec<TransitionSeed>,
}

#[derive(Debug, Deserialize)]
struct TransitionSeed {
    effect_id: i64,
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    wire_observed_micros: u64,
}

#[derive(Debug)]
struct TransitionSeedFilter {
    source: String,
    window_micros: u64,
    seed_count: u64,
    seeds: BTreeMap<(String, u32, i64), Vec<u64>>,
}

impl TransitionSeedFilter {
    fn matches(
        &self,
        session_id: &str,
        run_ordinal: u32,
        source_entity_uuid: i64,
        observed_micros: u64,
    ) -> bool {
        self.seeds
            .get(&(session_id.to_owned(), run_ordinal, source_entity_uuid))
            .is_some_and(|seeds| {
                seeds
                    .iter()
                    .any(|seed| observed_micros.abs_diff(*seed) <= self.window_micros)
            })
    }
}

#[derive(Debug, Deserialize)]
struct EffectiveStatWindowBundle {
    schema_version: u16,
    game_build: String,
    effect_id: i64,
    summary: EffectiveStatWindowSummary,
    lifecycle_windows: Vec<EffectiveStatWindowInput>,
}

#[derive(Debug, Deserialize)]
struct EffectiveStatWindowSummary {
    exact_lifecycle_windows: u64,
    exact_attribute_boundaries: u64,
    unique_attribute_boundary_joins: u64,
    candidate_status_window_damage_actions: u64,
    effective_stat_window_damage_actions: u64,
    excluded_before_attribute_activation: u64,
    excluded_at_or_after_attribute_deactivation: u64,
    observed_damage_reassigned_to_provider: String,
}

#[derive(Debug, Deserialize)]
struct EffectiveStatWindowInput {
    session_id: String,
    run_ordinal: u32,
    effect_id: i64,
    status_instance_id: i64,
    provider_entity_uuid: String,
    affected_entity_uuid: String,
    status_lifecycle: EffectiveStatusLifecycleInput,
    effective_stat_window: EffectiveSequenceWindowInput,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Deserialize)]
struct EffectiveStatusLifecycleInput {
    application_sequence: u64,
    removal_sequence: u64,
}

#[derive(Debug, Deserialize)]
struct EffectiveSequenceWindowInput {
    first_exclusive_canonical_source_rlog_sequence: u64,
    last_exclusive_canonical_source_rlog_sequence: u64,
}

#[derive(Debug, Clone)]
struct EffectiveStatWindow {
    provider_entity_uuid: i64,
    application_sequence: u64,
    removal_sequence: u64,
    first_exclusive_sequence: u64,
    last_exclusive_sequence: u64,
}

#[derive(Debug)]
struct EffectiveStatWindowFilter {
    source: String,
    game_build: String,
    exact_lifecycle_windows: u64,
    candidate_status_window_damage_actions: u64,
    effective_stat_window_damage_actions: u64,
    excluded_before_attribute_activation: u64,
    excluded_at_or_after_attribute_deactivation: u64,
    windows: BTreeMap<(String, u32, i64), Vec<EffectiveStatWindow>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectiveStatClassification {
    EffectiveExternal(i64),
    SelectedLifecycleOutsideEffectiveWindow,
    OutsideSelectedLifecycle,
}

impl EffectiveStatWindowFilter {
    fn classify(
        &self,
        session_id: &str,
        run_ordinal: u32,
        affected_entity_uuid: i64,
        sequence: u64,
    ) -> EffectiveStatClassification {
        let Some(windows) =
            self.windows
                .get(&(session_id.to_owned(), run_ordinal, affected_entity_uuid))
        else {
            return EffectiveStatClassification::OutsideSelectedLifecycle;
        };
        for window in windows {
            if sequence > window.first_exclusive_sequence
                && sequence < window.last_exclusive_sequence
            {
                return EffectiveStatClassification::EffectiveExternal(window.provider_entity_uuid);
            }
            if sequence >= window.application_sequence && sequence <= window.removal_sequence {
                return EffectiveStatClassification::SelectedLifecycleOutsideEffectiveWindow;
            }
        }
        EffectiveStatClassification::OutsideSelectedLifecycle
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct RequiredProviderStatus {
    effect_id: i64,
    source_config_id: i64,
    expected_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttackProviderDelta {
    FinalAttack(i64),
    BaseAdd(i64),
    RawPercent(i64),
    DerivedPrimaryPercent {
        primary_attribute_id: i32,
        raw_percent_delta: i64,
        attack_add_numerator: i64,
        attack_add_denominator: i64,
    },
}

impl AttackProviderDelta {
    fn component(self) -> &'static str {
        match self {
            Self::FinalAttack(_) => "final_attack",
            Self::BaseAdd(_) => "base_add",
            Self::RawPercent(_) => "raw_percent",
            Self::DerivedPrimaryPercent { .. } => "derived_primary_percent",
        }
    }

    fn raw_delta(self) -> i64 {
        match self {
            Self::FinalAttack(value) | Self::BaseAdd(value) | Self::RawPercent(value) => value,
            Self::DerivedPrimaryPercent {
                raw_percent_delta, ..
            } => raw_percent_delta,
        }
    }

    fn primary_attribute_id(self) -> Option<i32> {
        match self {
            Self::DerivedPrimaryPercent {
                primary_attribute_id,
                ..
            } => Some(primary_attribute_id),
            Self::FinalAttack(_) | Self::BaseAdd(_) | Self::RawPercent(_) => None,
        }
    }

    fn attack_add_ratio(self) -> Option<(i64, i64)> {
        match self {
            Self::DerivedPrimaryPercent {
                attack_add_numerator,
                attack_add_denominator,
                ..
            } => Some((attack_add_numerator, attack_add_denominator)),
            Self::FinalAttack(_) | Self::BaseAdd(_) | Self::RawPercent(_) => None,
        }
    }

    fn is_final_attack(self) -> bool {
        matches!(self, Self::FinalAttack(_))
    }
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    selected_effect_id: i64,
    selected_source_config_id: i64,
    selected_source_config_is_absent: bool,
    final_attack_attribute_id: i32,
    damage_surface_identity: Option<DamageSurfaceIdentity>,
    attack_provider_delta: Option<AttackProviderDeltaReport>,
    provider_external_damage_raw_delta: Option<i64>,
    provider_property_damage_attribute_id: Option<i32>,
    provider_property_damage_raw_delta: Option<i64>,
    required_damage_property: Option<i32>,
    required_provider_status: Option<RequiredProviderStatus>,
    source_entity_uuid_filter: Option<i64>,
    transition_seed_selection: Option<TransitionSeedSelectionReport>,
    effective_stat_window_selection: Option<EffectiveStatWindowSelectionReport>,
    pair_proof_only: bool,
    diagnostic_ignored_status_ids: Vec<i64>,
    max_pair_gap_micros: u64,
    sessions: Vec<SessionSummary>,
    status_mismatch_inventory: Vec<StatusMismatchReport>,
    pair_groups: Vec<PairGroupReport>,
    formula: FormulaReport,
    status_controlled_offensive_vector_formulas: Vec<OffensiveVectorFormulaReport>,
    companion_normalized_offensive_vector_formulas: Vec<OffensiveVectorFormulaReport>,
    status_uncontrolled_diagnostic_pair_groups: Vec<PairGroupReport>,
    status_uncontrolled_diagnostic_formula: FormulaReport,
    status_uncontrolled_offensive_vector_formulas: Vec<OffensiveVectorFormulaReport>,
    live_vector_stability: LiveVectorStabilityReport,
    damage_time_attack_state_proof: DamageTimeAttackStateReport,
    direct_observed_counterfactuals: DirectObservedCounterfactualReport,
    archetype_observed_counterfactuals: ArchetypeObservedCounterfactualReport,
    single_event_damage_attr_counterfactual: SingleEventCounterfactualReport,
}

#[derive(Debug, Serialize)]
struct TransitionSeedSelectionReport {
    source: String,
    window_micros_before_and_after: u64,
    retained_transition_seeds: u64,
    selection_policy: &'static str,
    formula_authority: bool,
}

#[derive(Debug, Serialize)]
struct EffectiveStatWindowSelectionReport {
    source: String,
    game_build: String,
    exact_lifecycle_windows: u64,
    candidate_status_window_damage_actions: u64,
    effective_stat_window_damage_actions: u64,
    excluded_before_attribute_activation: u64,
    excluded_at_or_after_attribute_deactivation: u64,
    selection_policy: &'static str,
    formula_authority: bool,
    provider_credit_allowed: bool,
}

#[derive(Debug, Serialize)]
struct AttackProviderDeltaReport {
    component: &'static str,
    raw_delta: i64,
    primary_attribute_id: Option<i32>,
    attack_add_numerator: Option<i64>,
    attack_add_denominator: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_use: &'static str,
    analysis_scope: &'static str,
    occurrence_authority: &'static str,
    provider_control: &'static str,
    pair_scope: &'static str,
    calculation_state_timing: &'static str,
    status_control: &'static str,
    diagnostic_status_control: &'static str,
    attack_control: &'static str,
    target_attribute_control: &'static str,
    counterfactual_control: &'static str,
    live_vector_guard: &'static str,
    derived_attribute_alias_control: &'static str,
    replay_lifecycle_completeness: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DamageAttrRow {
    authority: &'static str,
    damage_id: String,
    required_damage_source: Option<i32>,
    damage_type: Option<i32>,
    damage_script: Option<String>,
    pve_damage_ratio: Vec<i64>,
    pve_fixed_parameter: Vec<i64>,
}

#[derive(Debug, Default)]
struct DamageSurface {
    identity: DamageSurfaceIdentity,
    rows_by_key: BTreeMap<(i64, i32), Vec<DamageAttrRow>>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct DamageSurfaceIdentity {
    path: String,
    bytes: u64,
    sha256: String,
    source_kind: String,
    game_build: Option<String>,
    schema_version: Option<u64>,
    generated_by: Option<String>,
    build_identity_verified: bool,
}

#[derive(Debug, Default)]
struct SingleEventCounterfactualAccumulator {
    external_active_damage_events: u64,
    events_with_all_required_attack_family_components: u64,
    attack_family_component_coverage: BTreeMap<i32, AttackFamilyComponentCoverageAccumulator>,
    events_with_exact_attack_family_reversal: u64,
    events_with_ability_hit_identity: u64,
    events_with_wire_hit_event_id: u64,
    events_using_semantic_zero_for_omitted_hit_event_id: u64,
    events_with_unique_damage_row: u64,
    events_with_matching_damage_script: u64,
    events_with_nonzero_damage_type: u64,
    events_with_unsupported_damage_script: u64,
    events_with_exact_stage_coefficient: u64,
    events_missing_stage_coefficient: u64,
    events_with_base_candidates: u64,
    events_matching_required_provider_status: u64,
    events_rejected_by_required_provider_status: u64,
    events_with_exact_conserved_attack_stage_share: u64,
    events_with_packet_normal_value: u64,
    events_where_packet_normal_value_matches_amount: u64,
    events_with_integer_post_base_factor_interval: u64,
    events_without_integer_post_base_factor_interval: u64,
    events_with_unique_integer_post_base_factor: u64,
    events_with_one_counterfactual_across_integer_factor_interval: u64,
    events_with_multiple_counterfactuals_across_integer_factor_interval: u64,
    events_with_one_exact_counterfactual_across_all_candidates: u64,
    events_with_ambiguous_counterfactual: u64,
    events_with_invalid_counterfactual: u64,
    observed_damage: i128,
    exact_conserved_share_observed_damage: i128,
    events_with_exact_conserved_attack_external_composite_share: u64,
    events_without_exact_conserved_attack_external_composite_share: u64,
    exact_conserved_attack_external_composite_observed_damage: i128,
    exact_counterfactual_damage: i128,
    exact_provider_marginal: i128,
    exact_conserved_share_buckets: BTreeMap<i128, ExactConservedShareBucketAccumulator>,
    exact_conserved_attack_external_composite_buckets:
        BTreeMap<i128, ExactConservedShareBucketAccumulator>,
    events_matching_required_damage_property: u64,
    events_rejected_by_required_damage_property: u64,
    events_with_exact_conserved_attack_external_property_composite_share: u64,
    events_without_exact_conserved_attack_external_property_composite_share: u64,
    exact_conserved_attack_external_property_composite_observed_damage: i128,
    exact_conserved_attack_external_property_composite_buckets:
        BTreeMap<i128, ExactConservedShareBucketAccumulator>,
    coverage_gaps: BTreeMap<SingleEventCoverageGapKey, SingleEventCoverageGapAccumulator>,
    diagnostics_by_action: BTreeMap<SingleEventDiagnosticKey, SingleEventDiagnosticAccumulator>,
    nonzero_damage_type_actions: BTreeMap<NonzeroDamageTypeKey, NonzeroDamageTypeAccumulator>,
    unsupported_damage_script_actions:
        BTreeMap<UnsupportedDamageScriptKey, UnsupportedDamageScriptAccumulator>,
    target_attribute_coverage: BTreeMap<i32, SingleEventTargetAttributeAccumulator>,
    shared_post_base_factor_groups:
        BTreeMap<SharedPostBaseFactorStateKey, Vec<SharedPostBaseFactorObservation>>,
    position_relaxed_post_base_factor_groups:
        BTreeMap<SharedPostBaseFactorStateKey, Vec<SharedPostBaseFactorObservation>>,
    position_current_hp_relaxed_post_base_factor_groups:
        BTreeMap<SharedPostBaseFactorStateKey, Vec<SharedPostBaseFactorObservation>>,
    position_current_hp_component_relaxed_post_base_factor_groups:
        BTreeMap<SharedPostBaseFactorStateKey, Vec<SharedPostBaseFactorObservation>>,
    action_position_component_relaxed_post_base_factor_groups:
        BTreeMap<SharedPostBaseFactorStateKey, Vec<SharedPostBaseFactorObservation>>,
    action_position_current_hp_component_relaxed_post_base_factor_groups:
        BTreeMap<SharedPostBaseFactorStateKey, Vec<SharedPostBaseFactorObservation>>,
    examples: Vec<SingleEventCounterfactualExample>,
}

#[derive(Debug, Default)]
struct AttackFamilyComponentCoverageAccumulator {
    events_present: u64,
    events_missing: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SharedPostBaseFactorStateKey {
    action_identity: Option<(i64, i32, String)>,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
    causes_lucky: Option<bool>,
    missed: Option<bool>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    hit_part_ids: Vec<Option<i32>>,
    damage_weight_bits: Option<(Option<u32>, Option<u32>)>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    packet_position: Option<DamagePositionBits>,
    hit_part_positions: Vec<Option<DamagePositionBits>>,
    source_current_hp: Option<i64>,
    source_max_hp: Option<i64>,
    target_current_hp: Option<i64>,
    target_max_hp: Option<i64>,
    source_formula_attributes: Vec<(i32, i64)>,
    target_formula_attributes: Option<Vec<(i32, i64)>>,
    source_statuses: Vec<SemanticStatusEntry>,
    target_statuses: Vec<SemanticStatusEntry>,
}

#[derive(Debug, Clone)]
struct SharedPostBaseFactorObservation {
    sequence: u64,
    ability_id: i64,
    hit_event_id: i32,
    active_base: i64,
    counterfactual_base: i64,
    observed_damage: i64,
    minimum_factor: i64,
    maximum_factor: i64,
}

#[derive(Debug, Serialize)]
struct SharedPostBaseFactorReport {
    authority: &'static str,
    state_identity: &'static str,
    groups: u64,
    groups_with_multiple_events: u64,
    multi_event_groups_with_compatible_intersection: u64,
    multi_event_groups_with_disjoint_intersection: u64,
    multi_event_groups_with_unique_factor: u64,
    events_in_compatible_multi_event_groups: u64,
    events_with_one_counterfactual_from_shared_interval: u64,
    events_newly_resolved_from_shared_interval: u64,
    observed_damage_newly_resolved_from_shared_interval: String,
    provider_marginal_newly_resolved_from_shared_interval: String,
    compatible_examples: Vec<SharedPostBaseFactorExample>,
    disjoint_examples: Vec<SharedPostBaseFactorExample>,
}

#[derive(Debug, Serialize)]
struct SharedPostBaseFactorExample {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    event_count: usize,
    distinct_actions: Vec<String>,
    shared_factor_minimum_basis_points: Option<i64>,
    shared_factor_maximum_basis_points: Option<i64>,
    observations: Vec<SharedPostBaseFactorObservationReport>,
}

#[derive(Debug, Serialize)]
struct SharedPostBaseFactorObservationReport {
    sequence: u64,
    ability_id: i64,
    hit_event_id: i32,
    active_base: i64,
    counterfactual_base: i64,
    observed_damage: i64,
    individual_factor_minimum_basis_points: i64,
    individual_factor_maximum_basis_points: i64,
    shared_interval_counterfactual_minimum: Option<i64>,
    shared_interval_counterfactual_maximum: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SingleEventCoverageGapKey {
    reason: &'static str,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    property: i32,
    critical: bool,
    lucky: bool,
    candidate_rows: Vec<String>,
}

#[derive(Debug, Default)]
struct SingleEventCoverageGapAccumulator {
    events: u64,
    observed_damage: i128,
    examples: Vec<SingleEventCoverageGapExample>,
}

#[derive(Debug, Serialize)]
struct SingleEventCoverageGapReport {
    reason: &'static str,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    property: i32,
    critical: bool,
    lucky: bool,
    candidate_rows: Vec<String>,
    events: u64,
    observed_damage: String,
    examples: Vec<SingleEventCoverageGapExample>,
}

#[derive(Debug, Clone, Serialize)]
struct SingleEventCoverageGapExample {
    rlog: String,
    session_id: String,
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    observed_damage: i64,
    packet_normal_value: Option<i64>,
    packet_lucky_value: Option<i64>,
    skill_effect_uuid: Option<i64>,
    skill_effect_total_damage: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    passive_uuid: Option<u32>,
    damage_mode: Option<i32>,
    active_attack: i64,
    source_formula_attributes: Vec<AttributeVectorValue>,
    target_formula_attributes: Vec<AttributeVectorValue>,
    source_statuses: Vec<SemanticStatusEntry>,
    target_statuses: Vec<SemanticStatusEntry>,
}

#[derive(Debug, Default)]
struct ExactConservedShareBucketAccumulator {
    events: u64,
    observed_damage: i128,
    provider_numerator: i128,
}

#[derive(Debug, Serialize)]
struct ExactConservedShareBucketReport {
    denominator: String,
    events: u64,
    observed_damage: String,
    provider_numerator: String,
    recipient_numerator: String,
    observed_numerator: String,
    conservation_identity_holds: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NonzeroDamageTypeKey {
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    damage_type: i32,
    damage_script: String,
    owner_stage: Option<i32>,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
}

#[derive(Debug, Default)]
struct NonzeroDamageTypeAccumulator {
    events: u64,
    observed_damage: i128,
}

#[derive(Debug, Serialize)]
struct NonzeroDamageTypeReport {
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    damage_type: i32,
    damage_script: String,
    owner_stage: Option<i32>,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
    events: u64,
    observed_damage: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UnsupportedDamageScriptKey {
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    damage_type: Option<i32>,
    damage_script: String,
    owner_stage: Option<i32>,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
}

#[derive(Debug, Default)]
struct UnsupportedDamageScriptAccumulator {
    events: u64,
    observed_damage: i128,
}

#[derive(Debug, Serialize)]
struct UnsupportedDamageScriptReport {
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    damage_type: Option<i32>,
    damage_script: String,
    owner_stage: Option<i32>,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
    events: u64,
    observed_damage: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SingleEventDiagnosticKey {
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    damage_script: String,
    owner_stage: Option<i32>,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
}

#[derive(Debug, Default)]
struct SingleEventDiagnosticAccumulator {
    events: u64,
    events_with_target_attribute_snapshot: u64,
    events_with_target_physical_defense: u64,
    events_with_target_magical_defense: u64,
    events_with_target_season_level: u64,
    events_with_target_season_strength_or_weakness: u64,
    events_with_integer_factor_interval: u64,
    events_without_integer_factor_interval: u64,
    events_with_one_counterfactual_across_factor_interval: u64,
    events_with_multiple_counterfactuals_across_factor_interval: u64,
    target_identities: BTreeMap<(i64, Option<i64>), SingleEventTargetIdentityAccumulator>,
}

#[derive(Debug, Default)]
struct SingleEventTargetIdentityAccumulator {
    events: u64,
    events_with_integer_factor_interval: u64,
    events_without_integer_factor_interval: u64,
}

#[derive(Debug, Default)]
struct SingleEventTargetAttributeAccumulator {
    events_present: u64,
    events_with_integer_factor_interval: u64,
    events_without_integer_factor_interval: u64,
}

#[derive(Debug, Serialize)]
struct SingleEventDiagnosticReport {
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    damage_script: String,
    owner_stage: Option<i32>,
    critical: bool,
    lucky: bool,
    blocked: bool,
    periodic: bool,
    events: u64,
    events_with_target_attribute_snapshot: u64,
    events_with_target_physical_defense: u64,
    events_with_target_magical_defense: u64,
    events_with_target_season_level: u64,
    events_with_target_season_strength_or_weakness: u64,
    events_with_integer_factor_interval: u64,
    events_without_integer_factor_interval: u64,
    events_with_one_counterfactual_across_factor_interval: u64,
    events_with_multiple_counterfactuals_across_factor_interval: u64,
    target_identities: Vec<SingleEventTargetIdentityReport>,
}

#[derive(Debug, Serialize)]
struct SingleEventTargetIdentityReport {
    target_entity_uuid: i64,
    monster_id: Option<i64>,
    events: u64,
    events_with_integer_factor_interval: u64,
    events_without_integer_factor_interval: u64,
}

#[derive(Debug, Serialize)]
struct SingleEventTargetAttributeReport {
    attribute_id: i32,
    events_present: u64,
    events_with_integer_factor_interval: u64,
    events_without_integer_factor_interval: u64,
}

#[derive(Debug, Serialize)]
struct SingleEventCounterfactualReport {
    authority: &'static str,
    model: &'static str,
    coefficient_selection: &'static str,
    fixed_parameter_selection: &'static str,
    fixed_parameter_placement: &'static str,
    external_active_damage_events: u64,
    attack_family_reversal_policy: &'static str,
    events_with_all_required_attack_family_components: u64,
    attack_family_component_coverage: Vec<AttackFamilyComponentCoverageReport>,
    events_with_exact_attack_family_reversal: u64,
    events_without_exact_attack_family_reversal: u64,
    events_with_ability_hit_identity: u64,
    hit_event_identity: &'static str,
    events_with_wire_hit_event_id: u64,
    events_using_semantic_zero_for_omitted_hit_event_id: u64,
    events_with_unique_damage_row: u64,
    events_with_matching_damage_script: u64,
    events_with_nonzero_damage_type: u64,
    events_with_unsupported_damage_script: u64,
    events_with_exact_stage_coefficient: u64,
    events_missing_stage_coefficient: u64,
    events_with_base_candidates: u64,
    required_provider_status_policy: &'static str,
    events_matching_required_provider_status: u64,
    events_rejected_by_required_provider_status: u64,
    exact_conserved_attack_stage_share_model: &'static str,
    events_with_exact_conserved_attack_stage_share: u64,
    exact_conserved_share_observed_damage: String,
    exact_conserved_share_buckets: Vec<ExactConservedShareBucketReport>,
    exact_conserved_share_coverage_gaps: Vec<SingleEventCoverageGapReport>,
    exact_conserved_attack_external_composite_model: &'static str,
    configured_provider_external_damage_raw_delta: Option<i64>,
    events_with_exact_conserved_attack_external_composite_share: u64,
    events_without_exact_conserved_attack_external_composite_share: u64,
    exact_conserved_attack_external_composite_observed_damage: String,
    exact_conserved_attack_external_composite_buckets: Vec<ExactConservedShareBucketReport>,
    exact_conserved_attack_external_property_composite_model: &'static str,
    configured_provider_property_damage_attribute_id: Option<i32>,
    configured_provider_property_damage_raw_delta: Option<i64>,
    configured_required_damage_property: Option<i32>,
    events_matching_required_damage_property: u64,
    events_rejected_by_required_damage_property: u64,
    events_with_exact_conserved_attack_external_property_composite_share: u64,
    events_without_exact_conserved_attack_external_property_composite_share: u64,
    exact_conserved_attack_external_property_composite_observed_damage: String,
    exact_conserved_attack_external_property_composite_buckets:
        Vec<ExactConservedShareBucketReport>,
    events_with_packet_normal_value: u64,
    events_where_packet_normal_value_matches_amount: u64,
    integer_post_base_factor_model: &'static str,
    events_with_integer_post_base_factor_interval: u64,
    events_without_integer_post_base_factor_interval: u64,
    events_with_unique_integer_post_base_factor: u64,
    events_with_one_counterfactual_across_integer_factor_interval: u64,
    events_with_multiple_counterfactuals_across_integer_factor_interval: u64,
    events_with_one_exact_counterfactual_across_all_candidates: u64,
    events_with_ambiguous_counterfactual: u64,
    events_with_invalid_counterfactual: u64,
    shared_post_base_factor_diagnostic: SharedPostBaseFactorReport,
    position_relaxed_post_base_factor_diagnostic: SharedPostBaseFactorReport,
    position_current_hp_relaxed_post_base_factor_diagnostic: SharedPostBaseFactorReport,
    position_current_hp_component_relaxed_post_base_factor_diagnostic: SharedPostBaseFactorReport,
    action_position_component_relaxed_post_base_factor_diagnostic: SharedPostBaseFactorReport,
    action_position_current_hp_component_relaxed_post_base_factor_diagnostic:
        SharedPostBaseFactorReport,
    observed_damage: String,
    exact_counterfactual_damage: String,
    exact_provider_marginal: String,
    nonzero_damage_type_actions: Vec<NonzeroDamageTypeReport>,
    unsupported_damage_script_actions: Vec<UnsupportedDamageScriptReport>,
    diagnostics_by_action: Vec<SingleEventDiagnosticReport>,
    target_attribute_coverage: Vec<SingleEventTargetAttributeReport>,
    examples: Vec<SingleEventCounterfactualExample>,
}

#[derive(Debug, Serialize)]
struct AttackFamilyComponentCoverageReport {
    attribute_id: i32,
    events_present: u64,
    events_missing: u64,
}

#[derive(Debug, Clone, Serialize)]
struct SingleEventCounterfactualExample {
    rlog: String,
    session_id: String,
    sequence: u64,
    run_ordinal: u32,
    source_entity_uuid: i64,
    provider_entity_uuid: i64,
    target_entity_uuid: i64,
    target_monster_id: Option<i64>,
    target_level: Option<u32>,
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    damage_script: String,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    damage_type: Option<i32>,
    property: Option<i32>,
    critical: bool,
    lucky: bool,
    damage_weight_bits: Option<(Option<u32>, Option<u32>)>,
    damage_weight_values: Option<(Option<f32>, Option<f32>)>,
    observed_damage: i64,
    packet_normal_value: Option<i64>,
    packet_actual_value: Option<i64>,
    active_attack: i64,
    counterfactual_attack: i64,
    selected_coefficient: i64,
    fixed_parameter_candidates: Vec<i64>,
    exact_conserved_provider_share_numerator: Option<String>,
    exact_conserved_provider_share_denominator: Option<String>,
    counterfactual_minimum: i64,
    counterfactual_maximum: i64,
    integer_post_base_factor_minimum_basis_points: Option<i64>,
    integer_post_base_factor_maximum_basis_points: Option<i64>,
    integer_factor_counterfactual_minimum: Option<i64>,
    integer_factor_counterfactual_maximum: Option<i64>,
    exact_integer_factor_counterfactual: Option<i64>,
    exact_counterfactual: Option<i64>,
    exact_provider_marginal: Option<i64>,
    source_formula_attributes: Vec<AttributeVectorValue>,
    target_formula_attributes: Vec<AttributeVectorValue>,
    source_statuses: Vec<SemanticStatusEntry>,
    target_statuses: Vec<SemanticStatusEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DamageTimeAttackStateContext {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    unaffected_formula_attributes: Vec<(i32, i64)>,
    other_source_statuses: Vec<SemanticStatusEntry>,
}

#[derive(Debug, Default)]
struct DamageTimeAttackStateAccumulator {
    inactive_attacks: BTreeSet<i64>,
    external_attacks_by_provider: BTreeMap<i64, BTreeSet<i64>>,
    inactive_damage_events: u64,
    external_damage_events: u64,
    external_damage_events_by_provider: BTreeMap<i64, u64>,
    external_damage_events_by_provider_attack: BTreeMap<(i64, i64), u64>,
    exact_family_reversals_by_active_attack: BTreeMap<i64, BTreeSet<i64>>,
    external_samples_by_provider: BTreeMap<i64, Vec<(DamageContext, DamageSample)>>,
    first_inactive_sequence: Option<u64>,
    first_external_sequence_by_provider: BTreeMap<i64, u64>,
}

#[derive(Debug, Serialize)]
struct DamageTimeAttackStateReport {
    authority: &'static str,
    comparison_scope: &'static str,
    excluded_selected_effect_attributes: Vec<i32>,
    contexts: u64,
    contexts_with_external_damage: u64,
    contexts_with_external_and_inactive_damage: u64,
    exact_reversible_contexts: u64,
    ambiguous_reversible_contexts: u64,
    external_damage_events: u64,
    external_damage_events_with_exact_observed_counterfactual_attack: u64,
    external_damage_events_with_unique_run_actor_family_lookup: u64,
    external_damage_events_without_run_actor_family_lookup: u64,
    external_damage_events_with_ambiguous_run_actor_family_lookup: u64,
    external_damage_events_without_inactive_context: u64,
    external_damage_events_with_ambiguous_context: u64,
    exact_reversible_damage_events: Vec<ExactReversibleDamageEvent>,
    examples: Vec<DamageTimeAttackStateExample>,
    actor_attack_surfaces: Vec<ActorAttackSurfaceReport>,
}

#[derive(Debug, Serialize)]
struct ExactReversibleDamageEvent {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    target_monster_id: Option<i64>,
    target_level: Option<u32>,
    provider_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    amount: i64,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    actual_value: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    active_attack: i64,
    counterfactual_attack: i64,
    provider_attack_marginal: i64,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: bool,
    lucky: bool,
    causes_lucky: Option<bool>,
    blocked: bool,
    periodic: bool,
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
    skill_effect_uuid: Option<i64>,
    skill_effect_total_damage: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    hit_part_damage_values: Vec<Option<i64>>,
    source_formula_attributes: Vec<AttributeVectorValue>,
    target_formula_attributes: Vec<AttributeVectorValue>,
    source_statuses: Vec<SemanticStatusEntry>,
    target_statuses: Vec<SemanticStatusEntry>,
    damage_row_candidates: Vec<DamageAttrRow>,
}

#[derive(Debug, Serialize)]
struct DamageTimeAttackStateExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    provider_entity_uuid: i64,
    inactive_attack: i64,
    active_attack: i64,
    provider_attack_marginal: i64,
    inactive_damage_events: u64,
    external_damage_events: u64,
    first_inactive_sequence: Option<u64>,
    first_external_sequence: Option<u64>,
    controlled_attribute_count: usize,
    controlled_status_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DirectObservedContext {
    rlog: String,
    session_id: String,
    damage: DamageContext,
    unaffected_source_attributes: Vec<(i32, i64)>,
    target_attributes: Vec<(i32, i64)>,
    source_statuses: Vec<SemanticStatusEntry>,
    target_statuses: Vec<SemanticStatusEntry>,
}

#[derive(Debug, Default)]
struct DirectObservedContextAccumulator {
    inactive: BTreeMap<(i64, i64), u64>,
    external: BTreeMap<(i64, i64, i64, i64), u64>,
    first_inactive_sequence: Option<u64>,
    first_external_sequence: Option<u64>,
}

#[derive(Debug, Serialize)]
struct DirectObservedCounterfactualReport {
    authority: &'static str,
    context_control: &'static str,
    contexts: u64,
    contexts_with_both_states: u64,
    exact_contexts: u64,
    ambiguous_contexts: u64,
    exact_external_events: u64,
    exact_observed_damage: String,
    exact_counterfactual_damage: String,
    exact_provider_marginal: String,
    examples: Vec<DirectObservedCounterfactualExample>,
}

#[derive(Debug, Serialize)]
struct DirectObservedCounterfactualExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    provider_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    critical: bool,
    lucky: bool,
    inactive_attack: i64,
    active_attack: i64,
    inactive_damage: i64,
    active_damage: i64,
    provider_attack_marginal: i64,
    provider_damage_marginal: i64,
    external_events: u64,
    first_inactive_sequence: Option<u64>,
    first_external_sequence: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct DamagePositionBits {
    x: Option<u32>,
    y: Option<u32>,
    z: Option<u32>,
}

impl From<rlogs_events::DamagePosition> for DamagePositionBits {
    fn from(value: rlogs_events::DamagePosition) -> Self {
        Self {
            x: value.x.map(f32::to_bits),
            y: value.y.map(f32::to_bits),
            z: value.z.map(f32::to_bits),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ArchetypeObservedContext {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    target_monster_id: i64,
    target_level: u32,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: bool,
    lucky: bool,
    causes_lucky: Option<bool>,
    blocked: bool,
    periodic: bool,
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
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    unaffected_source_attributes: Vec<(i32, i64)>,
    target_attributes: Vec<(i32, i64)>,
    source_current_hp: Option<i64>,
    source_max_hp: Option<i64>,
    target_max_hp: Option<i64>,
    source_statuses: Vec<SemanticStatusEntry>,
    target_statuses: Vec<SemanticStatusEntry>,
}

#[derive(Debug, Default)]
struct ArchetypeOutcomeEvidence {
    events: u64,
    first_sequence: Option<u64>,
    target_entity_uuids: BTreeSet<i64>,
    target_current_hp_values: BTreeSet<i64>,
    events_without_target_current_hp: u64,
    packet_positions: BTreeSet<DamagePositionBits>,
    events_without_packet_position: u64,
    hit_part_positions: BTreeSet<Vec<Option<DamagePositionBits>>>,
    events_without_complete_hit_part_positions: u64,
    skill_effect_group_indices: BTreeSet<u32>,
}

#[derive(Debug, Default)]
struct ArchetypeObservedContextAccumulator {
    inactive: BTreeMap<(i64, i64), ArchetypeOutcomeEvidence>,
    external: BTreeMap<(i64, i64, i64), ArchetypeOutcomeEvidence>,
}

#[derive(Debug, Serialize)]
struct ArchetypeObservedCounterfactualReport {
    authority: &'static str,
    context_control: &'static str,
    invariance_gate: &'static str,
    contexts: u64,
    contexts_with_both_states: u64,
    contexts_with_unique_state_outcomes: u64,
    contexts_passing_entity_invariance: u64,
    contexts_passing_position_invariance: u64,
    contexts_passing_target_hp_invariance: u64,
    exact_contexts: u64,
    rejected_ambiguous_state_outcome: u64,
    rejected_insufficient_entity_diversity: u64,
    rejected_missing_or_insufficient_position_diversity: u64,
    rejected_missing_or_insufficient_target_hp_diversity: u64,
    exact_external_events: u64,
    exact_observed_damage: String,
    exact_counterfactual_damage: String,
    exact_provider_marginal: String,
    overlap_diagnostics: Vec<ArchetypeOverlapDiagnostic>,
    target_state_status_mismatch_examples: Vec<ArchetypeStatusMismatchExample>,
    examples: Vec<ArchetypeObservedCounterfactualExample>,
}

#[derive(Debug, Serialize)]
struct ArchetypeOverlapDiagnostic {
    stage: &'static str,
    controlled_dimensions: &'static str,
    inactive_keys: usize,
    external_keys: usize,
    keys_with_both_states: usize,
}

#[derive(Debug, Serialize)]
struct ArchetypeStatusMismatchExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_monster_id: i64,
    target_level: u32,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    inactive_source_status_snapshots: Vec<Vec<SemanticStatusEntry>>,
    external_source_status_snapshots: Vec<Vec<SemanticStatusEntry>>,
    inactive_target_status_snapshots: Vec<Vec<SemanticStatusEntry>>,
    external_target_status_snapshots: Vec<Vec<SemanticStatusEntry>>,
    inactive_outcomes: Vec<ArchetypeStatusOutcome>,
    external_outcomes: Vec<ArchetypeStatusOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ArchetypeStatusOutcome {
    provider_entity_uuid: Option<i64>,
    attack: i64,
    damage: i64,
    events: u64,
    first_sequence: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ArchetypeObservedCounterfactualExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    provider_entity_uuid: i64,
    target_monster_id: i64,
    target_level: u32,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    critical: bool,
    lucky: bool,
    inactive_attack: i64,
    active_attack: i64,
    inactive_damage: i64,
    active_damage: i64,
    provider_attack_marginal: i64,
    provider_damage_marginal: i64,
    inactive_events: u64,
    external_events: u64,
    inactive_target_entities: usize,
    external_target_entities: usize,
    inactive_target_hp_values: usize,
    external_target_hp_values: usize,
    inactive_packet_positions: usize,
    external_packet_positions: usize,
    inactive_hit_part_position_vectors: usize,
    external_hit_part_position_vectors: usize,
    inactive_group_indices: usize,
    external_group_indices: usize,
    first_inactive_sequence: Option<u64>,
    first_external_sequence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ActorAttackSurfaceKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
}

#[derive(Debug, Default)]
struct ActorAttackSurfaceAccumulator {
    inactive_attacks: BTreeSet<i64>,
    external_attacks_by_provider: BTreeMap<i64, BTreeSet<i64>>,
    inactive_damage_events: u64,
    external_damage_events_by_provider: BTreeMap<i64, u64>,
    external_damage_events_by_provider_attack: BTreeMap<(i64, i64), u64>,
    exact_family_reversals_by_active_attack: BTreeMap<i64, BTreeSet<i64>>,
}

#[derive(Debug, Serialize)]
struct ActorAttackSurfaceReport {
    authority: &'static str,
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    inactive_attacks: Vec<i64>,
    inactive_damage_events: u64,
    providers: Vec<ActorAttackProviderSurfaceReport>,
}

#[derive(Debug, Serialize)]
struct ActorAttackProviderSurfaceReport {
    provider_entity_uuid: i64,
    active_attacks: Vec<i64>,
    external_damage_events: u64,
    active_attack_lookups: Vec<ActiveAttackLookupReport>,
}

#[derive(Debug, Serialize)]
struct ActiveAttackLookupReport {
    active_attack: i64,
    counterfactual_attacks: Vec<i64>,
    external_damage_events: u64,
    exact: bool,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    rlog: String,
    session_id: String,
    run_ordinals_observed: u32,
    damage_events: u64,
    damage_events_with_attack: u64,
    selected_status_events: u64,
    external_active_damage_events: u64,
    inactive_damage_events: u64,
    ambiguous_or_self_active_damage_events: u64,
    effect_transition_candidates_before_controls: u64,
    strict_pairs: u64,
    rejected_status_mismatch: u64,
    rejected_target_attribute_mismatch: u64,
    rejected_target_attribute_unknown: u64,
    companion_normalized_pairs: u64,
    rejected_same_attack: u64,
    externally_affected_actor_specializations: Vec<ActorSpecializationReport>,
    ability_transition_candidates: Vec<AbilityTransitionCandidateReport>,
}

#[derive(Debug, Serialize)]
struct ActorSpecializationReport {
    source_entity_uuid: i64,
    observed_ability_ids: Vec<i64>,
    resolved_class_id: Option<i32>,
    resolved_specialization_id: Option<i32>,
    observed_status_routes: Vec<ActorObservedStatusRouteReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ActorObservedStatusRouteKey {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    source_config_id: Option<i64>,
}

#[derive(Debug, Default)]
struct ActorObservedStatusRouteAccumulator {
    lifecycle_events: u64,
    external_active_damage_events: u64,
}

#[derive(Debug, Serialize)]
struct ActorObservedStatusRouteReport {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    source_config_id: Option<i64>,
    lifecycle_events: u64,
    external_active_damage_events: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AbilityTransitionCandidateKey {
    ability_id: Option<i64>,
    critical: bool,
    lucky: bool,
}

#[derive(Debug, Default)]
struct AbilityTransitionCandidateAccumulator {
    transitions: u64,
    status_mismatches: u64,
    target_attribute_mismatches: u64,
    target_attribute_unknown: u64,
    companion_normalized_pairs: u64,
    strict_pairs: u64,
}

#[derive(Debug, Serialize)]
struct AbilityTransitionCandidateReport {
    ability_id: Option<i64>,
    critical: bool,
    lucky: bool,
    transitions: u64,
    status_mismatches: u64,
    target_attribute_mismatches: u64,
    target_attribute_unknown: u64,
    companion_normalized_pairs: u64,
    strict_pairs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StatusMismatchKey {
    ability_id: Option<i64>,
    owner_side: &'static str,
    direction: &'static str,
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    source_config_id: Option<i64>,
    source_relation: &'static str,
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
}

#[derive(Debug, Serialize)]
struct StatusMismatchReport {
    ability_id: Option<i64>,
    owner_side: &'static str,
    direction: &'static str,
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    source_config_id: Option<i64>,
    source_relation: &'static str,
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    candidate_occurrences: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PairGroupKey {
    critical: bool,
    lucky: bool,
    attack_delta: i64,
}

#[derive(Debug, Default)]
struct PairGroupAccumulator {
    pairs: u64,
    paired_counterfactual_proof_eligible_pairs: u64,
    source_multi_axis_pairs: u64,
    packet_input_mismatch_pairs: u64,
    exact_ratio_matches: u64,
    within_one_ratio_matches: u64,
    mismatches: u64,
    unique_counterfactuals: u64,
    unique_counterfactuals_matching_paired_inactive: u64,
    ambiguous_counterfactuals: u64,
    impossible_counterfactuals: u64,
    examples: Vec<PairExample>,
}

#[derive(Debug, Serialize)]
struct PairGroupReport {
    critical: bool,
    lucky: bool,
    attack_delta: i64,
    pairs: u64,
    paired_counterfactual_proof_eligible_pairs: u64,
    source_multi_axis_pairs: u64,
    packet_input_mismatch_pairs: u64,
    exact_ratio_matches: u64,
    within_one_ratio_matches: u64,
    mismatches: u64,
    unique_counterfactuals: u64,
    unique_counterfactuals_matching_paired_inactive: u64,
    ambiguous_counterfactuals: u64,
    impossible_counterfactuals: u64,
    examples: Vec<PairExample>,
}

#[derive(Debug, Default)]
struct FormulaAccumulator {
    pairs: u64,
    exact_ratio_matches: u64,
    within_one_ratio_matches: u64,
    mismatches: u64,
    unique_counterfactuals: u64,
    unique_counterfactuals_matching_paired_inactive: u64,
    ambiguous_counterfactuals: u64,
    impossible_counterfactuals: u64,
    maximum_absolute_residual: u64,
    residuals: BTreeSet<i64>,
}

#[derive(Debug, Serialize)]
struct FormulaReport {
    hypothesis: &'static str,
    paired_prediction: &'static str,
    exact_counterfactual_interval: &'static str,
    pairs: u64,
    exact_ratio_matches: u64,
    within_one_ratio_matches: u64,
    mismatches: u64,
    unique_counterfactuals: u64,
    unique_counterfactuals_matching_paired_inactive: u64,
    ambiguous_counterfactuals: u64,
    impossible_counterfactuals: u64,
    maximum_absolute_residual: u64,
    residual_examples: Vec<i64>,
}

#[derive(Debug, Default)]
struct OffensiveVectorFormulaAccumulator {
    evaluable_pairs: u64,
    exact_matches: u64,
    within_one_matches: u64,
    mismatches: u64,
    maximum_absolute_residual: u64,
    residuals: BTreeSet<i64>,
    examples: Vec<OffensiveVectorPairExample>,
}

#[derive(Debug, Serialize)]
struct OffensiveVectorFormulaReport {
    formula: &'static str,
    authority: &'static str,
    evaluable_pairs: u64,
    exact_matches: u64,
    within_one_matches: u64,
    mismatches: u64,
    maximum_absolute_residual: u64,
    residual_examples: Vec<i64>,
    examples: Vec<OffensiveVectorPairExample>,
}

#[derive(Debug, Clone, Serialize)]
struct OffensiveVectorPairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    property: Option<i32>,
    critical: bool,
    lucky: bool,
    inactive_sequence: u64,
    active_sequence: u64,
    inactive_damage: i64,
    active_damage: i64,
    inactive_attack: i64,
    active_attack: i64,
    inactive_critical_damage: Option<i64>,
    active_critical_damage: Option<i64>,
    inactive_lucky_damage: Option<i64>,
    active_lucky_damage: Option<i64>,
    inactive_mastery: Option<i64>,
    active_mastery: Option<i64>,
    inactive_external_damage: Option<i64>,
    active_external_damage: Option<i64>,
    inactive_versatility: Option<i64>,
    active_versatility: Option<i64>,
    inactive_source_attributes: Vec<AttributeVectorValue>,
    active_source_attributes: Vec<AttributeVectorValue>,
    target_attributes: Vec<AttributeVectorValue>,
    changed_source_attributes: Vec<AttributeVectorDelta>,
    changed_target_attributes: Vec<AttributeVectorDelta>,
    inactive_factor: String,
    active_factor: String,
    predicted_active_damage: i64,
    residual: i64,
}

#[derive(Debug, Clone, Serialize)]
struct AttributeVectorDelta {
    attribute_id: i32,
    inactive_value: Option<i64>,
    active_value: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct AttributeVectorValue {
    attribute_id: i32,
    value: i64,
}

#[derive(Debug, Default)]
struct LiveVectorStabilityAccumulator {
    conflicting_pairs: u64,
    examples: Vec<LiveVectorSnapshotConflict>,
}

#[derive(Debug, Serialize)]
struct LiveVectorStabilityReport {
    authority: &'static str,
    interpretation: &'static str,
    conflicting_pairs: u64,
    examples: Vec<LiveVectorSnapshotConflict>,
}

#[derive(Debug, Clone, Serialize)]
struct LiveVectorSnapshotConflict {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    critical: bool,
    lucky: bool,
    selected_effect_presence: &'static str,
    provider_entity_uuid: Option<i64>,
    earlier_sequence: u64,
    later_sequence: u64,
    identical_damage: i64,
    changed_source_attributes: Vec<AttributeVectorDelta>,
}

#[derive(Debug, Clone, Serialize)]
struct PairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    provider_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    property: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    critical: bool,
    lucky: bool,
    inactive_sequence: u64,
    active_sequence: u64,
    gap_micros: u64,
    inactive_damage: i64,
    active_damage: i64,
    inactive_attack: i64,
    active_attack: i64,
    attack_delta: i64,
    source_formula_attribute_deltas: Vec<AttributeVectorDelta>,
    attack_only_source_vector_controlled: bool,
    paired_packet_input_deltas: Vec<&'static str>,
    paired_packet_inputs_controlled: bool,
    eligible_for_paired_counterfactual_proof: bool,
    proof_classification: &'static str,
    predicted_active_damage_from_ratio: i64,
    ratio_residual: i64,
    counterfactual_minimum: i64,
    counterfactual_maximum: i64,
    counterfactual_is_compatible: bool,
    unique_counterfactual: Option<i64>,
    unique_marginal: Option<i64>,
    damage_stage: Option<PairDamageStageProof>,
    damage_stage_gap: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct PairDamageStageProof {
    authority: &'static str,
    damage_id: String,
    damage_script: String,
    coefficient_basis_points: i64,
    fixed_parameter: i64,
    active_base: i64,
    inactive_base: i64,
    provider_base_marginal: i64,
    exact_conserved_attack_stage_share_numerator: String,
    exact_conserved_attack_stage_share_denominator: String,
    attack_only_counterfactual_minimum: i64,
    attack_only_counterfactual_maximum: i64,
    paired_inactive_matches_attack_only_counterfactual: bool,
    paired_observed_damage_delta: i64,
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
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    actual_value: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    causes_lucky: Option<bool>,
    missed: Option<bool>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_total_damage: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    hit_part_damage_values: Vec<Option<i64>>,
    packet_position: Option<DamagePositionBits>,
    hit_part_positions: Vec<Option<DamagePositionBits>>,
    target_monster_id: Option<i64>,
    target_level: Option<u32>,
    source_current_hp: Option<i64>,
    source_max_hp: Option<i64>,
    target_current_hp: Option<i64>,
    target_max_hp: Option<i64>,
    attack: i64,
    critical_damage: Option<i64>,
    lucky_damage: Option<i64>,
    external_damage: Option<i64>,
    mastery: Option<i64>,
    versatility: Option<i64>,
    formula_attributes: Arc<BTreeMap<i32, i64>>,
    target_formula_attributes: Option<Arc<BTreeMap<i32, i64>>>,
    effect_presence: EffectPresence,
    source_statuses: Vec<SemanticStatusEntry>,
    target_statuses: Vec<SemanticStatusEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectPresence {
    Inactive,
    External(i64),
    SelfOwned,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StatusKey {
    effect_id: i64,
    instance_id: Option<i64>,
    source_entity_uuid: Option<i64>,
    source_config_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusValue {
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct SemanticStatusEntry {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    source_config_id: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
}

#[derive(Debug, Default, Clone)]
struct StatusTracker {
    active: BTreeMap<StatusKey, StatusValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WireMessageKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
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

impl StatusTracker {
    fn observe(&mut self, key: StatusKey, value: StatusValue, state: StatusState) {
        match state {
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                self.active.insert(key, value);
            }
            StatusState::Consumed if value.stacks.unwrap_or_default() > 0 => {
                self.active.insert(key, value);
            }
            StatusState::Consumed | StatusState::Removed => {
                self.active.remove(&key);
            }
        }
    }

    fn selected_presence(
        &self,
        effect_id: i64,
        source_config_id: i64,
        target_entity_uuid: i64,
    ) -> EffectPresence {
        let selected = self
            .active
            .keys()
            .filter(|key| selected_status_key_matches(key, effect_id, source_config_id))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return EffectPresence::Inactive;
        }
        if selected.iter().any(|key| key.source_entity_uuid.is_none()) {
            return EffectPresence::Ambiguous;
        }
        let providers = selected
            .into_iter()
            .filter_map(|key| key.source_entity_uuid)
            .collect::<BTreeSet<_>>();
        if providers.len() != 1 {
            return EffectPresence::Ambiguous;
        }
        let provider = *providers.first().expect("one provider was checked");
        if provider == target_entity_uuid {
            EffectPresence::SelfOwned
        } else {
            EffectPresence::External(provider)
        }
    }

    fn has_status_from_provider(
        &self,
        required: RequiredProviderStatus,
        provider_entity_uuid: i64,
    ) -> bool {
        self.active.keys().any(|key| {
            key.effect_id == required.effect_id
                && key.source_config_id == Some(required.source_config_id)
                && key.source_entity_uuid == Some(provider_entity_uuid)
        })
    }

    fn semantic_snapshot_without_selected(
        &self,
        effect_id: i64,
        source_config_id: i64,
    ) -> Vec<SemanticStatusEntry> {
        self.active
            .iter()
            .filter(|(key, _)| !selected_status_key_matches(key, effect_id, source_config_id))
            .map(|(key, value)| SemanticStatusEntry {
                effect_id: key.effect_id,
                source_entity_uuid: key.source_entity_uuid,
                source_config_id: key.source_config_id,
                stacks: value.stacks,
                level: value.level,
                part_id: value.part_id,
                count: value.count,
            })
            .collect()
    }
}

fn selected_status_key_matches(key: &StatusKey, effect_id: i64, source_config_id: i64) -> bool {
    key.effect_id == effect_id
        && if source_config_id == 0 {
            key.source_config_id.is_none()
        } else {
            key.source_config_id == Some(source_config_id)
        }
}

fn semantic_snapshots_equal_ignoring(
    left: &[SemanticStatusEntry],
    right: &[SemanticStatusEntry],
    ignored_effect_ids: &BTreeSet<i64>,
) -> bool {
    left.iter()
        .filter(|entry| !ignored_effect_ids.contains(&entry.effect_id))
        .eq(right
            .iter()
            .filter(|entry| !ignored_effect_ids.contains(&entry.effect_id)))
}

fn is_formula_vector_attribute(attribute_id: i32) -> bool {
    matches!(
        attribute_id,
        11_010..=11_155
            | 11_330..=11_585
            | 11_710..=11_785
            | 11_830..=11_995
            | 12_510..=12_805
            | 13_000..=13_410
    )
}

fn new_offensive_vector_accumulators() -> BTreeMap<&'static str, OffensiveVectorFormulaAccumulator>
{
    let mut formulas = BTreeMap::new();
    for (formula, _, _, _) in OFFENSIVE_VECTOR_FORMULAS {
        formulas.insert(formula, OffensiveVectorFormulaAccumulator::default());
    }
    formulas
}

fn offensive_vector_reports(
    formulas: BTreeMap<&'static str, OffensiveVectorFormulaAccumulator>,
    authority: &'static str,
) -> Vec<OffensiveVectorFormulaReport> {
    formulas
        .into_iter()
        .map(|(formula, value)| OffensiveVectorFormulaReport {
            formula,
            authority,
            evaluable_pairs: value.evaluable_pairs,
            exact_matches: value.exact_matches,
            within_one_matches: value.within_one_matches,
            mismatches: value.mismatches,
            maximum_absolute_residual: value.maximum_absolute_residual,
            residual_examples: value.residuals.into_iter().collect(),
            examples: value.examples,
        })
        .collect()
}

fn main() {
    if let Err(error) = run() {
        eprintln!("external Attack damage proof failed: {error}");
        std::process::exit(1);
    }
}

fn ensure_selected_status_lifecycle(
    selected_status_events: u64,
    effect_id: i64,
    source_config_id: i64,
) -> Result<(), String> {
    if selected_status_events == 0 {
        return Err(format!(
            "none of the supplied replay inputs contains lifecycle events for selected effect {effect_id} with source config selector {source_config_id} (0 means the canonical source config is absent); use a status-aware current-decoder replay instead of a damage-only compact snapshot"
        ));
    }
    Ok(())
}

fn normalized_path(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

fn load_transition_seed_filter(
    path: &Path,
    args: &Arguments,
) -> Result<TransitionSeedFilter, Box<dyn std::error::Error>> {
    let bundle: TransitionSeedBundle = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    if bundle.schema_version != 1
        || !bundle.all_equation_occurrences_retained
        || bundle.exact_single_term_equation_occurrences != bundle.retained_transition_seeds
        || bundle.retained_transition_seeds != bundle.transitions.len() as u64
        || bundle.transitions.is_empty()
    {
        return Err(
            "transition seed input must be complete schema 1 exact single-term evidence".into(),
        );
    }
    if !bundle.selected_effect_ids.contains(&args.effect_id) {
        return Err(format!(
            "transition seed input does not select requested effect {}",
            args.effect_id
        )
        .into());
    }
    let selected_source = args
        .source_entity_uuid
        .ok_or("--transition-seeds requires --source-entity-uuid")?;
    let declared_inputs = bundle
        .source_rlogs
        .iter()
        .map(|value| normalized_path(value))
        .collect::<BTreeSet<_>>();
    let actual_inputs = args
        .rlogs
        .iter()
        .map(|value| normalized_path(&value.to_string_lossy()))
        .collect::<BTreeSet<_>>();
    if declared_inputs != actual_inputs {
        return Err("transition seed source_rlogs must exactly match supplied rlog inputs".into());
    }
    let mut seeds = BTreeMap::<(String, u32, i64), Vec<u64>>::new();
    for transition in bundle.transitions {
        if transition.effect_id != args.effect_id
            || transition.target_entity_uuid != selected_source
        {
            return Err(
                "every transition seed must match the requested effect and selected recipient"
                    .into(),
            );
        }
        seeds
            .entry((
                transition.session_id,
                transition.run_ordinal,
                transition.target_entity_uuid,
            ))
            .or_default()
            .push(transition.wire_observed_micros);
    }
    for values in seeds.values_mut() {
        values.sort_unstable();
    }
    Ok(TransitionSeedFilter {
        source: path.to_string_lossy().replace('\\', "/"),
        window_micros: args.transition_window_micros,
        seed_count: bundle.retained_transition_seeds,
        seeds,
    })
}

fn parse_decimal_i64(value: &str, field: &str) -> Result<i64, Box<dyn std::error::Error>> {
    value
        .parse::<i64>()
        .map_err(|error| format!("invalid {field} value {value:?}: {error}").into())
}

fn load_effective_stat_window_filter(
    path: &Path,
    args: &Arguments,
) -> Result<EffectiveStatWindowFilter, Box<dyn std::error::Error>> {
    let bundle: EffectiveStatWindowBundle =
        serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let expected_build = args
        .expected_game_build
        .as_deref()
        .ok_or("--effective-stat-windows requires --expected-game-build")?;
    if bundle.schema_version != 1
        || bundle.game_build != expected_build
        || bundle.effect_id != args.effect_id
        || bundle.lifecycle_windows.is_empty()
        || bundle.summary.exact_lifecycle_windows != bundle.lifecycle_windows.len() as u64
        || bundle.summary.exact_attribute_boundaries
            != bundle.summary.exact_lifecycle_windows.saturating_mul(2)
        || bundle.summary.unique_attribute_boundary_joins
            != bundle.summary.exact_attribute_boundaries
        || bundle.summary.observed_damage_reassigned_to_provider != "0"
    {
        return Err("effective stat window input must be complete schema 1 exact current-build lifecycle evidence with zero provider credit".into());
    }

    let mut windows = BTreeMap::<(String, u32, i64), Vec<EffectiveStatWindow>>::new();
    let mut status_instances = BTreeSet::<(String, u32, i64)>::new();
    for input in bundle.lifecycle_windows {
        let provider_entity_uuid =
            parse_decimal_i64(&input.provider_entity_uuid, "provider_entity_uuid")?;
        let affected_entity_uuid =
            parse_decimal_i64(&input.affected_entity_uuid, "affected_entity_uuid")?;
        let first = input
            .effective_stat_window
            .first_exclusive_canonical_source_rlog_sequence;
        let last = input
            .effective_stat_window
            .last_exclusive_canonical_source_rlog_sequence;
        if input.effect_id != args.effect_id
            || input.provider_rdps_credit_allowed
            || input.status_lifecycle.application_sequence > input.status_lifecycle.removal_sequence
            || first >= last
            || first > input.status_lifecycle.removal_sequence
            || last < input.status_lifecycle.application_sequence
            || !status_instances.insert((
                input.session_id.clone(),
                input.run_ordinal,
                input.status_instance_id,
            ))
        {
            return Err("effective stat window rows must be unique, ordered, selected-effect lifecycle rows that explicitly forbid provider credit".into());
        }
        windows
            .entry((input.session_id, input.run_ordinal, affected_entity_uuid))
            .or_default()
            .push(EffectiveStatWindow {
                provider_entity_uuid,
                application_sequence: input.status_lifecycle.application_sequence,
                removal_sequence: input.status_lifecycle.removal_sequence,
                first_exclusive_sequence: first,
                last_exclusive_sequence: last,
            });
    }
    for values in windows.values_mut() {
        values.sort_by_key(|window| window.application_sequence);
        if values.windows(2).any(|pair| {
            pair[0].removal_sequence >= pair[1].application_sequence
                || pair[0].last_exclusive_sequence >= pair[1].first_exclusive_sequence
        }) {
            return Err(
                "effective stat windows must not overlap for one session/run/recipient".into(),
            );
        }
    }
    Ok(EffectiveStatWindowFilter {
        source: path.to_string_lossy().replace('\\', "/"),
        game_build: bundle.game_build,
        exact_lifecycle_windows: bundle.summary.exact_lifecycle_windows,
        candidate_status_window_damage_actions: bundle
            .summary
            .candidate_status_window_damage_actions,
        effective_stat_window_damage_actions: bundle.summary.effective_stat_window_damage_actions,
        excluded_before_attribute_activation: bundle.summary.excluded_before_attribute_activation,
        excluded_at_or_after_attribute_deactivation: bundle
            .summary
            .excluded_at_or_after_attribute_deactivation,
        windows,
    })
}

fn gate_effect_presence_by_effective_stat_window(
    observed_presence: EffectPresence,
    filter: Option<&EffectiveStatWindowFilter>,
    session_id: &str,
    run_ordinal: u32,
    affected_entity_uuid: i64,
    sequence: u64,
) -> EffectPresence {
    let Some(filter) = filter else {
        return observed_presence;
    };
    match filter.classify(session_id, run_ordinal, affected_entity_uuid, sequence) {
        EffectiveStatClassification::EffectiveExternal(expected_provider) => {
            match observed_presence {
                EffectPresence::External(observed_provider)
                    if observed_provider == expected_provider =>
                {
                    EffectPresence::External(observed_provider)
                }
                EffectPresence::Inactive
                | EffectPresence::External(_)
                | EffectPresence::SelfOwned
                | EffectPresence::Ambiguous => EffectPresence::Ambiguous,
            }
        }
        EffectiveStatClassification::SelectedLifecycleOutsideEffectiveWindow => {
            EffectPresence::Ambiguous
        }
        EffectiveStatClassification::OutsideSelectedLifecycle => match observed_presence {
            EffectPresence::Inactive => EffectPresence::Inactive,
            EffectPresence::SelfOwned => EffectPresence::SelfOwned,
            EffectPresence::External(_) | EffectPresence::Ambiguous => EffectPresence::Ambiguous,
        },
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(env::args_os().skip(1))?;
    let transition_seed_filter = args
        .transition_seeds
        .as_deref()
        .map(|path| load_transition_seed_filter(path, &args))
        .transpose()?;
    let effective_stat_window_filter = args
        .effective_stat_windows
        .as_deref()
        .map(|path| load_effective_stat_window_filter(path, &args))
        .transpose()?;
    let damage_surface = args
        .damage_surface
        .as_deref()
        .map(|path| load_damage_surface(path, args.expected_game_build.as_deref()))
        .transpose()?;
    let mut groups = BTreeMap::<PairGroupKey, PairGroupAccumulator>::new();
    let mut formula = FormulaAccumulator::default();
    let mut diagnostic_groups = BTreeMap::<PairGroupKey, PairGroupAccumulator>::new();
    let mut diagnostic_formula = FormulaAccumulator::default();
    let mut strict_vector_formulas = new_offensive_vector_accumulators();
    let mut companion_normalized_vector_formulas = new_offensive_vector_accumulators();
    let mut diagnostic_vector_formulas = new_offensive_vector_accumulators();
    let mut live_vector_stability = LiveVectorStabilityAccumulator::default();
    let mut damage_time_attack_states =
        BTreeMap::<DamageTimeAttackStateContext, DamageTimeAttackStateAccumulator>::new();
    let mut direct_observed_contexts =
        BTreeMap::<DirectObservedContext, DirectObservedContextAccumulator>::new();
    let mut archetype_observed_contexts =
        BTreeMap::<ArchetypeObservedContext, ArchetypeObservedContextAccumulator>::new();
    let mut single_event_counterfactual = SingleEventCounterfactualAccumulator::default();
    let mut status_mismatches = BTreeMap::<StatusMismatchKey, u64>::new();
    let mut sessions = Vec::new();
    for rlog in &args.rlogs {
        sessions.push(read_session(
            rlog,
            &args,
            &mut groups,
            &mut formula,
            &mut status_mismatches,
            &mut diagnostic_groups,
            &mut diagnostic_formula,
            &mut strict_vector_formulas,
            &mut companion_normalized_vector_formulas,
            &mut diagnostic_vector_formulas,
            &mut live_vector_stability,
            &mut damage_time_attack_states,
            &mut direct_observed_contexts,
            &mut archetype_observed_contexts,
            damage_surface.as_ref(),
            &mut single_event_counterfactual,
            transition_seed_filter.as_ref(),
            effective_stat_window_filter.as_ref(),
        )?);
    }
    ensure_selected_status_lifecycle(
        sessions
            .iter()
            .map(|session| session.selected_status_events)
            .sum(),
        args.effect_id,
        args.source_config_id,
    )?;

    let mut status_mismatch_inventory = status_mismatches
        .into_iter()
        .map(|(key, candidate_occurrences)| StatusMismatchReport {
            ability_id: key.ability_id,
            owner_side: key.owner_side,
            direction: key.direction,
            effect_id: key.effect_id,
            source_entity_uuid: key.source_entity_uuid,
            source_config_id: key.source_config_id,
            source_relation: key.source_relation,
            stacks: key.stacks,
            level: key.level,
            part_id: key.part_id,
            count: key.count,
            candidate_occurrences,
        })
        .collect::<Vec<_>>();
    status_mismatch_inventory.sort_by(|left, right| {
        right
            .candidate_occurrences
            .cmp(&left.candidate_occurrences)
            .then_with(|| left.effect_id.cmp(&right.effect_id))
    });

    let pair_groups = groups
        .into_iter()
        .map(|(key, value)| PairGroupReport {
            critical: key.critical,
            lucky: key.lucky,
            attack_delta: key.attack_delta,
            pairs: value.pairs,
            paired_counterfactual_proof_eligible_pairs: value
                .paired_counterfactual_proof_eligible_pairs,
            source_multi_axis_pairs: value.source_multi_axis_pairs,
            packet_input_mismatch_pairs: value.packet_input_mismatch_pairs,
            exact_ratio_matches: value.exact_ratio_matches,
            within_one_ratio_matches: value.within_one_ratio_matches,
            mismatches: value.mismatches,
            unique_counterfactuals: value.unique_counterfactuals,
            unique_counterfactuals_matching_paired_inactive: value
                .unique_counterfactuals_matching_paired_inactive,
            ambiguous_counterfactuals: value.ambiguous_counterfactuals,
            impossible_counterfactuals: value.impossible_counterfactuals,
            examples: value.examples,
        })
        .collect();
    let diagnostic_pair_groups = diagnostic_groups
        .into_iter()
        .map(|(key, value)| PairGroupReport {
            critical: key.critical,
            lucky: key.lucky,
            attack_delta: key.attack_delta,
            pairs: value.pairs,
            paired_counterfactual_proof_eligible_pairs: value
                .paired_counterfactual_proof_eligible_pairs,
            source_multi_axis_pairs: value.source_multi_axis_pairs,
            packet_input_mismatch_pairs: value.packet_input_mismatch_pairs,
            exact_ratio_matches: value.exact_ratio_matches,
            within_one_ratio_matches: value.within_one_ratio_matches,
            mismatches: value.mismatches,
            unique_counterfactuals: value.unique_counterfactuals,
            unique_counterfactuals_matching_paired_inactive: value
                .unique_counterfactuals_matching_paired_inactive,
            ambiguous_counterfactuals: value.ambiguous_counterfactuals,
            impossible_counterfactuals: value.impossible_counterfactuals,
            examples: value.examples,
        })
        .collect();
    let diagnostics_by_action = single_event_counterfactual
        .diagnostics_by_action
        .iter()
        .map(|(key, value)| SingleEventDiagnosticReport {
            ability_id: key.ability_id,
            hit_event_id: key.hit_event_id,
            damage_id: key.damage_id.clone(),
            damage_script: key.damage_script.clone(),
            owner_stage: key.owner_stage,
            critical: key.critical,
            lucky: key.lucky,
            blocked: key.blocked,
            periodic: key.periodic,
            events: value.events,
            events_with_target_attribute_snapshot: value.events_with_target_attribute_snapshot,
            events_with_target_physical_defense: value.events_with_target_physical_defense,
            events_with_target_magical_defense: value.events_with_target_magical_defense,
            events_with_target_season_level: value.events_with_target_season_level,
            events_with_target_season_strength_or_weakness: value
                .events_with_target_season_strength_or_weakness,
            events_with_integer_factor_interval: value.events_with_integer_factor_interval,
            events_without_integer_factor_interval: value.events_without_integer_factor_interval,
            events_with_one_counterfactual_across_factor_interval: value
                .events_with_one_counterfactual_across_factor_interval,
            events_with_multiple_counterfactuals_across_factor_interval: value
                .events_with_multiple_counterfactuals_across_factor_interval,
            target_identities: value
                .target_identities
                .iter()
                .map(|((target_entity_uuid, monster_id), identity)| {
                    SingleEventTargetIdentityReport {
                        target_entity_uuid: *target_entity_uuid,
                        monster_id: *monster_id,
                        events: identity.events,
                        events_with_integer_factor_interval: identity
                            .events_with_integer_factor_interval,
                        events_without_integer_factor_interval: identity
                            .events_without_integer_factor_interval,
                    }
                })
                .collect(),
        })
        .collect();
    let target_attribute_coverage = single_event_counterfactual
        .target_attribute_coverage
        .iter()
        .map(|(attribute_id, value)| SingleEventTargetAttributeReport {
            attribute_id: *attribute_id,
            events_present: value.events_present,
            events_with_integer_factor_interval: value.events_with_integer_factor_interval,
            events_without_integer_factor_interval: value.events_without_integer_factor_interval,
        })
        .collect();
    let nonzero_damage_type_actions = single_event_counterfactual
        .nonzero_damage_type_actions
        .iter()
        .map(|(key, value)| NonzeroDamageTypeReport {
            ability_id: key.ability_id,
            hit_event_id: key.hit_event_id,
            damage_id: key.damage_id.clone(),
            damage_type: key.damage_type.clone(),
            damage_script: key.damage_script.clone(),
            owner_stage: key.owner_stage,
            critical: key.critical,
            lucky: key.lucky,
            blocked: key.blocked,
            periodic: key.periodic,
            events: value.events,
            observed_damage: value.observed_damage.to_string(),
        })
        .collect();
    let unsupported_damage_script_actions = single_event_counterfactual
        .unsupported_damage_script_actions
        .iter()
        .map(|(key, value)| UnsupportedDamageScriptReport {
            ability_id: key.ability_id,
            hit_event_id: key.hit_event_id,
            damage_id: key.damage_id.clone(),
            damage_type: key.damage_type.clone(),
            damage_script: key.damage_script.clone(),
            owner_stage: key.owner_stage,
            critical: key.critical,
            lucky: key.lucky,
            blocked: key.blocked,
            periodic: key.periodic,
            events: value.events,
            observed_damage: value.observed_damage.to_string(),
        })
        .collect();
    let shared_post_base_factor_diagnostic = shared_post_base_factor_report(
        &single_event_counterfactual.shared_post_base_factor_groups,
        args.example_limit,
        "offline_diagnostic_only_not_runtime_authority",
        "same session/run/source/target, packet damage flags and property, owner level/stage, positions, CurrentHP/MaxHP, complete source and target formula-attribute vectors, and complete source and target status states; ability and DamageAttr row are deliberately excluded so independent coefficients can constrain one shared downstream integer factor",
    );
    let position_relaxed_post_base_factor_diagnostic = shared_post_base_factor_report(
        &single_event_counterfactual.position_relaxed_post_base_factor_groups,
        args.example_limit,
        "offline_position_relaxed_diagnostic_only_not_runtime_authority",
        "same exact state as the full shared-factor diagnostic except packet world position and hit-part positions are excluded; HP, complete attributes, complete statuses, all damage flags, property, owner level/stage, skill-effect component identity, hit-part IDs, and damage-weight bits remain exact. Compatible intersections are position-invariance candidates, while every disjoint intersection is retained",
    );
    let position_current_hp_relaxed_post_base_factor_diagnostic = shared_post_base_factor_report(
        &single_event_counterfactual.position_current_hp_relaxed_post_base_factor_groups,
        args.example_limit,
        "offline_position_and_current_hp_relaxed_diagnostic_only_not_runtime_authority",
        "same exact state as the position-relaxed scope except packet CurrentHP 11310 is excluded from source and target vectors and explicit CurrentHP fields; MaxHP, all remaining attributes, complete statuses, flags, property, owner level/stage, and skill-effect component identity remain exact. HP-dependent abilities are not eligible for promotion from this diagnostic without separate ability/hit formula proof",
    );
    let position_current_hp_component_relaxed_post_base_factor_diagnostic =
        shared_post_base_factor_report(
            &single_event_counterfactual
                .position_current_hp_component_relaxed_post_base_factor_groups,
            args.example_limit,
            "offline_position_current_hp_and_component_relaxed_diagnostic_only_not_runtime_authority",
            "same exact state as the position-and-CurrentHP-relaxed scope except skill-effect UUID/group/component identity is excluded; this identifies whether component serialization prevents repeated-state controls. It cannot establish cross-component formula equivalence and never drives runtime attribution",
        );
    let action_position_component_relaxed_post_base_factor_diagnostic =
        shared_post_base_factor_report(
            &single_event_counterfactual.action_position_component_relaxed_post_base_factor_groups,
            args.example_limit,
            "offline_action_preserved_position_and_component_relaxed_diagnostic_only_not_runtime_authority",
            "same session/run/source/target, exact ability/hit/DamageAttr row, packet CurrentHP/MaxHP, complete attributes and statuses, flags, property, owner level/stage, hit-part IDs, and damage-weight bits; only packet position and skill-effect UUID/group/component serialization identity are excluded. Every disjoint interval disproves one shared post-base factor even within that exact action state",
        );
    let action_position_current_hp_component_relaxed_post_base_factor_diagnostic =
        shared_post_base_factor_report(
            &single_event_counterfactual
                .action_position_current_hp_component_relaxed_post_base_factor_groups,
            args.example_limit,
            "offline_action_preserved_position_current_hp_and_component_relaxed_diagnostic_only_not_runtime_authority",
            "same exact action-preserved state as the position-and-component-relaxed scope except packet CurrentHP 11310 and explicit CurrentHP are excluded; MaxHP remains exact. This scope can diagnose repeated action rows, but HP-dependent actions remain ineligible for promotion without separate proof and every disjoint interval is retained",
        );
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-external-attack-damage-proof",
        policy: AuditPolicy {
            runtime_use: "offline_research_only_never_loaded_by_capture_or_live_meter",
            analysis_scope: if args.pair_proof_only {
                "bounded pair proof only: status-and-target-controlled pair candidates, explicit strict paired-counterfactual eligibility counts, rejection inventories, and complete offensive vectors are evaluated; independent archetype, live-vector, status-uncontrolled offensive-vector, and broad single-event diagnostic reports are intentionally empty and must never be interpreted as passing evidence"
            } else {
                "full external Attack research surface"
            },
            occurrence_authority: "current-build canonical packet events",
            provider_control: "the selected status must have one exact external provider entity on the damage actor; provider allegiance or actor kind is not inferred by this analyzer",
            pair_scope: "same session, run, source, direct source, target, ability, hit identity, damage source/type, hit flags, and core packet dimensions within the configured gap; complete source formula-attribute deltas are retained per example and a pair is labeled Attack-only-controlled only when no packet-observed source axis other than final Attack changes; HP, position, skill-component identity, optional flag, and target-identity deltas retained on the samples are separately enumerated and must be empty before the paired comparison is called packet-input-controlled",
            calculation_state_timing: "source and target attributes and statuses are reconstructed at wire-message start; notifications serialized earlier in the same delta message are not treated as pre-damage state",
            status_control: "all source and target status identities, providers, stacks, levels, parts, and counts other than the selected effect must match exactly; transient instance IDs are excluded",
            diagnostic_status_control: "user-specified diagnostic status IDs may be excluded only from the separately labeled companion-normalized comparison; the strict proof and full mismatch inventory retain every status",
            attack_control: "both packet-current final Attack values must be known and the active sample must differ from the inactive sample",
            target_attribute_control: "strict and companion-normalized pairs require the same target entity and a known, non-empty, exactly identical packet-current target combat vector; an omitted static defense field is not invented or required because the paired observation holds target identity and every packet-observed target field constant; remote-player packets known not to arrive remain unobserved rather than zero and never gate ordinary damage retention",
            counterfactual_control: "a transfer is exact only when the full coefficient interval compatible with the observed active integer damage yields one inactive integer result and paired_counterfactual_proof_eligible_pairs is nonzero; separately, a status-and-target-controlled diagnostic pair may expose a research-only exact conserved Attack-stage share when its observed active/inactive final Attack values resolve one exact current-build standard damage row, but any other source-axis co-transition, packet-input mismatch, or disagreement with paired inactive damage remains explicit and blocks total-effect promotion",
            live_vector_guard: "an ability cannot establish a live attribute formula when identical damage and exact packet/status context persist across different packet-current source vectors; such events remain visible as snapshot-semantic conflicts",
            derived_attribute_alias_control: "AttrExtDamInc 11840 equals floor(AttrVersatilityPct 11950 * 35 / 100) in this build; candidate formulas may test either representation but must never multiply both",
            replay_lifecycle_completeness: "the audit aborts when none of the supplied replay inputs contains the selected status lifecycle; a compact replay that retained damage but omitted statuses is invalid proof input",
            unresolved_evidence_is_hidden: false,
        },
        selected_effect_id: args.effect_id,
        selected_source_config_id: args.source_config_id,
        selected_source_config_is_absent: args.source_config_id == 0,
        final_attack_attribute_id: args.attack_attribute_id,
        damage_surface_identity: damage_surface
            .as_ref()
            .map(|surface| surface.identity.clone()),
        attack_provider_delta: args
            .attack_provider_delta
            .map(|delta| AttackProviderDeltaReport {
                component: delta.component(),
                raw_delta: delta.raw_delta(),
                primary_attribute_id: delta.primary_attribute_id(),
                attack_add_numerator: delta.attack_add_ratio().map(|value| value.0),
                attack_add_denominator: delta.attack_add_ratio().map(|value| value.1),
            }),
        provider_external_damage_raw_delta: args.provider_external_damage_raw_delta,
        provider_property_damage_attribute_id: args.provider_property_damage_attribute_id,
        provider_property_damage_raw_delta: args.provider_property_damage_raw_delta,
        required_damage_property: args.required_damage_property,
        required_provider_status: args.required_provider_status,
        source_entity_uuid_filter: args.source_entity_uuid,
        transition_seed_selection: transition_seed_filter.as_ref().map(|filter| {
            TransitionSeedSelectionReport {
                source: filter.source.clone(),
                window_micros_before_and_after: filter.window_micros,
                retained_transition_seeds: filter.seed_count,
                selection_policy: "damage samples and target-state retention are restricted to the selected recipient in the same session and run within the declared symmetric window; whole-run status lifecycle replay remains required; window proximity is diagnostic selection and never formula authority",
                formula_authority: false,
            }
        }),
        effective_stat_window_selection: effective_stat_window_filter.as_ref().map(|filter| {
            EffectiveStatWindowSelectionReport {
                source: filter.source.clone(),
                game_build: filter.game_build.clone(),
                exact_lifecycle_windows: filter.exact_lifecycle_windows,
                candidate_status_window_damage_actions: filter
                    .candidate_status_window_damage_actions,
                effective_stat_window_damage_actions: filter
                    .effective_stat_window_damage_actions,
                excluded_before_attribute_activation: filter
                    .excluded_before_attribute_activation,
                excluded_at_or_after_attribute_deactivation: filter
                    .excluded_at_or_after_attribute_deactivation,
                selection_policy: "a damage row is externally affected only while both the exact selected-status lifecycle and its uniquely joined recipient attribute window are active; rows between status application and delayed activation or at/after deactivation are ambiguous, never zero-filled or treated as unaffected controls",
                formula_authority: false,
                provider_credit_allowed: false,
            }
        }),
        pair_proof_only: args.pair_proof_only,
        diagnostic_ignored_status_ids: args.diagnostic_ignored_status_ids.iter().copied().collect(),
        max_pair_gap_micros: args.max_gap_micros,
        sessions,
        status_mismatch_inventory,
        pair_groups,
        formula: FormulaReport {
            hypothesis: "diagnostic hypothesis: with status and target context held constant, final damage is linear in final Attack before one integer floor; source_formula_attribute_deltas and attack_only_source_vector_controlled determine whether other packet-observed source axes also changed",
            paired_prediction: "predicted active = floor(inactive damage * active Attack / inactive Attack)",
            exact_counterfactual_interval: "inactive damage belongs to [floor(active damage*inactive Attack/active Attack), ceil((active damage+1)*inactive Attack/active Attack)-1]",
            pairs: formula.pairs,
            exact_ratio_matches: formula.exact_ratio_matches,
            within_one_ratio_matches: formula.within_one_ratio_matches,
            mismatches: formula.mismatches,
            unique_counterfactuals: formula.unique_counterfactuals,
            unique_counterfactuals_matching_paired_inactive: formula
                .unique_counterfactuals_matching_paired_inactive,
            ambiguous_counterfactuals: formula.ambiguous_counterfactuals,
            impossible_counterfactuals: formula.impossible_counterfactuals,
            maximum_absolute_residual: formula.maximum_absolute_residual,
            residual_examples: formula.residuals.into_iter().collect(),
        },
        status_controlled_offensive_vector_formulas: offensive_vector_reports(
            strict_vector_formulas,
            "strict_status_controlled_offline_evidence_not_runtime_authority_until_one_integer_stage_model_is_exact_and_conservative",
        ),
        companion_normalized_offensive_vector_formulas: offensive_vector_reports(
            companion_normalized_vector_formulas,
            "diagnostic_only_selected_companion_statuses_excluded_from_comparison_not_runtime_authority",
        ),
        status_uncontrolled_diagnostic_pair_groups: diagnostic_pair_groups,
        status_uncontrolled_diagnostic_formula: FormulaReport {
            hypothesis: "diagnostic only: final damage is linear in final Attack while other statuses are not controlled",
            paired_prediction: "predicted active = floor(inactive damage * active Attack / inactive Attack)",
            exact_counterfactual_interval: "inactive damage belongs to [floor(active damage*inactive Attack/active Attack), ceil((active damage+1)*inactive Attack/active Attack)-1]",
            pairs: diagnostic_formula.pairs,
            exact_ratio_matches: diagnostic_formula.exact_ratio_matches,
            within_one_ratio_matches: diagnostic_formula.within_one_ratio_matches,
            mismatches: diagnostic_formula.mismatches,
            unique_counterfactuals: diagnostic_formula.unique_counterfactuals,
            unique_counterfactuals_matching_paired_inactive: diagnostic_formula
                .unique_counterfactuals_matching_paired_inactive,
            ambiguous_counterfactuals: diagnostic_formula.ambiguous_counterfactuals,
            impossible_counterfactuals: diagnostic_formula.impossible_counterfactuals,
            maximum_absolute_residual: diagnostic_formula.maximum_absolute_residual,
            residual_examples: diagnostic_formula.residuals.into_iter().collect(),
        },
        status_uncontrolled_offensive_vector_formulas: offensive_vector_reports(
            diagnostic_vector_formulas,
            "diagnostic_only_not_runtime_authority_until_one_integer_stage_model_is_exact_and_conservative",
        ),
        live_vector_stability: LiveVectorStabilityReport {
            authority: "diagnostic_guard_not_a_damage_formula",
            interpretation: "same damage under the same packet and status context despite a changed packet-current source vector proves that the ability may use snapshotted or hidden state; do not use those events as live-vector formula authority",
            conflicting_pairs: live_vector_stability.conflicting_pairs,
            examples: live_vector_stability.examples,
        },
        damage_time_attack_state_proof: damage_time_attack_state_report(
            &args,
            damage_time_attack_states,
            damage_surface.as_ref(),
        ),
        direct_observed_counterfactuals: direct_observed_counterfactual_report(
            direct_observed_contexts,
            args.example_limit,
        ),
        archetype_observed_counterfactuals: archetype_observed_counterfactual_report(
            archetype_observed_contexts,
            args.example_limit,
        ),
        single_event_damage_attr_counterfactual: SingleEventCounterfactualReport {
            authority: "offline_candidate_only_not_runtime_authority_until_the_post_base_integer_pipeline_and_cast_snapshot_timing_are_proven",
            model: "for DamageAttr rows whose DamageScript is exactly Attack or MAttack, select the current-build coefficient by packet owner_stage, select the fixed term by packet owner_level, derive the output interval compatible with observed damage after removing only the selected provider's explicitly selected packet-proven Attack-family component, and accept only one integer counterfactual; base-add and raw-percent inputs are distinct and never inferred from the effect ID; the integer DamageType enum is retained as classification metadata and does not select the calculation function; other DamageScript families remain explicit coverage gaps until their formulas are proven",
            coefficient_selection: "current-build schema-consistent standard-script candidate: a one-value PVEDamageRadio vector is stage-invariant; for multi-value vectors packet owner_stage selects the zero-based entry and an omitted optional protobuf scalar is treated as semantic stage zero; DamageType is read as the aligned integer at row offset 16 and is never interpreted through the string pool; out-of-range multi-value stages and nonstandard scripts remain missing evidence rather than guesses, and the multi-value selection remains diagnostic until replay proves it across controlled stage changes",
            fixed_parameter_selection: "the packet owner-level one-based value is used when the current row has a level vector; empty vectors contribute zero",
            fixed_parameter_placement: "floor(Attack*PVEDamageRadio/10000)+PVEFixedParameter, matching the current client formula text; the disproven fixed-inside-ratio branch is not retained",
            external_active_damage_events: single_event_counterfactual
                .external_active_damage_events,
            attack_family_reversal_policy: "an occurrence-scoped final-Attack delta is admissible only with complete transition seeds and requires the packet-current final Attack; component-route reversals instead require every event-time component of the selected final Attack family and, for a derived-primary route, every primary-family component to be packet-observed. Absent attributes are unresolved rather than zero-filled, active component routes must replay exactly through the declared integer order, and every failure is retained in exact_conserved_share_coverage_gaps",
            events_with_all_required_attack_family_components: single_event_counterfactual
                .events_with_all_required_attack_family_components,
            attack_family_component_coverage: single_event_counterfactual
                .attack_family_component_coverage
                .iter()
                .map(|(&attribute_id, coverage)| AttackFamilyComponentCoverageReport {
                    attribute_id,
                    events_present: coverage.events_present,
                    events_missing: coverage.events_missing,
                })
                .collect(),
            events_with_exact_attack_family_reversal: single_event_counterfactual
                .events_with_exact_attack_family_reversal,
            events_without_exact_attack_family_reversal: single_event_counterfactual
                .external_active_damage_events
                .saturating_sub(single_event_counterfactual.events_with_exact_attack_family_reversal),
            events_with_ability_hit_identity: single_event_counterfactual
                .events_with_ability_hit_identity,
            hit_event_identity: "a present packet hit_event_id is retained exactly; an omitted optional scalar is semantic zero only when the current-build DamageAttr surface contains that ability:0 key, and missing or ambiguous rows remain explicit coverage gaps",
            events_with_wire_hit_event_id: single_event_counterfactual
                .events_with_wire_hit_event_id,
            events_using_semantic_zero_for_omitted_hit_event_id: single_event_counterfactual
                .events_using_semantic_zero_for_omitted_hit_event_id,
            events_with_unique_damage_row: single_event_counterfactual
                .events_with_unique_damage_row,
            events_with_matching_damage_script: single_event_counterfactual
                .events_with_matching_damage_script,
            events_with_nonzero_damage_type: single_event_counterfactual
                .events_with_nonzero_damage_type,
            events_with_unsupported_damage_script: single_event_counterfactual
                .events_with_unsupported_damage_script,
            events_with_exact_stage_coefficient: single_event_counterfactual
                .events_with_exact_stage_coefficient,
            events_missing_stage_coefficient: single_event_counterfactual
                .events_missing_stage_coefficient,
            events_with_base_candidates: single_event_counterfactual.events_with_base_candidates,
            required_provider_status_policy: "when configured active, the fixed provider vector is admitted only while the damage recipient has the exact status identity from the same external provider; when configured absent, it is admitted only after a captured run entry and while that exact provider status is absent; every contrary or pre-entry state is retained as a coverage gap",
            events_matching_required_provider_status: single_event_counterfactual
                .events_matching_required_provider_status,
            events_rejected_by_required_provider_status: single_event_counterfactual
                .events_rejected_by_required_provider_status,
            exact_conserved_attack_stage_share_model: "for each uniquely resolved standard-Attack DamageAttr row, compute active_body=floor(active_Attack*coefficient/10000)+fixed and provider_body=active_body-(floor(provider_removed_Attack*coefficient/10000)+fixed), then retain observed_damage*provider_body/active_body as an exact reduced rational transfer; this is conserved accounting through unresolved later stages, not a guessed integer game counterfactual",
            events_with_exact_conserved_attack_stage_share: single_event_counterfactual
                .events_with_exact_conserved_attack_stage_share,
            exact_conserved_share_observed_damage: single_event_counterfactual
                .exact_conserved_share_observed_damage
                .to_string(),
            exact_conserved_share_buckets: exact_conserved_share_bucket_reports(
                &single_event_counterfactual.exact_conserved_share_buckets,
            ),
            exact_conserved_share_coverage_gaps: single_event_coverage_gap_reports(
                &single_event_counterfactual.coverage_gaps,
            ),
            exact_conserved_attack_external_composite_model: "candidate accounting projection only: remove the packet-proven Attack-family delta and configured packet-proven External Damage raw delta together from the same uniquely resolved standard-Attack body, then retain observed_damage*(active_composite-provider_removed_composite)/active_composite as one reduced rational share; this prevents double-counting their multiplicative cross-term, but remains non-authoritative until External Damage stage applicability, placement, order, and snapshot semantics are proven for the event",
            configured_provider_external_damage_raw_delta: args.provider_external_damage_raw_delta,
            events_with_exact_conserved_attack_external_composite_share:
                single_event_counterfactual
                    .events_with_exact_conserved_attack_external_composite_share,
            events_without_exact_conserved_attack_external_composite_share:
                single_event_counterfactual
                    .events_without_exact_conserved_attack_external_composite_share,
            exact_conserved_attack_external_composite_observed_damage: single_event_counterfactual
                .exact_conserved_attack_external_composite_observed_damage
                .to_string(),
            exact_conserved_attack_external_composite_buckets: exact_conserved_share_bucket_reports(
                &single_event_counterfactual.exact_conserved_attack_external_composite_buckets,
            ),
            exact_conserved_attack_external_property_composite_model: "candidate accounting projection only: for packet events whose exact damage property matches the configured property, remove the packet-proven Attack-family delta, External Damage raw delta, and property-specific Damage raw delta together from one uniquely resolved standard-Attack body, then retain observed_damage*(active_composite-provider_removed_composite)/active_composite as one reduced rational share; this prevents double-counting every cross-term among the three provider stages, but remains non-authoritative until property-stage placement, order, and calculation-snapshot semantics are proven",
            configured_provider_property_damage_attribute_id: args
                .provider_property_damage_attribute_id,
            configured_provider_property_damage_raw_delta: args.provider_property_damage_raw_delta,
            configured_required_damage_property: args.required_damage_property,
            events_matching_required_damage_property: single_event_counterfactual
                .events_matching_required_damage_property,
            events_rejected_by_required_damage_property: single_event_counterfactual
                .events_rejected_by_required_damage_property,
            events_with_exact_conserved_attack_external_property_composite_share:
                single_event_counterfactual
                    .events_with_exact_conserved_attack_external_property_composite_share,
            events_without_exact_conserved_attack_external_property_composite_share:
                single_event_counterfactual
                    .events_without_exact_conserved_attack_external_property_composite_share,
            exact_conserved_attack_external_property_composite_observed_damage:
                single_event_counterfactual
                    .exact_conserved_attack_external_property_composite_observed_damage
                    .to_string(),
            exact_conserved_attack_external_property_composite_buckets:
                exact_conserved_share_bucket_reports(
                    &single_event_counterfactual
                        .exact_conserved_attack_external_property_composite_buckets,
                ),
            events_with_packet_normal_value: single_event_counterfactual
                .events_with_packet_normal_value,
            events_where_packet_normal_value_matches_amount: single_event_counterfactual
                .events_where_packet_normal_value_matches_amount,
            integer_post_base_factor_model: "diagnostic constraint only: if observed damage = floor(current-build DamageAttr base * one non-negative integer basis-point factor / 10000), invert the observed output to the complete compatible integer-factor interval and retain a counterfactual only when every compatible factor produces the same integer result",
            events_with_integer_post_base_factor_interval: single_event_counterfactual
                .events_with_integer_post_base_factor_interval,
            events_without_integer_post_base_factor_interval: single_event_counterfactual
                .events_without_integer_post_base_factor_interval,
            events_with_unique_integer_post_base_factor: single_event_counterfactual
                .events_with_unique_integer_post_base_factor,
            events_with_one_counterfactual_across_integer_factor_interval:
                single_event_counterfactual
                    .events_with_one_counterfactual_across_integer_factor_interval,
            events_with_multiple_counterfactuals_across_integer_factor_interval:
                single_event_counterfactual
                    .events_with_multiple_counterfactuals_across_integer_factor_interval,
            events_with_one_exact_counterfactual_across_all_candidates: single_event_counterfactual
                .events_with_one_exact_counterfactual_across_all_candidates,
            events_with_ambiguous_counterfactual: single_event_counterfactual
                .events_with_ambiguous_counterfactual,
            events_with_invalid_counterfactual: single_event_counterfactual
                .events_with_invalid_counterfactual,
            shared_post_base_factor_diagnostic,
            position_relaxed_post_base_factor_diagnostic,
            position_current_hp_relaxed_post_base_factor_diagnostic,
            position_current_hp_component_relaxed_post_base_factor_diagnostic,
            action_position_component_relaxed_post_base_factor_diagnostic,
            action_position_current_hp_component_relaxed_post_base_factor_diagnostic,
            observed_damage: single_event_counterfactual.observed_damage.to_string(),
            exact_counterfactual_damage: single_event_counterfactual
                .exact_counterfactual_damage
                .to_string(),
            exact_provider_marginal: single_event_counterfactual
                .exact_provider_marginal
                .to_string(),
            nonzero_damage_type_actions,
            unsupported_damage_script_actions,
            diagnostics_by_action,
            target_attribute_coverage,
            examples: single_event_counterfactual.examples,
        },
    };
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn exact_conserved_share_bucket_reports(
    buckets: &BTreeMap<i128, ExactConservedShareBucketAccumulator>,
) -> Vec<ExactConservedShareBucketReport> {
    buckets
        .iter()
        .map(|(denominator, bucket)| {
            let observed_numerator = bucket.observed_damage.saturating_mul(*denominator);
            let recipient_numerator = observed_numerator.saturating_sub(bucket.provider_numerator);
            ExactConservedShareBucketReport {
                denominator: denominator.to_string(),
                events: bucket.events,
                observed_damage: bucket.observed_damage.to_string(),
                provider_numerator: bucket.provider_numerator.to_string(),
                recipient_numerator: recipient_numerator.to_string(),
                observed_numerator: observed_numerator.to_string(),
                conservation_identity_holds: bucket
                    .provider_numerator
                    .saturating_add(recipient_numerator)
                    == observed_numerator,
            }
        })
        .collect()
}

fn single_event_coverage_gap_reports(
    gaps: &BTreeMap<SingleEventCoverageGapKey, SingleEventCoverageGapAccumulator>,
) -> Vec<SingleEventCoverageGapReport> {
    gaps.iter()
        .map(|(key, gap)| SingleEventCoverageGapReport {
            reason: key.reason,
            ability_id: key.ability_id,
            hit_event_id: key.hit_event_id,
            owner_level: key.owner_level,
            owner_stage: key.owner_stage,
            property: key.property,
            critical: key.critical,
            lucky: key.lucky,
            candidate_rows: key.candidate_rows.clone(),
            events: gap.events,
            observed_damage: gap.observed_damage.to_string(),
            examples: gap.examples.clone(),
        })
        .collect()
}

fn selected_source_entities(
    path: &Path,
    source_entity_uuid: i64,
    transition_seed_filter: Option<&TransitionSeedFilter>,
) -> Result<BTreeSet<i64>, Box<dyn std::error::Error>> {
    let mut retained = BTreeSet::from([source_entity_uuid]);
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut run_ordinal = 0_u32;
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
            TimelineEventKind::Damage(damage)
                if damage.source.entity_uuid.0 == source_entity_uuid
                    && transition_seed_filter.is_none_or(|filter| {
                        filter.matches(
                            &envelope.session_id,
                            run_ordinal,
                            source_entity_uuid,
                            envelope.time.observed_micros,
                        )
                    }) =>
            {
                retained.insert(damage.target.entity_uuid.0);
            }
            _ => {}
        }
    }
    Ok(retained)
}

fn read_session(
    path: &Path,
    args: &Arguments,
    groups: &mut BTreeMap<PairGroupKey, PairGroupAccumulator>,
    formula: &mut FormulaAccumulator,
    status_mismatches: &mut BTreeMap<StatusMismatchKey, u64>,
    diagnostic_groups: &mut BTreeMap<PairGroupKey, PairGroupAccumulator>,
    diagnostic_formula: &mut FormulaAccumulator,
    strict_vector_formulas: &mut BTreeMap<&'static str, OffensiveVectorFormulaAccumulator>,
    companion_normalized_vector_formulas: &mut BTreeMap<
        &'static str,
        OffensiveVectorFormulaAccumulator,
    >,
    diagnostic_vector_formulas: &mut BTreeMap<&'static str, OffensiveVectorFormulaAccumulator>,
    live_vector_stability: &mut LiveVectorStabilityAccumulator,
    damage_time_attack_states: &mut BTreeMap<
        DamageTimeAttackStateContext,
        DamageTimeAttackStateAccumulator,
    >,
    direct_observed_contexts: &mut BTreeMap<
        DirectObservedContext,
        DirectObservedContextAccumulator,
    >,
    archetype_observed_contexts: &mut BTreeMap<
        ArchetypeObservedContext,
        ArchetypeObservedContextAccumulator,
    >,
    damage_surface: Option<&DamageSurface>,
    single_event_counterfactual: &mut SingleEventCounterfactualAccumulator,
    transition_seed_filter: Option<&TransitionSeedFilter>,
    effective_stat_window_filter: Option<&EffectiveStatWindowFilter>,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    let retained_entity_uuids = args
        .source_entity_uuid
        .map(|source_entity_uuid| {
            selected_source_entities(path, source_entity_uuid, transition_seed_filter)
        })
        .transpose()?;
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = None::<String>;
    let mut current_run_ordinal = 0_u32;
    let mut maximum_run_ordinal = 0_u32;
    let mut damage_events = 0_u64;
    let mut damage_events_with_attack = 0_u64;
    let mut selected_status_events = 0_u64;
    let mut external_active_damage_events = 0_u64;
    let mut inactive_damage_events = 0_u64;
    let mut ambiguous_or_self_active_damage_events = 0_u64;
    let mut effect_transition_candidates_before_controls = 0_u64;
    let mut strict_pairs = 0_u64;
    let mut rejected_status_mismatch = 0_u64;
    let mut rejected_target_attribute_mismatch = 0_u64;
    let mut rejected_target_attribute_unknown = 0_u64;
    let mut companion_normalized_pairs = 0_u64;
    let mut rejected_same_attack = 0_u64;
    let mut ability_transition_candidates =
        BTreeMap::<AbilityTransitionCandidateKey, AbilityTransitionCandidateAccumulator>::new();
    let mut observed_abilities_by_source = BTreeMap::<i64, BTreeSet<i64>>::new();
    let mut externally_affected_actor_uuids = BTreeSet::<i64>::new();
    let mut observed_status_routes_by_target = BTreeMap::<
        i64,
        BTreeMap<ActorObservedStatusRouteKey, ActorObservedStatusRouteAccumulator>,
    >::new();
    let mut attributes = HashMap::<(u32, i64), Arc<BTreeMap<i32, i64>>>::new();
    let mut health_attributes = HashMap::<(u32, i64), Arc<BTreeMap<i32, i64>>>::new();
    let mut statuses = HashMap::<(u32, i64), StatusTracker>::new();
    let mut monster_ids = HashMap::<(u32, i64), i64>::new();
    let mut actor_levels = HashMap::<(u32, i64), u32>::new();
    let mut active_wire_message = None::<WireMessageKey>;
    let mut attributes_at_wire_message_start =
        HashMap::<(u32, i64), Arc<BTreeMap<i32, i64>>>::new();
    let mut health_attributes_at_wire_message_start =
        HashMap::<(u32, i64), Arc<BTreeMap<i32, i64>>>::new();
    let mut statuses_at_wire_message_start = HashMap::<(u32, i64), StatusTracker>::new();
    let mut recent = BTreeMap::<DamageContext, VecDeque<DamageSample>>::new();
    let mut recent_expirations = VecDeque::<(u64, DamageContext)>::new();

    while let Some(envelope) = reader.next_event()? {
        let wire_message = wire_message_key(&envelope.provenance.source);
        if wire_message != active_wire_message {
            active_wire_message = wire_message;
            attributes_at_wire_message_start.clear();
            health_attributes_at_wire_message_start.clear();
            statuses_at_wire_message_start.clear();
        }
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
                    recent.clear();
                    recent_expirations.clear();
                }
                RunState::Started if current_run_ordinal == 0 => {
                    current_run_ordinal = 1;
                    maximum_run_ordinal = 1;
                    recent.clear();
                    recent_expirations.clear();
                }
                _ => {}
            },
            TimelineEventKind::Actor(actor) => {
                let actor_key = (current_run_ordinal, actor.actor.entity_uuid.0);
                if retained_entity_uuids
                    .as_ref()
                    .is_some_and(|retained| !retained.contains(&actor_key.1))
                {
                    continue;
                }
                if let Some(monster_id) = actor.monster_id {
                    monster_ids.insert(actor_key, monster_id.0);
                }
                if let Some(level) = actor.level {
                    actor_levels.insert(actor_key, level);
                }
            }
            TimelineEventKind::EntityAttributes(event) => {
                let actor_key = (current_run_ordinal, event.actor.entity_uuid.0);
                if retained_entity_uuids
                    .as_ref()
                    .is_some_and(|retained| !retained.contains(&actor_key.1))
                {
                    continue;
                }
                if active_wire_message.is_some() {
                    attributes_at_wire_message_start
                        .entry(actor_key)
                        .or_insert_with(|| {
                            attributes
                                .get(&actor_key)
                                .cloned()
                                .unwrap_or_else(|| Arc::new(BTreeMap::new()))
                        });
                    health_attributes_at_wire_message_start
                        .entry(actor_key)
                        .or_insert_with(|| {
                            health_attributes
                                .get(&actor_key)
                                .cloned()
                                .unwrap_or_else(|| Arc::new(BTreeMap::new()))
                        });
                }
                let decoded_formula_attributes = event
                    .attributes
                    .iter()
                    .filter(|attribute| {
                        attribute.attribute_id == args.attack_attribute_id
                            || is_formula_vector_attribute(attribute.attribute_id)
                    })
                    .filter_map(|attribute| {
                        decode_attribute(attribute).map(|value| (attribute.attribute_id, value))
                    })
                    .collect::<Vec<_>>();
                let decoded_health_attributes = event
                    .attributes
                    .iter()
                    .filter(|attribute| {
                        matches!(
                            attribute.attribute_id,
                            CURRENT_HP_ATTRIBUTE_ID | MAX_HP_ATTRIBUTE_ID
                        )
                    })
                    .filter_map(|attribute| {
                        decode_attribute(attribute).map(|value| (attribute.attribute_id, value))
                    })
                    .collect::<Vec<_>>();
                if event.update_kind != EntityAttributeUpdateKind::Snapshot
                    && decoded_formula_attributes.is_empty()
                    && decoded_health_attributes.is_empty()
                {
                    continue;
                }
                let snapshot = attributes.entry(actor_key).or_default();
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    Arc::make_mut(snapshot).clear();
                }
                for (attribute_id, value) in decoded_formula_attributes {
                    Arc::make_mut(snapshot).insert(attribute_id, value);
                }
                let health_snapshot = health_attributes.entry(actor_key).or_default();
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    Arc::make_mut(health_snapshot).clear();
                }
                for (attribute_id, value) in decoded_health_attributes {
                    Arc::make_mut(health_snapshot).insert(attribute_id, value);
                }
            }
            TimelineEventKind::Status(status) => {
                let target_key = (current_run_ordinal, status.target.entity_uuid.0);
                if retained_entity_uuids
                    .as_ref()
                    .is_some_and(|retained| !retained.contains(&target_key.1))
                {
                    continue;
                }
                if active_wire_message.is_some() {
                    statuses_at_wire_message_start
                        .entry(target_key)
                        .or_insert_with(|| statuses.get(&target_key).cloned().unwrap_or_default());
                }
                let route = observed_status_routes_by_target
                    .entry(status.target.entity_uuid.0)
                    .or_default()
                    .entry(ActorObservedStatusRouteKey {
                        effect_id: status.effect.0,
                        source_entity_uuid: status.source.map(|value| value.entity_uuid.0),
                        source_config_id: status.origin.map(|origin| origin.source_config_id),
                    })
                    .or_default();
                route.lifecycle_events = route.lifecycle_events.saturating_add(1);
                let selected_status_key = StatusKey {
                    effect_id: status.effect.0,
                    instance_id: status.instance_id.map(|value| value.0),
                    source_entity_uuid: status.source.map(|value| value.entity_uuid.0),
                    source_config_id: status.origin.map(|origin| origin.source_config_id),
                };
                if selected_status_key_matches(
                    &selected_status_key,
                    args.effect_id,
                    args.source_config_id,
                ) {
                    selected_status_events = selected_status_events.saturating_add(1);
                }
                statuses.entry(target_key).or_default().observe(
                    selected_status_key,
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
                if args
                    .source_entity_uuid
                    .is_some_and(|selected| selected != source_uuid)
                {
                    continue;
                }
                if transition_seed_filter.is_some_and(|filter| {
                    !filter.matches(
                        &envelope.session_id,
                        current_run_ordinal,
                        source_uuid,
                        envelope.time.observed_micros,
                    )
                }) {
                    continue;
                }
                let target_uuid = damage.target.entity_uuid.0;
                let source_key = (current_run_ordinal, source_uuid);
                let target_key = (current_run_ordinal, target_uuid);
                if let Some(ability_id) = damage.ability.map(|value| value.0) {
                    observed_abilities_by_source
                        .entry(source_uuid)
                        .or_default()
                        .insert(ability_id);
                }
                let Some(source_attributes) = attributes_at_wire_message_start
                    .get(&source_key)
                    .or_else(|| attributes.get(&source_key))
                else {
                    continue;
                };
                let Some(&attack) = source_attributes.get(&args.attack_attribute_id) else {
                    continue;
                };
                if attack <= 0 || damage.amount <= 0 {
                    continue;
                }
                damage_events_with_attack = damage_events_with_attack.saturating_add(1);
                let source_tracker = statuses_at_wire_message_start
                    .get(&source_key)
                    .or_else(|| statuses.get(&source_key))
                    .unwrap_or(&EMPTY_STATUS_TRACKER);
                let target_tracker = statuses_at_wire_message_start
                    .get(&target_key)
                    .or_else(|| statuses.get(&target_key))
                    .unwrap_or(&EMPTY_STATUS_TRACKER);
                let effect_presence = gate_effect_presence_by_effective_stat_window(
                    source_tracker.selected_presence(
                        args.effect_id,
                        args.source_config_id,
                        source_uuid,
                    ),
                    effective_stat_window_filter,
                    &envelope.session_id,
                    current_run_ordinal,
                    source_uuid,
                    envelope.sequence,
                );
                match effect_presence {
                    EffectPresence::External(_) => {
                        external_active_damage_events =
                            external_active_damage_events.saturating_add(1);
                        externally_affected_actor_uuids.insert(source_uuid);
                        if let Some(routes) = observed_status_routes_by_target.get_mut(&source_uuid)
                        {
                            for key in source_tracker.active.keys() {
                                let route = routes
                                    .entry(ActorObservedStatusRouteKey {
                                        effect_id: key.effect_id,
                                        source_entity_uuid: key.source_entity_uuid,
                                        source_config_id: key.source_config_id,
                                    })
                                    .or_default();
                                route.external_active_damage_events =
                                    route.external_active_damage_events.saturating_add(1);
                            }
                        }
                    }
                    EffectPresence::Inactive => {
                        inactive_damage_events = inactive_damage_events.saturating_add(1)
                    }
                    EffectPresence::SelfOwned | EffectPresence::Ambiguous => {
                        ambiguous_or_self_active_damage_events =
                            ambiguous_or_self_active_damage_events.saturating_add(1)
                    }
                }
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
                    normal_value: damage.packet.normal_value,
                    lucky_value: damage.packet.lucky_value,
                    actual_value: damage.actual_amount,
                    hp_loss: damage.hp_loss,
                    shield_loss: damage.shield_loss,
                    causes_lucky: damage.flags.causes_lucky,
                    missed: damage.packet.missed,
                    reported_critical: damage.packet.reported_critical,
                    type_flags: damage.packet.type_flags,
                    skill_effect_uuid: damage.packet.skill_effect_uuid,
                    skill_effect_total_damage: damage.packet.skill_effect_total_damage,
                    skill_effect_group_index: damage.packet.skill_effect_group_index,
                    skill_effect_component_index: damage.packet.skill_effect_component_index,
                    skill_effect_component_count: damage.packet.skill_effect_component_count,
                    hit_part_damage_values: damage
                        .packet
                        .hit_parts
                        .iter()
                        .map(|part| part.damage_value)
                        .collect(),
                    packet_position: damage.packet.position.map(DamagePositionBits::from),
                    hit_part_positions: damage
                        .packet
                        .hit_parts
                        .iter()
                        .map(|part| part.position.map(DamagePositionBits::from))
                        .collect(),
                    target_monster_id: monster_ids.get(&target_key).copied(),
                    target_level: actor_levels.get(&target_key).copied(),
                    source_current_hp: health_attributes_at_wire_message_start
                        .get(&source_key)
                        .or_else(|| health_attributes.get(&source_key))
                        .and_then(|state| state.get(&CURRENT_HP_ATTRIBUTE_ID))
                        .copied(),
                    source_max_hp: health_attributes_at_wire_message_start
                        .get(&source_key)
                        .or_else(|| health_attributes.get(&source_key))
                        .and_then(|state| state.get(&MAX_HP_ATTRIBUTE_ID))
                        .copied(),
                    target_current_hp: health_attributes_at_wire_message_start
                        .get(&target_key)
                        .or_else(|| health_attributes.get(&target_key))
                        .and_then(|state| state.get(&CURRENT_HP_ATTRIBUTE_ID))
                        .copied(),
                    target_max_hp: health_attributes_at_wire_message_start
                        .get(&target_key)
                        .or_else(|| health_attributes.get(&target_key))
                        .and_then(|state| state.get(&MAX_HP_ATTRIBUTE_ID))
                        .copied(),
                    attack,
                    critical_damage: source_attributes
                        .get(&CRITICAL_DAMAGE_ATTRIBUTE_ID)
                        .copied(),
                    lucky_damage: source_attributes.get(&LUCKY_DAMAGE_ATTRIBUTE_ID).copied(),
                    external_damage: source_attributes
                        .get(&EXTERNAL_DAMAGE_ATTRIBUTE_ID)
                        .copied(),
                    mastery: source_attributes.get(&MASTERY_ATTRIBUTE_ID).copied(),
                    versatility: source_attributes.get(&VERSATILITY_ATTRIBUTE_ID).copied(),
                    formula_attributes: Arc::clone(source_attributes),
                    target_formula_attributes: attributes_at_wire_message_start
                        .get(&target_key)
                        .or_else(|| attributes.get(&target_key))
                        .cloned(),
                    effect_presence,
                    source_statuses: source_tracker
                        .semantic_snapshot_without_selected(args.effect_id, args.source_config_id),
                    target_statuses: target_tracker
                        .semantic_snapshot_without_selected(args.effect_id, args.source_config_id),
                };
                if !args.pair_proof_only {
                    observe_damage_time_attack_state(
                        args,
                        &context,
                        &sample,
                        damage_time_attack_states,
                    );
                    observe_direct_observed_context(
                        args,
                        &context,
                        &sample,
                        direct_observed_contexts,
                    );
                    observe_archetype_observed_context(
                        &context,
                        &sample,
                        archetype_observed_contexts,
                    );
                }
                if let (EffectPresence::External(provider), Some(surface), Some(provider_delta)) = (
                    sample.effect_presence,
                    damage_surface,
                    args.attack_provider_delta,
                ) {
                    let required_provider_status_matches =
                        args.required_provider_status.is_none_or(|required| {
                            let observed_active =
                                source_tracker.has_status_from_provider(required, provider);
                            if required.expected_active {
                                observed_active
                            } else {
                                current_run_ordinal > 0 && !observed_active
                            }
                        });
                    observe_single_event_damage_attr_counterfactual(
                        args,
                        surface,
                        &context,
                        &sample,
                        provider,
                        provider_delta,
                        required_provider_status_matches,
                        single_event_counterfactual,
                    );
                }
                let expiry_cutoff = sample.observed_micros.saturating_sub(args.max_gap_micros);
                while recent_expirations
                    .front()
                    .is_some_and(|(observed_micros, _)| *observed_micros < expiry_cutoff)
                {
                    let (expired_micros, expired_context) =
                        recent_expirations.pop_front().expect("front was present");
                    if recent.get(&expired_context).is_some_and(|samples| {
                        samples
                            .back()
                            .is_none_or(|sample| sample.observed_micros <= expired_micros)
                    }) {
                        recent.remove(&expired_context);
                    }
                }
                let samples = recent.entry(context.clone()).or_default();
                if !args.pair_proof_only
                    && let Some(previous) = samples.iter().rev().find(|previous| {
                        sample
                            .observed_micros
                            .saturating_sub(previous.observed_micros)
                            <= args.max_gap_micros
                            && same_damage_live_vector_conflict(previous, &sample)
                    })
                {
                    observe_live_vector_conflict(
                        &context,
                        previous,
                        &sample,
                        live_vector_stability,
                        args.example_limit,
                    );
                }
                for previous in samples.iter().rev() {
                    let gap = sample
                        .observed_micros
                        .saturating_sub(previous.observed_micros);
                    if gap > args.max_gap_micros {
                        break;
                    }
                    let selected_changed = matches!(
                        (previous.effect_presence, sample.effect_presence),
                        (EffectPresence::Inactive, EffectPresence::External(_))
                            | (EffectPresence::External(_), EffectPresence::Inactive)
                    );
                    if !selected_changed {
                        continue;
                    }
                    effect_transition_candidates_before_controls =
                        effect_transition_candidates_before_controls.saturating_add(1);
                    let ability_candidate = ability_transition_candidates
                        .entry(AbilityTransitionCandidateKey {
                            ability_id: context.ability_id,
                            critical: context.critical,
                            lucky: context.lucky,
                        })
                        .or_default();
                    ability_candidate.transitions = ability_candidate.transitions.saturating_add(1);
                    let (inactive, active, provider) =
                        match (previous.effect_presence, sample.effect_presence) {
                            (EffectPresence::Inactive, EffectPresence::External(provider)) => {
                                (previous, &sample, provider)
                            }
                            (EffectPresence::External(provider), EffectPresence::Inactive) => {
                                (&sample, previous, provider)
                            }
                            _ => continue,
                        };
                    if active.attack > inactive.attack {
                        observe_pair(
                            args,
                            damage_surface,
                            &context,
                            inactive,
                            active,
                            provider,
                            gap,
                            diagnostic_groups,
                            diagnostic_formula,
                        );
                        if !args.pair_proof_only {
                            observe_offensive_vector_candidates(
                                &context,
                                inactive,
                                active,
                                diagnostic_vector_formulas,
                            );
                        }
                    }
                    if previous.source_statuses != sample.source_statuses
                        || previous.target_statuses != sample.target_statuses
                    {
                        rejected_status_mismatch = rejected_status_mismatch.saturating_add(1);
                        ability_candidate.status_mismatches =
                            ability_candidate.status_mismatches.saturating_add(1);
                        observe_status_mismatches(
                            &context,
                            inactive,
                            active,
                            provider,
                            status_mismatches,
                        );
                        if !args.diagnostic_ignored_status_ids.is_empty()
                            && semantic_snapshots_equal_ignoring(
                                &previous.source_statuses,
                                &sample.source_statuses,
                                &args.diagnostic_ignored_status_ids,
                            )
                            && semantic_snapshots_equal_ignoring(
                                &previous.target_statuses,
                                &sample.target_statuses,
                                &args.diagnostic_ignored_status_ids,
                            )
                            && target_formula_attributes_match(inactive, active)
                            && active.attack > inactive.attack
                        {
                            if !args.pair_proof_only {
                                observe_offensive_vector_candidates(
                                    &context,
                                    inactive,
                                    active,
                                    companion_normalized_vector_formulas,
                                );
                            }
                            companion_normalized_pairs =
                                companion_normalized_pairs.saturating_add(1);
                            ability_candidate.companion_normalized_pairs = ability_candidate
                                .companion_normalized_pairs
                                .saturating_add(1);
                        }
                        continue;
                    }
                    match (
                        &inactive.target_formula_attributes,
                        &active.target_formula_attributes,
                    ) {
                        (Some(inactive_target), Some(active_target))
                            if inactive_target != active_target =>
                        {
                            rejected_target_attribute_mismatch =
                                rejected_target_attribute_mismatch.saturating_add(1);
                            ability_candidate.target_attribute_mismatches = ability_candidate
                                .target_attribute_mismatches
                                .saturating_add(1);
                            continue;
                        }
                        (None, _) | (_, None) => {
                            rejected_target_attribute_unknown =
                                rejected_target_attribute_unknown.saturating_add(1);
                            ability_candidate.target_attribute_unknown =
                                ability_candidate.target_attribute_unknown.saturating_add(1);
                            continue;
                        }
                        _ => {}
                    }
                    if previous.attack == sample.attack {
                        rejected_same_attack = rejected_same_attack.saturating_add(1);
                        continue;
                    }
                    if active.attack <= inactive.attack {
                        continue;
                    }
                    observe_offensive_vector_candidates(
                        &context,
                        inactive,
                        active,
                        strict_vector_formulas,
                    );
                    let eligible_for_paired_counterfactual_proof = observe_pair(
                        args,
                        damage_surface,
                        &context,
                        inactive,
                        active,
                        provider,
                        gap,
                        groups,
                        formula,
                    );
                    if eligible_for_paired_counterfactual_proof {
                        strict_pairs = strict_pairs.saturating_add(1);
                        ability_candidate.strict_pairs =
                            ability_candidate.strict_pairs.saturating_add(1);
                    }
                    break;
                }
                samples.push_back(sample);
                recent_expirations.push_back((envelope.time.observed_micros, context));
                while samples.len() > RECENT_PER_CONTEXT {
                    samples.pop_front();
                }
            }
            _ => {}
        }
    }

    let ability_transition_candidates = ability_transition_candidates
        .into_iter()
        .map(|(key, value)| AbilityTransitionCandidateReport {
            ability_id: key.ability_id,
            critical: key.critical,
            lucky: key.lucky,
            transitions: value.transitions,
            status_mismatches: value.status_mismatches,
            target_attribute_mismatches: value.target_attribute_mismatches,
            target_attribute_unknown: value.target_attribute_unknown,
            companion_normalized_pairs: value.companion_normalized_pairs,
            strict_pairs: value.strict_pairs,
        })
        .collect();
    let externally_affected_actor_specializations = externally_affected_actor_uuids
        .into_iter()
        .map(|source_entity_uuid| {
            let observed_ability_ids = observed_abilities_by_source
                .remove(&source_entity_uuid)
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            let identity = specialization_identity_from_observed_abilities(
                observed_ability_ids.iter().copied(),
            )?;
            Ok(ActorSpecializationReport {
                source_entity_uuid,
                observed_ability_ids,
                resolved_class_id: identity.map(|value| value.0),
                resolved_specialization_id: identity.map(|value| value.1),
                observed_status_routes: observed_status_routes_by_target
                    .remove(&source_entity_uuid)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(key, value)| ActorObservedStatusRouteReport {
                        effect_id: key.effect_id,
                        source_entity_uuid: key.source_entity_uuid,
                        source_config_id: key.source_config_id,
                        lifecycle_events: value.lifecycle_events,
                        external_active_damage_events: value.external_active_damage_events,
                    })
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(SessionSummary {
        rlog: file_label(path),
        session_id: session_id.unwrap_or_else(|| "unobserved".to_owned()),
        run_ordinals_observed: maximum_run_ordinal,
        damage_events,
        damage_events_with_attack,
        selected_status_events,
        external_active_damage_events,
        inactive_damage_events,
        ambiguous_or_self_active_damage_events,
        effect_transition_candidates_before_controls,
        strict_pairs,
        rejected_status_mismatch,
        rejected_target_attribute_mismatch,
        rejected_target_attribute_unknown,
        companion_normalized_pairs,
        rejected_same_attack,
        externally_affected_actor_specializations,
        ability_transition_candidates,
    })
}

fn observe_status_mismatches(
    context: &DamageContext,
    inactive: &DamageSample,
    active: &DamageSample,
    selected_provider: i64,
    inventory: &mut BTreeMap<StatusMismatchKey, u64>,
) {
    observe_status_side_mismatches(
        "damage_source",
        context,
        &inactive.source_statuses,
        &active.source_statuses,
        selected_provider,
        inventory,
    );
    observe_status_side_mismatches(
        "damage_target",
        context,
        &inactive.target_statuses,
        &active.target_statuses,
        selected_provider,
        inventory,
    );
}

fn observe_status_side_mismatches(
    owner_side: &'static str,
    context: &DamageContext,
    inactive: &[SemanticStatusEntry],
    active: &[SemanticStatusEntry],
    selected_provider: i64,
    inventory: &mut BTreeMap<StatusMismatchKey, u64>,
) {
    let inactive = inactive.iter().cloned().collect::<BTreeSet<_>>();
    let active = active.iter().cloned().collect::<BTreeSet<_>>();
    for entry in active.difference(&inactive) {
        observe_status_difference(
            owner_side,
            "active_only",
            context,
            entry,
            selected_provider,
            inventory,
        );
    }
    for entry in inactive.difference(&active) {
        observe_status_difference(
            owner_side,
            "inactive_only",
            context,
            entry,
            selected_provider,
            inventory,
        );
    }
}

fn observe_status_difference(
    owner_side: &'static str,
    direction: &'static str,
    context: &DamageContext,
    entry: &SemanticStatusEntry,
    selected_provider: i64,
    inventory: &mut BTreeMap<StatusMismatchKey, u64>,
) {
    let source_relation = match entry.source_entity_uuid {
        Some(source) if source == selected_provider => "selected_provider",
        Some(source) if source == context.source_entity_uuid => "damage_source",
        Some(source) if source == context.target_entity_uuid => "damage_target",
        Some(_) => "other_entity",
        None => "source_missing",
    };
    let key = StatusMismatchKey {
        ability_id: context.ability_id,
        owner_side,
        direction,
        effect_id: entry.effect_id,
        source_entity_uuid: entry.source_entity_uuid,
        source_config_id: entry.source_config_id,
        source_relation,
        stacks: entry.stacks,
        level: entry.level,
        part_id: entry.part_id,
        count: entry.count,
    };
    let count = inventory.entry(key).or_default();
    *count = count.saturating_add(1);
}

#[allow(clippy::too_many_arguments)]
fn observe_pair(
    args: &Arguments,
    damage_surface: Option<&DamageSurface>,
    context: &DamageContext,
    inactive: &DamageSample,
    active: &DamageSample,
    provider: i64,
    gap_micros: u64,
    groups: &mut BTreeMap<PairGroupKey, PairGroupAccumulator>,
    formula: &mut FormulaAccumulator,
) -> bool {
    let Some(predicted_active) = mul_div_floor(inactive.amount, active.attack, inactive.attack)
    else {
        return false;
    };
    let residual = active.amount.saturating_sub(predicted_active);
    let Some((minimum, maximum)) =
        counterfactual_interval(active.amount, inactive.attack, active.attack)
    else {
        return false;
    };
    let source_formula_attribute_deltas =
        formula_attribute_deltas(&inactive.formula_attributes, &active.formula_attributes);
    let attack_only_source_vector_controlled = source_formula_attribute_deltas
        .iter()
        .all(|delta| delta.attribute_id == args.attack_attribute_id);
    let paired_packet_input_deltas = pair_packet_input_deltas(inactive, active);
    let paired_packet_inputs_controlled = paired_packet_input_deltas.is_empty();
    let eligible_for_paired_counterfactual_proof =
        attack_only_source_vector_controlled && paired_packet_inputs_controlled;
    let proof_classification = match (
        attack_only_source_vector_controlled,
        paired_packet_inputs_controlled,
    ) {
        (true, true) => "strict_attack_only_packet_input_control",
        (false, true) => "status_target_controlled_source_multi_axis_diagnostic",
        (true, false) => "status_target_controlled_packet_input_mismatch_diagnostic",
        (false, false) => {
            "status_target_controlled_source_multi_axis_and_packet_input_mismatch_diagnostic"
        }
    };
    let compatible = minimum <= maximum;
    let unique = (minimum == maximum).then_some(minimum);
    let marginal = unique.and_then(|value| active.amount.checked_sub(value));
    let key = PairGroupKey {
        critical: context.critical,
        lucky: context.lucky,
        attack_delta: active.attack.saturating_sub(inactive.attack),
    };
    let accumulator = groups.entry(key).or_default();
    accumulator.pairs = accumulator.pairs.saturating_add(1);
    if eligible_for_paired_counterfactual_proof {
        accumulator.paired_counterfactual_proof_eligible_pairs = accumulator
            .paired_counterfactual_proof_eligible_pairs
            .saturating_add(1);
    }
    if !attack_only_source_vector_controlled {
        accumulator.source_multi_axis_pairs = accumulator.source_multi_axis_pairs.saturating_add(1);
    }
    if !paired_packet_inputs_controlled {
        accumulator.packet_input_mismatch_pairs =
            accumulator.packet_input_mismatch_pairs.saturating_add(1);
    }
    if residual == 0 {
        accumulator.exact_ratio_matches = accumulator.exact_ratio_matches.saturating_add(1);
    } else if residual.unsigned_abs() <= 1 {
        accumulator.within_one_ratio_matches =
            accumulator.within_one_ratio_matches.saturating_add(1);
    } else {
        accumulator.mismatches = accumulator.mismatches.saturating_add(1);
    }
    if let Some(value) = unique {
        accumulator.unique_counterfactuals = accumulator.unique_counterfactuals.saturating_add(1);
        if value == inactive.amount {
            accumulator.unique_counterfactuals_matching_paired_inactive = accumulator
                .unique_counterfactuals_matching_paired_inactive
                .saturating_add(1);
        }
    } else if compatible {
        accumulator.ambiguous_counterfactuals =
            accumulator.ambiguous_counterfactuals.saturating_add(1);
    } else {
        accumulator.impossible_counterfactuals =
            accumulator.impossible_counterfactuals.saturating_add(1);
    }
    observe_formula_counts(formula, residual, compatible, unique, inactive.amount);
    if accumulator.examples.len() < args.example_limit {
        let (damage_stage, damage_stage_gap) = match damage_surface {
            Some(surface) => match pair_damage_stage_proof(
                args.attack_attribute_id,
                surface,
                context,
                inactive,
                active,
            ) {
                Ok(proof) => (Some(proof), None),
                Err(reason) => (None, Some(reason)),
            },
            None => (None, Some("damage_surface_not_supplied")),
        };
        accumulator.examples.push(PairExample {
            rlog: active.rlog.clone(),
            session_id: active.session_id.clone(),
            run_ordinal: context.run_ordinal,
            source_entity_uuid: context.source_entity_uuid,
            provider_entity_uuid: provider,
            target_entity_uuid: context.target_entity_uuid,
            ability_id: context.ability_id,
            hit_event_id: context.hit_event_id,
            damage_source: context.damage_source,
            damage_type: context.damage_type,
            property: context.property,
            owner_level: context.owner_level,
            owner_stage: context.owner_stage,
            critical: context.critical,
            lucky: context.lucky,
            inactive_sequence: inactive.sequence,
            active_sequence: active.sequence,
            gap_micros,
            inactive_damage: inactive.amount,
            active_damage: active.amount,
            inactive_attack: inactive.attack,
            active_attack: active.attack,
            attack_delta: active.attack.saturating_sub(inactive.attack),
            source_formula_attribute_deltas,
            attack_only_source_vector_controlled,
            paired_packet_input_deltas,
            paired_packet_inputs_controlled,
            eligible_for_paired_counterfactual_proof,
            proof_classification,
            predicted_active_damage_from_ratio: predicted_active,
            ratio_residual: residual,
            counterfactual_minimum: minimum,
            counterfactual_maximum: maximum,
            counterfactual_is_compatible: compatible,
            unique_counterfactual: unique,
            unique_marginal: marginal,
            damage_stage,
            damage_stage_gap,
        });
    }
    eligible_for_paired_counterfactual_proof
}

fn pair_damage_stage_proof(
    final_attack_attribute_id: i32,
    surface: &DamageSurface,
    context: &DamageContext,
    inactive: &DamageSample,
    active: &DamageSample,
) -> Result<PairDamageStageProof, &'static str> {
    if !surface.identity.build_identity_verified {
        return Err("damage_surface_build_identity_not_verified");
    }
    let ability_id = context.ability_id.ok_or("missing_ability_identity")?;
    let hit_event_id = semantic_hit_event_id(context.hit_event_id);
    let rows = surface
        .rows_by_key
        .get(&(ability_id, hit_event_id))
        .ok_or("missing_damage_surface_key")?;
    let matching_rows = rows
        .iter()
        .filter(|row| {
            row.required_damage_source.is_none()
                || row.required_damage_source == context.damage_source
        })
        .collect::<Vec<_>>();
    if matching_rows.is_empty() {
        return Err("damage_source_does_not_match_damage_surface_route");
    }
    let [row] = matching_rows.as_slice() else {
        return Err("ambiguous_damage_surface_key");
    };
    let expected_script = match final_attack_attribute_id {
        11_330 => "Attack",
        11_340 => "MAttack",
        _ => return Err("unsupported_attack_family"),
    };
    if row.damage_script.as_deref() != Some(expected_script) {
        return Err("damage_script_does_not_match_attack_family");
    }
    let coefficient = select_stage_coefficient(&row.pve_damage_ratio, context.owner_stage)
        .ok_or("owner_stage_out_of_range_or_empty_coefficient")?;
    let fixed = if row.pve_fixed_parameter.is_empty() {
        0
    } else {
        let owner_level = context
            .owner_level
            .ok_or("missing_owner_level_for_fixed_parameter")?;
        let level =
            usize::try_from(owner_level).map_err(|_| "invalid_owner_level_for_fixed_parameter")?;
        *level
            .checked_sub(1)
            .and_then(|index| row.pve_fixed_parameter.get(index))
            .ok_or("owner_level_out_of_fixed_parameter_range")?
    };
    let active_base = mul_div_floor(active.attack, coefficient, 10_000)
        .and_then(|value| value.checked_add(fixed))
        .ok_or("active_damage_base_overflow")?;
    let inactive_base = mul_div_floor(inactive.attack, coefficient, 10_000)
        .and_then(|value| value.checked_add(fixed))
        .ok_or("inactive_damage_base_overflow")?;
    let provider_base_marginal = active_base
        .checked_sub(inactive_base)
        .filter(|value| *value > 0)
        .ok_or("nonpositive_provider_base_marginal")?;
    let provider_attack_marginal = active
        .attack
        .checked_sub(inactive.attack)
        .filter(|value| *value > 0)
        .ok_or("nonpositive_provider_attack_marginal")?;
    let (share_numerator, share_denominator) = exact_external_attack_coefficient_stage_fraction(
        active.amount,
        PacketDamageScriptFamily::StandardAttack,
        active.attack,
        provider_attack_marginal,
        coefficient,
        fixed,
    )
    .ok_or("exact_conserved_attack_stage_share_unavailable")?;
    let (counterfactual_minimum, counterfactual_maximum) =
        counterfactual_interval(active.amount, inactive_base, active_base)
            .ok_or("attack_only_counterfactual_interval_unavailable")?;
    let paired_inactive_matches =
        inactive.amount >= counterfactual_minimum && inactive.amount <= counterfactual_maximum;
    Ok(PairDamageStageProof {
        authority: row.authority,
        damage_id: row.damage_id.clone(),
        damage_script: expected_script.to_owned(),
        coefficient_basis_points: coefficient,
        fixed_parameter: fixed,
        active_base,
        inactive_base,
        provider_base_marginal,
        exact_conserved_attack_stage_share_numerator: share_numerator.to_string(),
        exact_conserved_attack_stage_share_denominator: share_denominator.to_string(),
        attack_only_counterfactual_minimum: counterfactual_minimum,
        attack_only_counterfactual_maximum: counterfactual_maximum,
        paired_inactive_matches_attack_only_counterfactual: paired_inactive_matches,
        paired_observed_damage_delta: active.amount.saturating_sub(inactive.amount),
    })
}

fn observe_formula_counts(
    formula: &mut FormulaAccumulator,
    residual: i64,
    counterfactual_is_compatible: bool,
    unique: Option<i64>,
    observed_inactive: i64,
) {
    formula.pairs = formula.pairs.saturating_add(1);
    formula.maximum_absolute_residual = formula
        .maximum_absolute_residual
        .max(residual.unsigned_abs());
    if formula.residuals.len() < DEFAULT_EXAMPLE_LIMIT {
        formula.residuals.insert(residual);
    }
    if residual == 0 {
        formula.exact_ratio_matches = formula.exact_ratio_matches.saturating_add(1);
    } else if residual.unsigned_abs() <= 1 {
        formula.within_one_ratio_matches = formula.within_one_ratio_matches.saturating_add(1);
    } else {
        formula.mismatches = formula.mismatches.saturating_add(1);
    }
    if let Some(value) = unique {
        formula.unique_counterfactuals = formula.unique_counterfactuals.saturating_add(1);
        if value == observed_inactive {
            formula.unique_counterfactuals_matching_paired_inactive = formula
                .unique_counterfactuals_matching_paired_inactive
                .saturating_add(1);
        }
    } else if counterfactual_is_compatible {
        formula.ambiguous_counterfactuals = formula.ambiguous_counterfactuals.saturating_add(1);
    } else {
        formula.impossible_counterfactuals = formula.impossible_counterfactuals.saturating_add(1);
    }
}

fn observe_offensive_vector_candidates(
    context: &DamageContext,
    inactive: &DamageSample,
    active: &DamageSample,
    formulas: &mut BTreeMap<&'static str, OffensiveVectorFormulaAccumulator>,
) {
    for (formula, include_mastery, include_external_damage, include_versatility) in
        OFFENSIVE_VECTOR_FORMULAS
    {
        observe_offensive_vector_candidate(
            formula,
            context,
            inactive,
            active,
            include_mastery,
            include_external_damage,
            include_versatility,
            formulas,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn observe_offensive_vector_candidate(
    formula: &'static str,
    context: &DamageContext,
    inactive: &DamageSample,
    active: &DamageSample,
    include_mastery: bool,
    include_external_damage: bool,
    include_versatility: bool,
    formulas: &mut BTreeMap<&'static str, OffensiveVectorFormulaAccumulator>,
) {
    let Some(inactive_factor) = offensive_vector_factor(
        context,
        inactive,
        include_mastery,
        include_external_damage,
        include_versatility,
    ) else {
        return;
    };
    let Some(active_factor) = offensive_vector_factor(
        context,
        active,
        include_mastery,
        include_external_damage,
        include_versatility,
    ) else {
        return;
    };
    let Some(predicted) = mul_div_floor_i128(inactive.amount, active_factor, inactive_factor)
    else {
        return;
    };
    let residual = active.amount.saturating_sub(predicted);
    let accumulator = formulas.entry(formula).or_default();
    accumulator.evaluable_pairs = accumulator.evaluable_pairs.saturating_add(1);
    accumulator.maximum_absolute_residual = accumulator
        .maximum_absolute_residual
        .max(residual.unsigned_abs());
    if accumulator.residuals.len() < DEFAULT_EXAMPLE_LIMIT {
        accumulator.residuals.insert(residual);
    }
    if residual == 0 {
        accumulator.exact_matches = accumulator.exact_matches.saturating_add(1);
    } else if residual.unsigned_abs() <= 1 {
        accumulator.within_one_matches = accumulator.within_one_matches.saturating_add(1);
    } else {
        accumulator.mismatches = accumulator.mismatches.saturating_add(1);
    }
    if accumulator.examples.len() < DEFAULT_EXAMPLE_LIMIT {
        accumulator.examples.push(OffensiveVectorPairExample {
            rlog: active.rlog.clone(),
            session_id: active.session_id.clone(),
            run_ordinal: context.run_ordinal,
            source_entity_uuid: context.source_entity_uuid,
            target_entity_uuid: context.target_entity_uuid,
            ability_id: context.ability_id,
            hit_event_id: context.hit_event_id,
            damage_source: context.damage_source,
            damage_type: context.damage_type,
            property: context.property,
            critical: context.critical,
            lucky: context.lucky,
            inactive_sequence: inactive.sequence,
            active_sequence: active.sequence,
            inactive_damage: inactive.amount,
            active_damage: active.amount,
            inactive_attack: inactive.attack,
            active_attack: active.attack,
            inactive_critical_damage: inactive.critical_damage,
            active_critical_damage: active.critical_damage,
            inactive_lucky_damage: inactive.lucky_damage,
            active_lucky_damage: active.lucky_damage,
            inactive_mastery: inactive.mastery,
            active_mastery: active.mastery,
            inactive_external_damage: inactive.external_damage,
            active_external_damage: active.external_damage,
            inactive_versatility: inactive.versatility,
            active_versatility: active.versatility,
            inactive_source_attributes: formula_attribute_values(&inactive.formula_attributes),
            active_source_attributes: formula_attribute_values(&active.formula_attributes),
            target_attributes: active
                .target_formula_attributes
                .as_deref()
                .map_or_else(Vec::new, formula_attribute_values),
            changed_source_attributes: formula_attribute_deltas(
                &inactive.formula_attributes,
                &active.formula_attributes,
            ),
            changed_target_attributes: target_formula_attribute_deltas(inactive, active),
            inactive_factor: inactive_factor.to_string(),
            active_factor: active_factor.to_string(),
            predicted_active_damage: predicted,
            residual,
        });
    }
}

fn formula_attribute_values(attributes: &BTreeMap<i32, i64>) -> Vec<AttributeVectorValue> {
    attributes
        .iter()
        .map(|(&attribute_id, &value)| AttributeVectorValue {
            attribute_id,
            value,
        })
        .collect()
}

const FUNCTIONAL_AMP_AFFECTED_ATTRIBUTE_IDS: [i32; 18] = [
    11_330, 11_331, 11_332, 11_333, 11_334, 11_335, 11_340, 11_341, 11_342, 11_343, 11_344, 11_345,
    11_720, 11_721, 11_722, 11_730, 11_731, 11_732,
];

// Only fields proven to change under Functional Amp are removed from the
// direct-observation context key. Stable family inputs (base add, extra add,
// and extra percent) remain controlled rather than being discarded with the
// derived outputs.
const FUNCTIONAL_AMP_CHANGED_ATTRIBUTE_IDS: [i32; 12] = [
    11_330, 11_331, 11_334, 11_340, 11_341, 11_344, 11_720, 11_721, 11_722, 11_730, 11_731, 11_732,
];

fn observe_direct_observed_context(
    args: &Arguments,
    context: &DamageContext,
    sample: &DamageSample,
    contexts: &mut BTreeMap<DirectObservedContext, DirectObservedContextAccumulator>,
) {
    let Some(target_attributes) = sample.target_formula_attributes.as_deref() else {
        return;
    };
    if target_attributes.is_empty() {
        return;
    }
    let key = DirectObservedContext {
        rlog: sample.rlog.clone(),
        session_id: sample.session_id.clone(),
        damage: context.clone(),
        unaffected_source_attributes: sample
            .formula_attributes
            .iter()
            .filter(|(attribute_id, _)| {
                !FUNCTIONAL_AMP_CHANGED_ATTRIBUTE_IDS.contains(attribute_id)
            })
            .map(|(&attribute_id, &value)| (attribute_id, value))
            .collect(),
        target_attributes: target_attributes
            .iter()
            .map(|(&attribute_id, &value)| (attribute_id, value))
            .collect(),
        source_statuses: sample.source_statuses.clone(),
        target_statuses: sample.target_statuses.clone(),
    };
    let state = contexts.entry(key).or_default();
    match sample.effect_presence {
        EffectPresence::Inactive => {
            *state
                .inactive
                .entry((sample.attack, sample.amount))
                .or_default() += 1;
            state.first_inactive_sequence = Some(
                state
                    .first_inactive_sequence
                    .map_or(sample.sequence, |value| value.min(sample.sequence)),
            );
        }
        EffectPresence::External(provider) => {
            let Some(provider_delta) = args.attack_provider_delta else {
                return;
            };
            let Ok(counterfactual_attack) = counterfactual_attack_without_provider_delta(
                &sample.formula_attributes,
                args.attack_attribute_id,
                provider_delta,
            ) else {
                return;
            };
            *state
                .external
                .entry((
                    provider,
                    sample.attack,
                    counterfactual_attack,
                    sample.amount,
                ))
                .or_default() += 1;
            state.first_external_sequence = Some(
                state
                    .first_external_sequence
                    .map_or(sample.sequence, |value| value.min(sample.sequence)),
            );
        }
        EffectPresence::SelfOwned | EffectPresence::Ambiguous => {}
    }
}

fn direct_observed_counterfactual_report(
    contexts: BTreeMap<DirectObservedContext, DirectObservedContextAccumulator>,
    example_limit: usize,
) -> DirectObservedCounterfactualReport {
    let context_count = contexts.len() as u64;
    let mut contexts_with_both_states = 0_u64;
    let mut exact_contexts = 0_u64;
    let mut ambiguous_contexts = 0_u64;
    let mut exact_external_events = 0_u64;
    let mut exact_observed_damage = 0_i128;
    let mut exact_counterfactual_damage = 0_i128;
    let mut exact_provider_marginal = 0_i128;
    let mut examples = Vec::new();
    for (key, value) in contexts {
        if value.inactive.is_empty() || value.external.is_empty() {
            continue;
        }
        contexts_with_both_states = contexts_with_both_states.saturating_add(1);
        if value.inactive.len() != 1 || value.external.len() != 1 {
            ambiguous_contexts = ambiguous_contexts.saturating_add(1);
            continue;
        }
        let Some((inactive_key, _inactive_events)) = value.inactive.iter().next() else {
            continue;
        };
        let Some((external_key, external_events)) = value.external.iter().next() else {
            continue;
        };
        let (inactive_attack, inactive_damage) = *inactive_key;
        let (provider, active_attack, counterfactual_attack, active_damage) = *external_key;
        if counterfactual_attack != inactive_attack || active_damage < inactive_damage {
            ambiguous_contexts = ambiguous_contexts.saturating_add(1);
            continue;
        }
        exact_contexts = exact_contexts.saturating_add(1);
        exact_external_events = exact_external_events.saturating_add(*external_events);
        exact_observed_damage = exact_observed_damage
            .saturating_add(i128::from(active_damage).saturating_mul(i128::from(*external_events)));
        exact_counterfactual_damage = exact_counterfactual_damage.saturating_add(
            i128::from(inactive_damage).saturating_mul(i128::from(*external_events)),
        );
        let damage_marginal = active_damage.saturating_sub(inactive_damage);
        exact_provider_marginal = exact_provider_marginal.saturating_add(
            i128::from(damage_marginal).saturating_mul(i128::from(*external_events)),
        );
        if examples.len() < example_limit {
            examples.push(DirectObservedCounterfactualExample {
                rlog: key.rlog,
                session_id: key.session_id,
                run_ordinal: key.damage.run_ordinal,
                source_entity_uuid: key.damage.source_entity_uuid,
                provider_entity_uuid: provider,
                target_entity_uuid: key.damage.target_entity_uuid,
                ability_id: key.damage.ability_id,
                hit_event_id: key.damage.hit_event_id,
                critical: key.damage.critical,
                lucky: key.damage.lucky,
                inactive_attack,
                active_attack,
                inactive_damage,
                active_damage,
                provider_attack_marginal: active_attack.saturating_sub(inactive_attack),
                provider_damage_marginal: damage_marginal,
                external_events: *external_events,
                first_inactive_sequence: value.first_inactive_sequence,
                first_external_sequence: value.first_external_sequence,
            });
        }
    }
    DirectObservedCounterfactualReport {
        authority: "exact_empirical_current_build_pair_not_a_generic_formula",
        context_control: "same session, run, source, direct source, target, action, hit, flags, packet dimensions, stable source attributes, complete packet-observed target vector, and all other source and target status state; only the selected effect's proven changed attributes are excluded",
        contexts: context_count,
        contexts_with_both_states,
        exact_contexts,
        ambiguous_contexts,
        exact_external_events,
        exact_observed_damage: exact_observed_damage.to_string(),
        exact_counterfactual_damage: exact_counterfactual_damage.to_string(),
        exact_provider_marginal: exact_provider_marginal.to_string(),
        examples,
    }
}

fn observe_archetype_outcome(
    evidence: &mut ArchetypeOutcomeEvidence,
    context: &DamageContext,
    sample: &DamageSample,
) {
    evidence.events = evidence.events.saturating_add(1);
    evidence.first_sequence = Some(
        evidence
            .first_sequence
            .map_or(sample.sequence, |value| value.min(sample.sequence)),
    );
    evidence
        .target_entity_uuids
        .insert(context.target_entity_uuid);
    if let Some(value) = sample.target_current_hp {
        evidence.target_current_hp_values.insert(value);
    } else {
        evidence.events_without_target_current_hp =
            evidence.events_without_target_current_hp.saturating_add(1);
    }
    if let Some(position) = sample.packet_position {
        evidence.packet_positions.insert(position);
    } else {
        evidence.events_without_packet_position =
            evidence.events_without_packet_position.saturating_add(1);
    }
    let complete_hit_part_positions = !sample.hit_part_positions.is_empty()
        && sample.hit_part_positions.iter().all(Option::is_some);
    if complete_hit_part_positions {
        evidence
            .hit_part_positions
            .insert(sample.hit_part_positions.clone());
    } else {
        evidence.events_without_complete_hit_part_positions = evidence
            .events_without_complete_hit_part_positions
            .saturating_add(1);
    }
    if let Some(group_index) = sample.skill_effect_group_index {
        evidence.skill_effect_group_indices.insert(group_index);
    }
}

fn observe_archetype_observed_context(
    context: &DamageContext,
    sample: &DamageSample,
    contexts: &mut BTreeMap<ArchetypeObservedContext, ArchetypeObservedContextAccumulator>,
) {
    let (Some(target_monster_id), Some(target_level), Some(target_attributes)) = (
        sample.target_monster_id,
        sample.target_level,
        sample.target_formula_attributes.as_deref(),
    ) else {
        return;
    };
    if target_attributes.is_empty() {
        return;
    }
    let key = ArchetypeObservedContext {
        rlog: sample.rlog.clone(),
        session_id: sample.session_id.clone(),
        run_ordinal: context.run_ordinal,
        source_entity_uuid: context.source_entity_uuid,
        direct_source_entity_uuid: context.direct_source_entity_uuid,
        raw_attacker_uuid: context.raw_attacker_uuid,
        raw_top_summoner_uuid: context.raw_top_summoner_uuid,
        raw_owner_id: context.raw_owner_id,
        target_monster_id,
        target_level,
        ability_id: context.ability_id,
        hit_event_id: context.hit_event_id,
        damage_source: context.damage_source,
        damage_type: context.damage_type,
        critical: context.critical,
        lucky: context.lucky,
        causes_lucky: sample.causes_lucky,
        blocked: context.blocked,
        periodic: context.periodic,
        missed: sample.missed,
        reported_critical: sample.reported_critical,
        type_flags: sample.type_flags,
        owner_level: context.owner_level,
        owner_stage: context.owner_stage,
        normal_hit: context.normal_hit,
        property: context.property,
        hit_part_ids: context.hit_part_ids.clone(),
        damage_weight_bits: context.damage_weight_bits,
        passive_uuid: context.passive_uuid,
        rainbow: context.rainbow,
        damage_mode: context.damage_mode,
        skill_effect_component_index: sample.skill_effect_component_index,
        skill_effect_component_count: sample.skill_effect_component_count,
        unaffected_source_attributes: sample
            .formula_attributes
            .iter()
            .filter(|(attribute_id, _)| {
                !FUNCTIONAL_AMP_CHANGED_ATTRIBUTE_IDS.contains(attribute_id)
            })
            .map(|(&attribute_id, &value)| (attribute_id, value))
            .collect(),
        target_attributes: target_attributes
            .iter()
            .map(|(&attribute_id, &value)| (attribute_id, value))
            .collect(),
        source_current_hp: sample.source_current_hp,
        source_max_hp: sample.source_max_hp,
        target_max_hp: sample.target_max_hp,
        source_statuses: sample.source_statuses.clone(),
        target_statuses: sample.target_statuses.clone(),
    };
    let state = contexts.entry(key).or_default();
    match sample.effect_presence {
        EffectPresence::Inactive => observe_archetype_outcome(
            state
                .inactive
                .entry((sample.attack, sample.amount))
                .or_default(),
            context,
            sample,
        ),
        EffectPresence::External(provider) => observe_archetype_outcome(
            state
                .external
                .entry((provider, sample.attack, sample.amount))
                .or_default(),
            context,
            sample,
        ),
        EffectPresence::SelfOwned | EffectPresence::Ambiguous => {}
    }
}

fn has_entity_invariance(evidence: &ArchetypeOutcomeEvidence) -> bool {
    evidence.target_entity_uuids.len() >= 2
}

fn has_position_invariance(evidence: &ArchetypeOutcomeEvidence) -> bool {
    let packet_positions_proven =
        evidence.events_without_packet_position == 0 && evidence.packet_positions.len() >= 2;
    let hit_part_positions_proven = evidence.events_without_complete_hit_part_positions == 0
        && evidence.hit_part_positions.len() >= 2;
    packet_positions_proven || hit_part_positions_proven
}

fn has_target_hp_invariance(evidence: &ArchetypeOutcomeEvidence) -> bool {
    evidence.events_without_target_current_hp == 0 && evidence.target_current_hp_values.len() >= 2
}

const ARCHETYPE_OVERLAP_STAGES: [(&str, &str); 8] = [
    (
        "action_archetype",
        "same session, run, source, direct source, monster config and level, ability, hit, critical/lucky outcome, and component identity",
    ),
    (
        "packet_inputs",
        "action_archetype plus all retained damage flags and packet calculation inputs; occurrence outputs, positions, exact target entity UUID, and group index remain excluded",
    ),
    (
        "stable_source_attributes",
        "packet_inputs plus every packet-current source formula attribute not proven changed by Functional Amp",
    ),
    (
        "target_state",
        "stable_source_attributes plus packet-observed target formula attributes and target max HP",
    ),
    (
        "source_status_state",
        "target_state plus every other source semantic status; target statuses remain excluded for diagnosis",
    ),
    (
        "target_status_state",
        "target_state plus every target semantic status; source statuses remain excluded for diagnosis",
    ),
    (
        "all_status_state",
        "target_state plus every other source and target semantic status",
    ),
    (
        "source_health_state",
        "all_status_state plus source current and max HP",
    ),
];

fn archetype_overlap_key(key: &ArchetypeObservedContext, stage: usize) -> String {
    let base = serde_json::json!({
        "rlog": key.rlog,
        "session_id": key.session_id,
        "run_ordinal": key.run_ordinal,
        "source_entity_uuid": key.source_entity_uuid,
        "direct_source_entity_uuid": key.direct_source_entity_uuid,
        "target_monster_id": key.target_monster_id,
        "target_level": key.target_level,
        "ability_id": key.ability_id,
        "hit_event_id": key.hit_event_id,
        "critical": key.critical,
        "lucky": key.lucky,
        "skill_effect_component_index": key.skill_effect_component_index,
        "skill_effect_component_count": key.skill_effect_component_count,
    });
    if stage == 0 {
        return base.to_string();
    }
    let packet = serde_json::json!({
        "base": base,
        "damage_source": key.damage_source,
        "damage_type": key.damage_type,
        "causes_lucky": key.causes_lucky,
        "blocked": key.blocked,
        "periodic": key.periodic,
        "missed": key.missed,
        "reported_critical": key.reported_critical,
        "type_flags": key.type_flags,
        "owner_level": key.owner_level,
        "owner_stage": key.owner_stage,
        "normal_hit": key.normal_hit,
        "property": key.property,
        "hit_part_ids": key.hit_part_ids,
        "damage_weight_bits": key.damage_weight_bits,
        "passive_uuid": key.passive_uuid,
        "rainbow": key.rainbow,
        "damage_mode": key.damage_mode,
    });
    if stage == 1 {
        return packet.to_string();
    }
    let source = serde_json::json!({
        "packet": packet,
        "unaffected_source_attributes": key.unaffected_source_attributes,
    });
    if stage == 2 {
        return source.to_string();
    }
    let target = serde_json::json!({
        "source": source,
        "target_attributes": key.target_attributes,
        "target_max_hp": key.target_max_hp,
    });
    if stage == 3 {
        return target.to_string();
    }
    let source_statuses = serde_json::json!({
        "target": target,
        "source_statuses": key.source_statuses,
    });
    if stage == 4 {
        return source_statuses.to_string();
    }
    let target = serde_json::json!({
        "target": archetype_overlap_key(key, 3),
        "target_statuses": key.target_statuses,
    });
    if stage == 5 {
        return target.to_string();
    }
    let statuses = serde_json::json!({
        "source_status_state": source_statuses,
        "target_statuses": key.target_statuses,
    });
    if stage == 6 {
        return statuses.to_string();
    }
    serde_json::json!({
        "statuses": statuses,
        "source_current_hp": key.source_current_hp,
        "source_max_hp": key.source_max_hp,
    })
    .to_string()
}

fn archetype_observed_counterfactual_report(
    contexts: BTreeMap<ArchetypeObservedContext, ArchetypeObservedContextAccumulator>,
    example_limit: usize,
) -> ArchetypeObservedCounterfactualReport {
    let mut overlap_sets = ARCHETYPE_OVERLAP_STAGES
        .iter()
        .map(|_| (BTreeSet::<String>::new(), BTreeSet::<String>::new()))
        .collect::<Vec<_>>();
    for (key, state) in &contexts {
        for (stage, (inactive, external)) in overlap_sets.iter_mut().enumerate() {
            let projected = archetype_overlap_key(key, stage);
            if !state.inactive.is_empty() {
                inactive.insert(projected.clone());
            }
            if !state.external.is_empty() {
                external.insert(projected);
            }
        }
    }
    let overlap_diagnostics = overlap_sets
        .into_iter()
        .zip(ARCHETYPE_OVERLAP_STAGES)
        .map(|((inactive, external), (stage, controlled_dimensions))| {
            let keys_with_both_states = inactive.intersection(&external).count();
            ArchetypeOverlapDiagnostic {
                stage,
                controlled_dimensions,
                inactive_keys: inactive.len(),
                external_keys: external.len(),
                keys_with_both_states,
            }
        })
        .collect();
    let mut status_mismatch_groups = BTreeMap::<
        String,
        (
            ArchetypeObservedContext,
            BTreeSet<Vec<SemanticStatusEntry>>,
            BTreeSet<Vec<SemanticStatusEntry>>,
            BTreeSet<Vec<SemanticStatusEntry>>,
            BTreeSet<Vec<SemanticStatusEntry>>,
            BTreeSet<ArchetypeStatusOutcome>,
            BTreeSet<ArchetypeStatusOutcome>,
        ),
    >::new();
    for (key, state) in &contexts {
        let projected = archetype_overlap_key(key, 3);
        let entry = status_mismatch_groups.entry(projected).or_insert_with(|| {
            (
                key.clone(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
            )
        });
        if !state.inactive.is_empty() {
            entry.1.insert(key.source_statuses.clone());
            entry.3.insert(key.target_statuses.clone());
            entry
                .5
                .extend(state.inactive.iter().map(|((attack, damage), evidence)| {
                    ArchetypeStatusOutcome {
                        provider_entity_uuid: None,
                        attack: *attack,
                        damage: *damage,
                        events: evidence.events,
                        first_sequence: evidence.first_sequence,
                    }
                }));
        }
        if !state.external.is_empty() {
            entry.2.insert(key.source_statuses.clone());
            entry.4.insert(key.target_statuses.clone());
            entry.6.extend(
                state
                    .external
                    .iter()
                    .map(
                        |((provider, attack, damage), evidence)| ArchetypeStatusOutcome {
                            provider_entity_uuid: Some(*provider),
                            attack: *attack,
                            damage: *damage,
                            events: evidence.events,
                            first_sequence: evidence.first_sequence,
                        },
                    ),
            );
        }
    }
    let target_state_status_mismatch_examples = status_mismatch_groups
        .into_values()
        .filter(
            |(_, inactive_source, external_source, inactive_target, external_target, _, _)| {
                !inactive_source.is_empty()
                    && !external_source.is_empty()
                    && !inactive_target.is_empty()
                    && !external_target.is_empty()
            },
        )
        .take(example_limit)
        .map(
            |(
                key,
                inactive_source,
                external_source,
                inactive_target,
                external_target,
                inactive_outcomes,
                external_outcomes,
            )| {
                ArchetypeStatusMismatchExample {
                    rlog: key.rlog,
                    session_id: key.session_id,
                    run_ordinal: key.run_ordinal,
                    source_entity_uuid: key.source_entity_uuid,
                    target_monster_id: key.target_monster_id,
                    target_level: key.target_level,
                    ability_id: key.ability_id,
                    hit_event_id: key.hit_event_id,
                    inactive_source_status_snapshots: inactive_source.into_iter().collect(),
                    external_source_status_snapshots: external_source.into_iter().collect(),
                    inactive_target_status_snapshots: inactive_target.into_iter().collect(),
                    external_target_status_snapshots: external_target.into_iter().collect(),
                    inactive_outcomes: inactive_outcomes.into_iter().collect(),
                    external_outcomes: external_outcomes.into_iter().collect(),
                }
            },
        )
        .collect();
    let context_count = contexts.len() as u64;
    let mut contexts_with_both_states = 0_u64;
    let mut contexts_with_unique_state_outcomes = 0_u64;
    let mut contexts_passing_entity_invariance = 0_u64;
    let mut contexts_passing_position_invariance = 0_u64;
    let mut contexts_passing_target_hp_invariance = 0_u64;
    let mut exact_contexts = 0_u64;
    let mut rejected_ambiguous_state_outcome = 0_u64;
    let mut rejected_insufficient_entity_diversity = 0_u64;
    let mut rejected_missing_or_insufficient_position_diversity = 0_u64;
    let mut rejected_missing_or_insufficient_target_hp_diversity = 0_u64;
    let mut exact_external_events = 0_u64;
    let mut exact_observed_damage = 0_i128;
    let mut exact_counterfactual_damage = 0_i128;
    let mut exact_provider_marginal = 0_i128;
    let mut examples = Vec::new();

    for (key, state) in contexts {
        if state.inactive.is_empty() || state.external.is_empty() {
            continue;
        }
        contexts_with_both_states = contexts_with_both_states.saturating_add(1);
        if state.inactive.len() != 1 || state.external.len() != 1 {
            rejected_ambiguous_state_outcome = rejected_ambiguous_state_outcome.saturating_add(1);
            continue;
        }
        contexts_with_unique_state_outcomes = contexts_with_unique_state_outcomes.saturating_add(1);
        let Some(((inactive_attack, inactive_damage), inactive_evidence)) =
            state.inactive.iter().next()
        else {
            continue;
        };
        let Some(((provider, active_attack, active_damage), external_evidence)) =
            state.external.iter().next()
        else {
            continue;
        };

        if !has_entity_invariance(inactive_evidence) || !has_entity_invariance(external_evidence) {
            rejected_insufficient_entity_diversity =
                rejected_insufficient_entity_diversity.saturating_add(1);
            continue;
        }
        contexts_passing_entity_invariance = contexts_passing_entity_invariance.saturating_add(1);
        if !has_position_invariance(inactive_evidence)
            || !has_position_invariance(external_evidence)
        {
            rejected_missing_or_insufficient_position_diversity =
                rejected_missing_or_insufficient_position_diversity.saturating_add(1);
            continue;
        }
        contexts_passing_position_invariance =
            contexts_passing_position_invariance.saturating_add(1);
        if !has_target_hp_invariance(inactive_evidence)
            || !has_target_hp_invariance(external_evidence)
        {
            rejected_missing_or_insufficient_target_hp_diversity =
                rejected_missing_or_insufficient_target_hp_diversity.saturating_add(1);
            continue;
        }
        contexts_passing_target_hp_invariance =
            contexts_passing_target_hp_invariance.saturating_add(1);
        if active_attack <= inactive_attack || active_damage < inactive_damage {
            rejected_ambiguous_state_outcome = rejected_ambiguous_state_outcome.saturating_add(1);
            continue;
        }

        exact_contexts = exact_contexts.saturating_add(1);
        exact_external_events = exact_external_events.saturating_add(external_evidence.events);
        exact_observed_damage = exact_observed_damage.saturating_add(
            i128::from(*active_damage).saturating_mul(i128::from(external_evidence.events)),
        );
        exact_counterfactual_damage = exact_counterfactual_damage.saturating_add(
            i128::from(*inactive_damage).saturating_mul(i128::from(external_evidence.events)),
        );
        let damage_marginal = active_damage.saturating_sub(*inactive_damage);
        exact_provider_marginal = exact_provider_marginal.saturating_add(
            i128::from(damage_marginal).saturating_mul(i128::from(external_evidence.events)),
        );
        if examples.len() < example_limit {
            examples.push(ArchetypeObservedCounterfactualExample {
                rlog: key.rlog,
                session_id: key.session_id,
                run_ordinal: key.run_ordinal,
                source_entity_uuid: key.source_entity_uuid,
                provider_entity_uuid: *provider,
                target_monster_id: key.target_monster_id,
                target_level: key.target_level,
                ability_id: key.ability_id,
                hit_event_id: key.hit_event_id,
                critical: key.critical,
                lucky: key.lucky,
                inactive_attack: *inactive_attack,
                active_attack: *active_attack,
                inactive_damage: *inactive_damage,
                active_damage: *active_damage,
                provider_attack_marginal: active_attack.saturating_sub(*inactive_attack),
                provider_damage_marginal: damage_marginal,
                inactive_events: inactive_evidence.events,
                external_events: external_evidence.events,
                inactive_target_entities: inactive_evidence.target_entity_uuids.len(),
                external_target_entities: external_evidence.target_entity_uuids.len(),
                inactive_target_hp_values: inactive_evidence.target_current_hp_values.len(),
                external_target_hp_values: external_evidence.target_current_hp_values.len(),
                inactive_packet_positions: inactive_evidence.packet_positions.len(),
                external_packet_positions: external_evidence.packet_positions.len(),
                inactive_hit_part_position_vectors: inactive_evidence.hit_part_positions.len(),
                external_hit_part_position_vectors: external_evidence.hit_part_positions.len(),
                inactive_group_indices: inactive_evidence.skill_effect_group_indices.len(),
                external_group_indices: external_evidence.skill_effect_group_indices.len(),
                first_inactive_sequence: inactive_evidence.first_sequence,
                first_external_sequence: external_evidence.first_sequence,
            });
        }
    }

    ArchetypeObservedCounterfactualReport {
        authority: "exact_empirical_current_build_archetype_pair_only_not_a_generic_formula_or_runtime_rule",
        context_control: "same session, run, source, direct source, monster config and level, action, hit, flags, component identity, stable source attributes, target max HP and packet-observed formula vector, source HP state, and all other source and target statuses; exact target entity UUID, target current HP, occurrence group index, and packet/hit-part positions are varied only by the invariance gate",
        invariance_gate: "inactive and external cohorts must each have one Attack/damage outcome across at least two target entity UUIDs, at least two known packet or complete hit-part positions, and at least two known target-current-HP values; missing evidence rejects the cohort",
        contexts: context_count,
        contexts_with_both_states,
        contexts_with_unique_state_outcomes,
        contexts_passing_entity_invariance,
        contexts_passing_position_invariance,
        contexts_passing_target_hp_invariance,
        exact_contexts,
        rejected_ambiguous_state_outcome,
        rejected_insufficient_entity_diversity,
        rejected_missing_or_insufficient_position_diversity,
        rejected_missing_or_insufficient_target_hp_diversity,
        exact_external_events,
        exact_observed_damage: exact_observed_damage.to_string(),
        exact_counterfactual_damage: exact_counterfactual_damage.to_string(),
        exact_provider_marginal: exact_provider_marginal.to_string(),
        overlap_diagnostics,
        target_state_status_mismatch_examples,
        examples,
    }
}

fn observe_damage_time_attack_state(
    args: &Arguments,
    context: &DamageContext,
    sample: &DamageSample,
    states: &mut BTreeMap<DamageTimeAttackStateContext, DamageTimeAttackStateAccumulator>,
) {
    let provider = match sample.effect_presence {
        EffectPresence::Inactive => None,
        EffectPresence::External(provider) => Some(provider),
        EffectPresence::SelfOwned | EffectPresence::Ambiguous => return,
    };
    let unaffected_formula_attributes = sample
        .formula_attributes
        .iter()
        .filter(|(attribute_id, _)| !FUNCTIONAL_AMP_AFFECTED_ATTRIBUTE_IDS.contains(attribute_id))
        .map(|(&attribute_id, &value)| (attribute_id, value))
        .collect::<Vec<_>>();
    let key = DamageTimeAttackStateContext {
        rlog: sample.rlog.clone(),
        session_id: sample.session_id.clone(),
        run_ordinal: context.run_ordinal,
        source_entity_uuid: context.source_entity_uuid,
        unaffected_formula_attributes,
        other_source_statuses: sample.source_statuses.clone(),
    };
    let state = states.entry(key).or_default();
    if let Some(provider) = provider {
        state.external_damage_events = state.external_damage_events.saturating_add(1);
        let provider_events = state
            .external_damage_events_by_provider
            .entry(provider)
            .or_default();
        *provider_events = provider_events.saturating_add(1);
        let attack_events = state
            .external_damage_events_by_provider_attack
            .entry((provider, sample.attack))
            .or_default();
        *attack_events = attack_events.saturating_add(1);
        if let Some(provider_delta) = args.attack_provider_delta {
            if let Ok(counterfactual_attack) = counterfactual_attack_without_provider_delta(
                &sample.formula_attributes,
                args.attack_attribute_id,
                provider_delta,
            ) {
                state
                    .exact_family_reversals_by_active_attack
                    .entry(sample.attack)
                    .or_default()
                    .insert(counterfactual_attack);
            }
        }
        state
            .external_attacks_by_provider
            .entry(provider)
            .or_default()
            .insert(sample.attack);
        state
            .external_samples_by_provider
            .entry(provider)
            .or_default()
            .push((context.clone(), sample.clone()));
        state
            .first_external_sequence_by_provider
            .entry(provider)
            .or_insert(sample.sequence);
    } else {
        state.inactive_damage_events = state.inactive_damage_events.saturating_add(1);
        state.inactive_attacks.insert(sample.attack);
        state.first_inactive_sequence.get_or_insert(sample.sequence);
    }
}

fn damage_time_attack_state_report(
    args: &Arguments,
    states: BTreeMap<DamageTimeAttackStateContext, DamageTimeAttackStateAccumulator>,
    damage_surface: Option<&DamageSurface>,
) -> DamageTimeAttackStateReport {
    let mut contexts_with_external_damage = 0_u64;
    let mut contexts_with_external_and_inactive_damage = 0_u64;
    let mut exact_reversible_contexts = 0_u64;
    let mut ambiguous_reversible_contexts = 0_u64;
    let mut external_damage_events = 0_u64;
    let mut exact_events = 0_u64;
    let mut events_without_inactive = 0_u64;
    let mut ambiguous_events = 0_u64;
    let mut examples = Vec::new();
    let mut exact_reversible_damage_events = Vec::new();
    let context_count = states.len() as u64;
    let mut actor_surfaces =
        BTreeMap::<ActorAttackSurfaceKey, ActorAttackSurfaceAccumulator>::new();

    for (context, state) in &states {
        let actor = actor_surfaces
            .entry(ActorAttackSurfaceKey {
                rlog: context.rlog.clone(),
                session_id: context.session_id.clone(),
                run_ordinal: context.run_ordinal,
                source_entity_uuid: context.source_entity_uuid,
            })
            .or_default();
        actor
            .inactive_attacks
            .extend(state.inactive_attacks.iter().copied());
        actor.inactive_damage_events = actor
            .inactive_damage_events
            .saturating_add(state.inactive_damage_events);
        for (provider, attacks) in &state.external_attacks_by_provider {
            actor
                .external_attacks_by_provider
                .entry(*provider)
                .or_default()
                .extend(attacks.iter().copied());
        }
        for (provider, events) in &state.external_damage_events_by_provider {
            let total = actor
                .external_damage_events_by_provider
                .entry(*provider)
                .or_default();
            *total = total.saturating_add(*events);
        }
        for (key, events) in &state.external_damage_events_by_provider_attack {
            let total = actor
                .external_damage_events_by_provider_attack
                .entry(*key)
                .or_default();
            *total = total.saturating_add(*events);
        }
        for (active_attack, counterfactual_attacks) in
            &state.exact_family_reversals_by_active_attack
        {
            actor
                .exact_family_reversals_by_active_attack
                .entry(*active_attack)
                .or_default()
                .extend(counterfactual_attacks.iter().copied());
        }
    }

    for (context, state) in states {
        if state.external_damage_events == 0 {
            continue;
        }
        contexts_with_external_damage = contexts_with_external_damage.saturating_add(1);
        external_damage_events =
            external_damage_events.saturating_add(state.external_damage_events);
        if state.inactive_attacks.is_empty() {
            events_without_inactive =
                events_without_inactive.saturating_add(state.external_damage_events);
            continue;
        }
        contexts_with_external_and_inactive_damage =
            contexts_with_external_and_inactive_damage.saturating_add(1);

        for (provider, active_attacks) in &state.external_attacks_by_provider {
            let provider_events = state
                .external_damage_events_by_provider
                .get(provider)
                .copied()
                .unwrap_or_default();
            let exact = match (
                state
                    .inactive_attacks
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .as_slice(),
                active_attacks
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .as_slice(),
            ) {
                ([inactive], [active]) if active > inactive => Some((*inactive, *active)),
                _ => None,
            };
            let Some((inactive_attack, active_attack)) = exact else {
                ambiguous_events = ambiguous_events.saturating_add(provider_events);
                continue;
            };
            exact_events = exact_events.saturating_add(provider_events);
            if let Some(samples) = state.external_samples_by_provider.get(provider) {
                for (damage_context, sample) in samples {
                    if sample.attack != active_attack {
                        continue;
                    }
                    let damage_row_candidates =
                        damage_surface
                            .and_then(|surface| {
                                Some(surface.rows_by_key.get(&(
                                    damage_context.ability_id?,
                                    damage_context.hit_event_id?,
                                )))
                            })
                            .flatten()
                            .cloned()
                            .unwrap_or_default();
                    exact_reversible_damage_events.push(ExactReversibleDamageEvent {
                        rlog: context.rlog.clone(),
                        session_id: context.session_id.clone(),
                        run_ordinal: context.run_ordinal,
                        sequence: sample.sequence,
                        observed_micros: sample.observed_micros,
                        source_entity_uuid: damage_context.source_entity_uuid,
                        direct_source_entity_uuid: damage_context.direct_source_entity_uuid,
                        target_entity_uuid: damage_context.target_entity_uuid,
                        target_monster_id: sample.target_monster_id,
                        target_level: sample.target_level,
                        provider_entity_uuid: *provider,
                        ability_id: damage_context.ability_id,
                        hit_event_id: damage_context.hit_event_id,
                        amount: sample.amount,
                        normal_value: sample.normal_value,
                        lucky_value: sample.lucky_value,
                        actual_value: sample.actual_value,
                        hp_loss: sample.hp_loss,
                        shield_loss: sample.shield_loss,
                        active_attack,
                        counterfactual_attack: inactive_attack,
                        provider_attack_marginal: active_attack.saturating_sub(inactive_attack),
                        damage_source: damage_context.damage_source,
                        damage_type: damage_context.damage_type,
                        critical: damage_context.critical,
                        lucky: damage_context.lucky,
                        causes_lucky: sample.causes_lucky,
                        blocked: damage_context.blocked,
                        periodic: damage_context.periodic,
                        missed: sample.missed,
                        reported_critical: sample.reported_critical,
                        type_flags: sample.type_flags,
                        owner_level: damage_context.owner_level,
                        owner_stage: damage_context.owner_stage,
                        normal_hit: damage_context.normal_hit,
                        property: damage_context.property,
                        hit_part_ids: damage_context.hit_part_ids.clone(),
                        damage_weight_bits: damage_context.damage_weight_bits,
                        passive_uuid: damage_context.passive_uuid,
                        rainbow: damage_context.rainbow,
                        damage_mode: damage_context.damage_mode,
                        skill_effect_uuid: sample.skill_effect_uuid,
                        skill_effect_total_damage: sample.skill_effect_total_damage,
                        skill_effect_group_index: sample.skill_effect_group_index,
                        skill_effect_component_index: sample.skill_effect_component_index,
                        skill_effect_component_count: sample.skill_effect_component_count,
                        hit_part_damage_values: sample.hit_part_damage_values.clone(),
                        source_formula_attributes: formula_attribute_values(
                            &sample.formula_attributes,
                        ),
                        target_formula_attributes: sample
                            .target_formula_attributes
                            .as_deref()
                            .map_or_else(Vec::new, formula_attribute_values),
                        source_statuses: sample.source_statuses.clone(),
                        target_statuses: sample.target_statuses.clone(),
                        damage_row_candidates,
                    });
                }
            }
            if examples.len() < args.example_limit {
                examples.push(DamageTimeAttackStateExample {
                    rlog: context.rlog.clone(),
                    session_id: context.session_id.clone(),
                    run_ordinal: context.run_ordinal,
                    source_entity_uuid: context.source_entity_uuid,
                    provider_entity_uuid: *provider,
                    inactive_attack,
                    active_attack,
                    provider_attack_marginal: active_attack.saturating_sub(inactive_attack),
                    inactive_damage_events: state.inactive_damage_events,
                    external_damage_events: provider_events,
                    first_inactive_sequence: state.first_inactive_sequence,
                    first_external_sequence: state
                        .first_external_sequence_by_provider
                        .get(provider)
                        .copied(),
                    controlled_attribute_count: context.unaffected_formula_attributes.len(),
                    controlled_status_count: context.other_source_statuses.len(),
                });
            }
        }

        let provider_contexts = state.external_attacks_by_provider.len() as u64;
        let exact_provider_contexts = state
            .external_attacks_by_provider
            .values()
            .filter(|active_attacks| {
                state.inactive_attacks.len() == 1
                    && active_attacks.len() == 1
                    && active_attacks.first() > state.inactive_attacks.first()
            })
            .count() as u64;
        exact_reversible_contexts =
            exact_reversible_contexts.saturating_add(exact_provider_contexts);
        ambiguous_reversible_contexts = ambiguous_reversible_contexts
            .saturating_add(provider_contexts.saturating_sub(exact_provider_contexts));
    }

    let mut family_lookup_exact_events = 0_u64;
    let mut family_lookup_missing_events = 0_u64;
    let mut family_lookup_ambiguous_events = 0_u64;
    let mut actor_attack_surfaces = Vec::new();
    for (key, surface) in actor_surfaces {
        if surface.external_attacks_by_provider.is_empty() {
            continue;
        }
        let mut providers = Vec::new();
        for (provider_entity_uuid, active_attacks) in surface.external_attacks_by_provider {
            let external_damage_events = surface
                .external_damage_events_by_provider
                .get(&provider_entity_uuid)
                .copied()
                .unwrap_or_default();
            let mut active_attack_lookups = Vec::new();
            for active_attack in &active_attacks {
                let counterfactual_attacks = surface
                    .exact_family_reversals_by_active_attack
                    .get(active_attack)
                    .cloned()
                    .unwrap_or_default();
                let events = surface
                    .external_damage_events_by_provider_attack
                    .get(&(provider_entity_uuid, *active_attack))
                    .copied()
                    .unwrap_or_default();
                match counterfactual_attacks.len() {
                    1 => {
                        family_lookup_exact_events =
                            family_lookup_exact_events.saturating_add(events)
                    }
                    0 => {
                        family_lookup_missing_events =
                            family_lookup_missing_events.saturating_add(events)
                    }
                    _ => {
                        family_lookup_ambiguous_events =
                            family_lookup_ambiguous_events.saturating_add(events)
                    }
                }
                active_attack_lookups.push(ActiveAttackLookupReport {
                    active_attack: *active_attack,
                    counterfactual_attacks: counterfactual_attacks.into_iter().collect(),
                    external_damage_events: events,
                    exact: surface
                        .exact_family_reversals_by_active_attack
                        .get(active_attack)
                        .is_some_and(|values| values.len() == 1),
                });
            }
            providers.push(ActorAttackProviderSurfaceReport {
                provider_entity_uuid,
                active_attacks: active_attacks.into_iter().collect(),
                external_damage_events,
                active_attack_lookups,
            });
        }
        actor_attack_surfaces.push(ActorAttackSurfaceReport {
            authority: "diagnostic_inventory_only_not_a_causal_counterfactual",
            rlog: key.rlog,
            session_id: key.session_id,
            run_ordinal: key.run_ordinal,
            source_entity_uuid: key.source_entity_uuid,
            inactive_attacks: surface.inactive_attacks.into_iter().collect(),
            inactive_damage_events: surface.inactive_damage_events,
            providers,
        });
    }

    DamageTimeAttackStateReport {
        authority: "offline_packet_observation_only_not_runtime_authority_until_cast_snapshot_timing_is_proven",
        comparison_scope: "same session, run, actor, every other packet-current formula attribute, and every other semantic source status; target and ability are intentionally excluded because this proves the actor Attack state rather than damage",
        excluded_selected_effect_attributes: FUNCTIONAL_AMP_AFFECTED_ATTRIBUTE_IDS.to_vec(),
        contexts: context_count,
        contexts_with_external_damage,
        contexts_with_external_and_inactive_damage,
        exact_reversible_contexts,
        ambiguous_reversible_contexts,
        external_damage_events,
        external_damage_events_with_exact_observed_counterfactual_attack: exact_events,
        external_damage_events_with_unique_run_actor_family_lookup: family_lookup_exact_events,
        external_damage_events_without_run_actor_family_lookup: family_lookup_missing_events,
        external_damage_events_with_ambiguous_run_actor_family_lookup:
            family_lookup_ambiguous_events,
        external_damage_events_without_inactive_context: events_without_inactive,
        external_damage_events_with_ambiguous_context: ambiguous_events,
        exact_reversible_damage_events,
        examples,
        actor_attack_surfaces,
    }
}

fn same_damage_live_vector_conflict(earlier: &DamageSample, later: &DamageSample) -> bool {
    earlier.amount == later.amount
        && earlier.effect_presence == later.effect_presence
        && earlier.source_statuses == later.source_statuses
        && earlier.target_statuses == later.target_statuses
        && earlier.formula_attributes != later.formula_attributes
}

fn observe_live_vector_conflict(
    context: &DamageContext,
    earlier: &DamageSample,
    later: &DamageSample,
    accumulator: &mut LiveVectorStabilityAccumulator,
    example_limit: usize,
) {
    accumulator.conflicting_pairs = accumulator.conflicting_pairs.saturating_add(1);
    if accumulator.examples.len() >= example_limit {
        return;
    }
    let (selected_effect_presence, provider_entity_uuid) = match later.effect_presence {
        EffectPresence::Inactive => ("inactive", None),
        EffectPresence::External(provider) => ("external", Some(provider)),
        EffectPresence::SelfOwned => ("self_owned", Some(context.source_entity_uuid)),
        EffectPresence::Ambiguous => ("ambiguous", None),
    };
    accumulator.examples.push(LiveVectorSnapshotConflict {
        rlog: later.rlog.clone(),
        session_id: later.session_id.clone(),
        run_ordinal: context.run_ordinal,
        source_entity_uuid: context.source_entity_uuid,
        target_entity_uuid: context.target_entity_uuid,
        ability_id: context.ability_id,
        critical: context.critical,
        lucky: context.lucky,
        selected_effect_presence,
        provider_entity_uuid,
        earlier_sequence: earlier.sequence,
        later_sequence: later.sequence,
        identical_damage: later.amount,
        changed_source_attributes: formula_attribute_deltas(
            &earlier.formula_attributes,
            &later.formula_attributes,
        ),
    });
}

fn formula_attribute_deltas(
    inactive: &BTreeMap<i32, i64>,
    active: &BTreeMap<i32, i64>,
) -> Vec<AttributeVectorDelta> {
    inactive
        .keys()
        .chain(active.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|attribute_id| {
            let inactive_value = inactive.get(&attribute_id).copied();
            let active_value = active.get(&attribute_id).copied();
            (inactive_value != active_value).then_some(AttributeVectorDelta {
                attribute_id,
                inactive_value,
                active_value,
            })
        })
        .collect()
}

fn pair_packet_input_deltas(inactive: &DamageSample, active: &DamageSample) -> Vec<&'static str> {
    let mut deltas = Vec::new();
    macro_rules! retain_delta {
        ($field:ident) => {
            if inactive.$field != active.$field {
                deltas.push(stringify!($field));
            }
        };
    }
    retain_delta!(missed);
    retain_delta!(reported_critical);
    retain_delta!(type_flags);
    retain_delta!(causes_lucky);
    retain_delta!(skill_effect_uuid);
    retain_delta!(skill_effect_group_index);
    retain_delta!(skill_effect_component_index);
    retain_delta!(skill_effect_component_count);
    retain_delta!(packet_position);
    retain_delta!(hit_part_positions);
    retain_delta!(target_monster_id);
    retain_delta!(target_level);
    retain_delta!(source_current_hp);
    retain_delta!(source_max_hp);
    retain_delta!(target_current_hp);
    retain_delta!(target_max_hp);
    deltas
}

fn target_formula_attributes_match(inactive: &DamageSample, active: &DamageSample) -> bool {
    matches!(
        (
            &inactive.target_formula_attributes,
            &active.target_formula_attributes,
        ),
        (Some(inactive_target), Some(active_target))
            if !inactive_target.is_empty()
                && !active_target.is_empty()
                && inactive_target == active_target
    )
}

fn target_formula_attribute_deltas(
    inactive: &DamageSample,
    active: &DamageSample,
) -> Vec<AttributeVectorDelta> {
    match (
        &inactive.target_formula_attributes,
        &active.target_formula_attributes,
    ) {
        (Some(inactive_target), Some(active_target)) => {
            formula_attribute_deltas(inactive_target, active_target)
        }
        _ => Vec::new(),
    }
}

fn offensive_vector_factor(
    context: &DamageContext,
    sample: &DamageSample,
    include_mastery: bool,
    include_external_damage: bool,
    include_versatility: bool,
) -> Option<i128> {
    // These two fields are a packet-proven input/output alias in the current
    // build. Refuse any future caller that accidentally treats them as two
    // independent multiplier stages.
    if include_external_damage && include_versatility {
        return None;
    }
    if sample.attack <= 0 {
        return None;
    }
    let mut factor = i128::from(sample.attack);
    if context.critical {
        let raw = sample.critical_damage?;
        let critical_factor = FIXED_POINT_SCALE.checked_add(i128::from(raw))?;
        if critical_factor <= 0 {
            return None;
        }
        factor = factor.checked_mul(critical_factor)?;
    }
    if context.lucky {
        let lucky_factor = i128::from(sample.lucky_damage?);
        if lucky_factor <= 0 {
            return None;
        }
        factor = factor.checked_mul(lucky_factor)?;
    }
    if include_mastery {
        let mastery_factor = FIXED_POINT_SCALE.checked_add(i128::from(sample.mastery?))?;
        if mastery_factor <= 0 {
            return None;
        }
        factor = factor.checked_mul(mastery_factor)?;
    }
    if include_external_damage {
        let external_factor = FIXED_POINT_SCALE.checked_add(i128::from(sample.external_damage?))?;
        if external_factor <= 0 {
            return None;
        }
        factor = factor.checked_mul(external_factor)?;
    }
    if include_versatility {
        let versatility_factor = FIXED_POINT_SCALE.checked_add(i128::from(sample.versatility?))?;
        if versatility_factor <= 0 {
            return None;
        }
        factor = factor.checked_mul(versatility_factor)?;
    }
    Some(factor)
}

fn mul_div_floor_i128(value: i64, numerator: i128, denominator: i128) -> Option<i64> {
    if value < 0 || numerator < 0 || denominator <= 0 {
        return None;
    }
    let result = i128::from(value)
        .checked_mul(numerator)?
        .checked_div(denominator)?;
    i64::try_from(result).ok()
}

fn mul_div_floor(value: i64, numerator: i64, denominator: i64) -> Option<i64> {
    if value < 0 || numerator < 0 || denominator <= 0 {
        return None;
    }
    let result = i128::from(value)
        .checked_mul(i128::from(numerator))?
        .checked_div(i128::from(denominator))?;
    i64::try_from(result).ok()
}

fn counterfactual_interval(
    observed_active: i64,
    inactive_attack: i64,
    active_attack: i64,
) -> Option<(i64, i64)> {
    if observed_active < 0 || inactive_attack < 0 || active_attack <= 0 {
        return None;
    }
    let minimum = i128::from(observed_active)
        .checked_mul(i128::from(inactive_attack))?
        .checked_div(i128::from(active_attack))?;
    let upper_numerator = i128::from(observed_active)
        .checked_add(1)?
        .checked_mul(i128::from(inactive_attack))?;
    let maximum = ceil_div(upper_numerator, i128::from(active_attack))?.checked_sub(1)?;
    Some((i64::try_from(minimum).ok()?, i64::try_from(maximum).ok()?))
}

fn fixed_point_factor_interval(output: i64, body: i64) -> Option<(i64, i64)> {
    if output < 0 || body <= 0 {
        return None;
    }
    let output = i128::from(output);
    let body = i128::from(body);
    let denominator = 10_000_i128;
    let lower = ceil_div(output.checked_mul(denominator)?, body)?;
    let upper = output
        .checked_add(1)?
        .checked_mul(denominator)?
        .checked_sub(1)?
        .checked_div(body)?;
    (lower <= upper && lower >= 0 && upper <= i128::from(i64::MAX))
        .then_some((lower as i64, upper as i64))
}

fn counterfactual_range_for_factor_interval(
    counterfactual_body: i64,
    minimum_factor: i64,
    maximum_factor: i64,
) -> Option<(i64, i64)> {
    if counterfactual_body < 0 || minimum_factor < 0 || maximum_factor < minimum_factor {
        return None;
    }
    Some((
        mul_div_floor(counterfactual_body, minimum_factor, 10_000)?,
        mul_div_floor(counterfactual_body, maximum_factor, 10_000)?,
    ))
}

fn shared_post_base_factor_report(
    groups: &BTreeMap<SharedPostBaseFactorStateKey, Vec<SharedPostBaseFactorObservation>>,
    example_limit: usize,
    authority: &'static str,
    state_identity: &'static str,
) -> SharedPostBaseFactorReport {
    let mut groups_with_multiple_events = 0_u64;
    let mut compatible_groups = 0_u64;
    let mut disjoint_groups = 0_u64;
    let mut unique_factor_groups = 0_u64;
    let mut events_in_compatible_groups = 0_u64;
    let mut events_with_one_counterfactual = 0_u64;
    let mut newly_resolved_events = 0_u64;
    let mut newly_resolved_damage = 0_i128;
    let mut newly_resolved_marginal = 0_i128;
    let mut compatible_examples = Vec::new();
    let mut disjoint_examples = Vec::new();

    for (key, observations) in groups {
        if observations.len() < 2 {
            continue;
        }
        groups_with_multiple_events = groups_with_multiple_events.saturating_add(1);
        let minimum = observations
            .iter()
            .map(|observation| observation.minimum_factor)
            .max()
            .unwrap_or(0);
        let maximum = observations
            .iter()
            .map(|observation| observation.maximum_factor)
            .min()
            .unwrap_or(-1);
        let compatible = minimum <= maximum;
        if compatible {
            compatible_groups = compatible_groups.saturating_add(1);
            events_in_compatible_groups = events_in_compatible_groups
                .saturating_add(u64::try_from(observations.len()).unwrap_or(u64::MAX));
            if minimum == maximum {
                unique_factor_groups = unique_factor_groups.saturating_add(1);
            }
            for observation in observations {
                let shared_range = counterfactual_range_for_factor_interval(
                    observation.counterfactual_base,
                    minimum,
                    maximum,
                );
                let individual_range = counterfactual_range_for_factor_interval(
                    observation.counterfactual_base,
                    observation.minimum_factor,
                    observation.maximum_factor,
                );
                if let Some((shared_minimum, shared_maximum)) = shared_range {
                    if shared_minimum == shared_maximum {
                        events_with_one_counterfactual =
                            events_with_one_counterfactual.saturating_add(1);
                        if individual_range.is_some_and(
                            |(individual_minimum, individual_maximum)| {
                                individual_minimum != individual_maximum
                            },
                        ) {
                            newly_resolved_events = newly_resolved_events.saturating_add(1);
                            newly_resolved_damage = newly_resolved_damage
                                .saturating_add(i128::from(observation.observed_damage));
                            newly_resolved_marginal =
                                newly_resolved_marginal.saturating_add(i128::from(
                                    observation.observed_damage.saturating_sub(shared_minimum),
                                ));
                        }
                    }
                }
            }
        } else {
            disjoint_groups = disjoint_groups.saturating_add(1);
        }

        let examples = if compatible {
            &mut compatible_examples
        } else {
            &mut disjoint_examples
        };
        if examples.len() < example_limit {
            let distinct_actions = observations
                .iter()
                .map(|observation| {
                    format!("{}:{}", observation.ability_id, observation.hit_event_id)
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            examples.push(SharedPostBaseFactorExample {
                session_id: key.session_id.clone(),
                run_ordinal: key.run_ordinal,
                source_entity_uuid: key.source_entity_uuid,
                target_entity_uuid: key.target_entity_uuid,
                event_count: observations.len(),
                distinct_actions,
                shared_factor_minimum_basis_points: compatible.then_some(minimum),
                shared_factor_maximum_basis_points: compatible.then_some(maximum),
                observations: observations
                    .iter()
                    .take(example_limit)
                    .map(|observation| {
                        let shared_range = compatible.then(|| {
                            counterfactual_range_for_factor_interval(
                                observation.counterfactual_base,
                                minimum,
                                maximum,
                            )
                        });
                        SharedPostBaseFactorObservationReport {
                            sequence: observation.sequence,
                            ability_id: observation.ability_id,
                            hit_event_id: observation.hit_event_id,
                            active_base: observation.active_base,
                            counterfactual_base: observation.counterfactual_base,
                            observed_damage: observation.observed_damage,
                            individual_factor_minimum_basis_points: observation.minimum_factor,
                            individual_factor_maximum_basis_points: observation.maximum_factor,
                            shared_interval_counterfactual_minimum: shared_range
                                .flatten()
                                .map(|range| range.0),
                            shared_interval_counterfactual_maximum: shared_range
                                .flatten()
                                .map(|range| range.1),
                        }
                    })
                    .collect(),
            });
        }
    }

    SharedPostBaseFactorReport {
        authority,
        state_identity,
        groups: u64::try_from(groups.len()).unwrap_or(u64::MAX),
        groups_with_multiple_events,
        multi_event_groups_with_compatible_intersection: compatible_groups,
        multi_event_groups_with_disjoint_intersection: disjoint_groups,
        multi_event_groups_with_unique_factor: unique_factor_groups,
        events_in_compatible_multi_event_groups: events_in_compatible_groups,
        events_with_one_counterfactual_from_shared_interval: events_with_one_counterfactual,
        events_newly_resolved_from_shared_interval: newly_resolved_events,
        observed_damage_newly_resolved_from_shared_interval: newly_resolved_damage.to_string(),
        provider_marginal_newly_resolved_from_shared_interval: newly_resolved_marginal.to_string(),
        compatible_examples,
        disjoint_examples,
    }
}

fn load_damage_surface(
    path: &Path,
    expected_game_build: Option<&str>,
) -> Result<DamageSurface, Box<dyn std::error::Error>> {
    let surface: Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let (mut parsed, source_kind) = if surface.get("rules").and_then(Value::as_array).is_some() {
        (
            damage_surface_from_runtime_catalog(&surface)?,
            "damage_stage_runtime_candidate_catalog",
        )
    } else {
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
            let Ok(ability_id) = ability.parse::<i64>() else {
                continue;
            };
            let Ok(hit_event_id) = hit.parse::<i32>() else {
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
                candidates.push(damage_attr_row_from_surface_value(damage_id, row)?);
            }
            rows_by_key.insert((ability_id, hit_event_id), candidates);
        }
        (
            DamageSurface {
                identity: DamageSurfaceIdentity::default(),
                rows_by_key,
            },
            "raw_damage_formula_surface",
        )
    };
    let game_build = json_scalar_string(surface.get("game_build"));
    let expected_game_build = expected_game_build
        .ok_or("--damage-surface requires --expected-game-build for fail-closed build identity")?;
    if game_build.as_deref() != Some(expected_game_build) {
        return Err(format!(
            "damage surface game_build {:?} does not match expected build {expected_game_build}",
            game_build
        )
        .into());
    }
    let metadata = path.metadata()?;
    parsed.identity = DamageSurfaceIdentity {
        path: path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string(),
        bytes: metadata.len(),
        sha256: sha256_file(path)?,
        source_kind: source_kind.to_owned(),
        game_build,
        schema_version: surface.get("schema_version").and_then(Value::as_u64),
        generated_by: surface
            .get("generated_by")
            .and_then(Value::as_str)
            .map(str::to_owned),
        build_identity_verified: true,
    };
    Ok(parsed)
}

fn json_scalar_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn damage_surface_from_runtime_catalog(surface: &Value) -> Result<DamageSurface, String> {
    let rules = surface
        .get("rules")
        .and_then(Value::as_array)
        .ok_or_else(|| "damage-stage catalog is missing rules".to_owned())?;
    let mut rows_by_key = BTreeMap::new();
    for rule in rules {
        let ability_id = rule
            .get("ability_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| "damage-stage catalog rule is missing ability_id".to_owned())?;
        let hit_event_id = rule
            .get("hit_event_id")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| "damage-stage catalog rule is missing hit_event_id".to_owned())?;
        let damage_id = match rule.get("damage_attr_id") {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) => value.to_string(),
            _ => {
                return Err("damage-stage catalog rule is missing damage_attr_id".to_owned());
            }
        };
        let required_damage_source = rule
            .get("damage_source")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok());
        let damage_type = rule
            .get("damage_type")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok());
        let damage_script = rule
            .get("damage_script")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let pve_damage_ratio = damage_catalog_integer_array(
            rule.get("coefficient_basis_points_by_stage"),
            "coefficient_basis_points_by_stage",
        )?;
        let pve_fixed_parameter = damage_catalog_integer_array(
            rule.get("fixed_parameter_by_level"),
            "fixed_parameter_by_level",
        )?;
        rows_by_key
            .entry((ability_id, hit_event_id))
            .or_insert_with(Vec::new)
            .push(DamageAttrRow {
                authority: "research_only_exact_current_build_candidate_damage_stage_catalog_and_packet_observed_pair_attack_values_not_runtime_promoted",
                damage_id,
                required_damage_source,
                damage_type,
                damage_script,
                pve_damage_ratio,
                pve_fixed_parameter,
            });
    }
    Ok(DamageSurface {
        identity: DamageSurfaceIdentity::default(),
        rows_by_key,
    })
}

fn damage_catalog_integer_array(value: Option<&Value>, field: &str) -> Result<Vec<i64>, String> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("damage-stage catalog rule is missing {field}"))?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| format!("damage-stage catalog {field} value is not an integer"))
        })
        .collect()
}

fn damage_attr_row_from_surface_value(
    damage_id: String,
    row: &Value,
) -> Result<DamageAttrRow, String> {
    let arrays = row
        .get("int_array_pool_1_candidates_by_offset")
        .and_then(Value::as_object);
    Ok(DamageAttrRow {
        authority: "research_only_exact_current_build_raw_damage_surface_candidate_and_packet_observed_pair_attack_values_not_runtime_promoted",
        damage_id,
        required_damage_source: None,
        // The current-build field layout proves +16 is the integer DamageType
        // enum. A string-pool candidate at the same byte offset is only a
        // heuristic collision and must never be interpreted as this field.
        damage_type: row
            .get("aligned_scalars_by_offset")
            .and_then(|value| value.get("16"))
            .and_then(|value| value.get("i32"))
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok()),
        // +24 is independently proven as the DamageScript string pointer.
        damage_script: row
            .get("string_pool_6_candidates_by_offset")
            .and_then(|value| value.get("24"))
            .and_then(|value| value.get("value"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        pve_damage_ratio: damage_surface_array_values(arrays.and_then(|values| values.get("28")))?,
        pve_fixed_parameter: damage_surface_array_values(
            arrays.and_then(|values| values.get("32")),
        )?,
    })
}

fn damage_surface_array_values(value: Option<&Value>) -> Result<Vec<i64>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "damage surface array candidate is missing values".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "damage surface array value is not an integer".to_owned())
        })
        .collect()
}

fn observe_single_event_coverage_gap(
    accumulator: &mut SingleEventCounterfactualAccumulator,
    reason: &'static str,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    context: &DamageContext,
    sample: &DamageSample,
    rows: &[DamageAttrRow],
) {
    let candidate_rows = rows
        .iter()
        .map(|row| {
            format!(
                "{}|required_damage_source={}|damage_type={}|damage_script={}|coefficients={:?}|fixed_count={}",
                row.damage_id,
                row.required_damage_source
                    .map_or_else(|| "<any>".to_owned(), |value| value.to_string()),
                row.damage_type
                    .map_or_else(|| "<missing>".to_owned(), |value| value.to_string()),
                row.damage_script.as_deref().unwrap_or("<missing>"),
                row.pve_damage_ratio,
                row.pve_fixed_parameter.len(),
            )
        })
        .collect();
    let key = SingleEventCoverageGapKey {
        reason,
        ability_id,
        hit_event_id,
        owner_level: context.owner_level,
        owner_stage: context.owner_stage,
        property: semantic_damage_property(context.property),
        critical: context.critical,
        lucky: context.lucky,
        candidate_rows,
    };
    let gap = accumulator.coverage_gaps.entry(key).or_default();
    gap.events = gap.events.saturating_add(1);
    gap.observed_damage = gap
        .observed_damage
        .saturating_add(i128::from(sample.amount));
    if gap.examples.len() < DEFAULT_EXAMPLE_LIMIT {
        gap.examples.push(SingleEventCoverageGapExample {
            rlog: sample.rlog.clone(),
            session_id: sample.session_id.clone(),
            sequence: sample.sequence,
            observed_micros: sample.observed_micros,
            run_ordinal: context.run_ordinal,
            source_entity_uuid: context.source_entity_uuid,
            target_entity_uuid: context.target_entity_uuid,
            observed_damage: sample.amount,
            packet_normal_value: sample.normal_value,
            packet_lucky_value: sample.lucky_value,
            skill_effect_uuid: sample.skill_effect_uuid,
            skill_effect_total_damage: sample.skill_effect_total_damage,
            skill_effect_group_index: sample.skill_effect_group_index,
            skill_effect_component_index: sample.skill_effect_component_index,
            skill_effect_component_count: sample.skill_effect_component_count,
            passive_uuid: context.passive_uuid,
            damage_mode: context.damage_mode,
            active_attack: sample.attack,
            source_formula_attributes: formula_attribute_values(&sample.formula_attributes),
            target_formula_attributes: sample
                .target_formula_attributes
                .as_deref()
                .map(formula_attribute_values)
                .unwrap_or_default(),
            source_statuses: sample.source_statuses.clone(),
            target_statuses: sample.target_statuses.clone(),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn observe_single_event_damage_attr_counterfactual(
    args: &Arguments,
    surface: &DamageSurface,
    context: &DamageContext,
    sample: &DamageSample,
    provider: i64,
    provider_delta: AttackProviderDelta,
    required_provider_status_matches: bool,
    accumulator: &mut SingleEventCounterfactualAccumulator,
) {
    accumulator.external_active_damage_events =
        accumulator.external_active_damage_events.saturating_add(1);
    accumulator.observed_damage = accumulator
        .observed_damage
        .saturating_add(i128::from(sample.amount));
    if required_provider_status_matches {
        accumulator.events_matching_required_provider_status = accumulator
            .events_matching_required_provider_status
            .saturating_add(1);
    } else {
        accumulator.events_rejected_by_required_provider_status = accumulator
            .events_rejected_by_required_provider_status
            .saturating_add(1);
        observe_single_event_coverage_gap(
            accumulator,
            "configured_provider_vector_status_gate_not_satisfied",
            context.ability_id,
            context.hit_event_id,
            context,
            sample,
            &[],
        );
        return;
    }
    if let Some(normal_value) = sample.normal_value {
        accumulator.events_with_packet_normal_value = accumulator
            .events_with_packet_normal_value
            .saturating_add(1);
        if normal_value == sample.amount {
            accumulator.events_where_packet_normal_value_matches_amount = accumulator
                .events_where_packet_normal_value_matches_amount
                .saturating_add(1);
        }
    }
    if let Some(required_attribute_ids) =
        attack_reversal_required_attribute_ids(args.attack_attribute_id, provider_delta)
    {
        let mut all_required_components_present = true;
        for attribute_id in required_attribute_ids {
            let coverage = accumulator
                .attack_family_component_coverage
                .entry(attribute_id)
                .or_default();
            if sample.formula_attributes.contains_key(&attribute_id) {
                coverage.events_present = coverage.events_present.saturating_add(1);
            } else {
                coverage.events_missing = coverage.events_missing.saturating_add(1);
                all_required_components_present = false;
            }
        }
        if all_required_components_present {
            accumulator.events_with_all_required_attack_family_components = accumulator
                .events_with_all_required_attack_family_components
                .saturating_add(1);
        }
    }
    let counterfactual_attack = match counterfactual_attack_without_provider_delta(
        &sample.formula_attributes,
        args.attack_attribute_id,
        provider_delta,
    ) {
        Ok(value) => value,
        Err(reason) => {
            observe_single_event_coverage_gap(
                accumulator,
                reason,
                context.ability_id,
                context.hit_event_id,
                context,
                sample,
                &[],
            );
            return;
        }
    };
    accumulator.events_with_exact_attack_family_reversal = accumulator
        .events_with_exact_attack_family_reversal
        .saturating_add(1);
    let Some(ability_id) = context.ability_id else {
        observe_single_event_coverage_gap(
            accumulator,
            "missing_ability_or_hit_identity",
            context.ability_id,
            context.hit_event_id,
            context,
            sample,
            &[],
        );
        return;
    };
    let hit_event_id = semantic_hit_event_id(context.hit_event_id);
    accumulator.events_with_ability_hit_identity = accumulator
        .events_with_ability_hit_identity
        .saturating_add(1);
    if context.hit_event_id.is_some() {
        accumulator.events_with_wire_hit_event_id =
            accumulator.events_with_wire_hit_event_id.saturating_add(1);
    }
    let Some(rows) = surface.rows_by_key.get(&(ability_id, hit_event_id)) else {
        observe_single_event_coverage_gap(
            accumulator,
            "missing_damage_surface_key",
            Some(ability_id),
            Some(hit_event_id),
            context,
            sample,
            &[],
        );
        return;
    };
    if context.hit_event_id.is_none() {
        accumulator.events_using_semantic_zero_for_omitted_hit_event_id = accumulator
            .events_using_semantic_zero_for_omitted_hit_event_id
            .saturating_add(1);
    }
    let [row] = rows.as_slice() else {
        observe_single_event_coverage_gap(
            accumulator,
            "ambiguous_damage_surface_key",
            Some(ability_id),
            Some(hit_event_id),
            context,
            sample,
            rows,
        );
        return;
    };
    accumulator.events_with_unique_damage_row =
        accumulator.events_with_unique_damage_row.saturating_add(1);
    let expected_input = match args.attack_attribute_id {
        11_330 => "Attack",
        11_340 => "MAttack",
        _ => return,
    };
    if row.damage_script.as_deref() != Some(expected_input) {
        accumulator.events_with_unsupported_damage_script = accumulator
            .events_with_unsupported_damage_script
            .saturating_add(1);
        let key = UnsupportedDamageScriptKey {
            ability_id,
            hit_event_id,
            damage_id: row.damage_id.clone(),
            damage_type: row.damage_type,
            damage_script: row
                .damage_script
                .clone()
                .unwrap_or_else(|| "<missing>".to_owned()),
            owner_stage: context.owner_stage,
            critical: context.critical,
            lucky: context.lucky,
            blocked: context.blocked,
            periodic: context.periodic,
        };
        let diagnostic = accumulator
            .unsupported_damage_script_actions
            .entry(key)
            .or_default();
        diagnostic.events = diagnostic.events.saturating_add(1);
        diagnostic.observed_damage = diagnostic
            .observed_damage
            .saturating_add(i128::from(sample.amount));
        observe_single_event_coverage_gap(
            accumulator,
            "unsupported_damage_script",
            Some(ability_id),
            Some(hit_event_id),
            context,
            sample,
            rows,
        );
        return;
    }
    accumulator.events_with_matching_damage_script = accumulator
        .events_with_matching_damage_script
        .saturating_add(1);
    if let Some(damage_type) = row.damage_type.filter(|value| *value != 0) {
        accumulator.events_with_nonzero_damage_type = accumulator
            .events_with_nonzero_damage_type
            .saturating_add(1);
        let key = NonzeroDamageTypeKey {
            ability_id,
            hit_event_id,
            damage_id: row.damage_id.clone(),
            damage_type,
            damage_script: expected_input.to_owned(),
            owner_stage: context.owner_stage,
            critical: context.critical,
            lucky: context.lucky,
            blocked: context.blocked,
            periodic: context.periodic,
        };
        let diagnostic = accumulator
            .nonzero_damage_type_actions
            .entry(key)
            .or_default();
        diagnostic.events = diagnostic.events.saturating_add(1);
        diagnostic.observed_damage = diagnostic
            .observed_damage
            .saturating_add(i128::from(sample.amount));
    }
    let Some(coefficient) = select_stage_coefficient(&row.pve_damage_ratio, context.owner_stage)
    else {
        accumulator.events_missing_stage_coefficient = accumulator
            .events_missing_stage_coefficient
            .saturating_add(1);
        observe_single_event_coverage_gap(
            accumulator,
            "owner_stage_out_of_range_or_empty_coefficient",
            Some(ability_id),
            Some(hit_event_id),
            context,
            sample,
            rows,
        );
        return;
    };
    accumulator.events_with_exact_stage_coefficient = accumulator
        .events_with_exact_stage_coefficient
        .saturating_add(1);
    let fixed = if row.pve_fixed_parameter.is_empty() {
        0
    } else {
        let Some(owner_level) = context.owner_level else {
            observe_single_event_coverage_gap(
                accumulator,
                "missing_owner_level_for_fixed_parameter",
                Some(ability_id),
                Some(hit_event_id),
                context,
                sample,
                rows,
            );
            return;
        };
        let Ok(level) = usize::try_from(owner_level) else {
            observe_single_event_coverage_gap(
                accumulator,
                "invalid_owner_level_for_fixed_parameter",
                Some(ability_id),
                Some(hit_event_id),
                context,
                sample,
                rows,
            );
            return;
        };
        let Some(value) = level
            .checked_sub(1)
            .and_then(|index| row.pve_fixed_parameter.get(index))
        else {
            observe_single_event_coverage_gap(
                accumulator,
                "owner_level_out_of_fixed_parameter_range",
                Some(ability_id),
                Some(hit_event_id),
                context,
                sample,
                rows,
            );
            return;
        };
        *value
    };
    let (Some(active_scaled), Some(counterfactual_scaled)) = (
        mul_div_floor(sample.attack, coefficient, 10_000),
        mul_div_floor(counterfactual_attack, coefficient, 10_000),
    ) else {
        accumulator.events_with_invalid_counterfactual = accumulator
            .events_with_invalid_counterfactual
            .saturating_add(1);
        return;
    };
    let active_base = active_scaled.saturating_add(fixed);
    let counterfactual_base = counterfactual_scaled.saturating_add(fixed);
    accumulator.events_with_base_candidates =
        accumulator.events_with_base_candidates.saturating_add(1);
    let exact_conserved_provider_share =
        sample
            .attack
            .checked_sub(counterfactual_attack)
            .and_then(|provider_attack_marginal| {
                exact_external_attack_coefficient_stage_fraction(
                    sample.amount,
                    PacketDamageScriptFamily::StandardAttack,
                    sample.attack,
                    provider_attack_marginal,
                    coefficient,
                    fixed,
                )
            });
    if let Some((numerator, denominator)) = exact_conserved_provider_share {
        accumulator.events_with_exact_conserved_attack_stage_share = accumulator
            .events_with_exact_conserved_attack_stage_share
            .saturating_add(1);
        accumulator.exact_conserved_share_observed_damage = accumulator
            .exact_conserved_share_observed_damage
            .saturating_add(i128::from(sample.amount));
        let bucket = accumulator
            .exact_conserved_share_buckets
            .entry(denominator)
            .or_default();
        bucket.events = bucket.events.saturating_add(1);
        bucket.observed_damage = bucket
            .observed_damage
            .saturating_add(i128::from(sample.amount));
        bucket.provider_numerator = bucket.provider_numerator.saturating_add(numerator);
    } else {
        observe_single_event_coverage_gap(
            accumulator,
            "invalid_conserved_attack_stage_share",
            Some(ability_id),
            Some(hit_event_id),
            context,
            sample,
            rows,
        );
    }
    if let Some(provider_external_damage_raw_delta) = args.provider_external_damage_raw_delta {
        let combined_share = sample
            .external_damage
            .and_then(|current_external_damage_raw| {
                sample.attack.checked_sub(counterfactual_attack).and_then(
                    |provider_attack_marginal| {
                        exact_external_attack_and_damage_bonus_fraction(
                            sample.amount,
                            PacketDamageScriptFamily::StandardAttack,
                            sample.attack,
                            provider_attack_marginal,
                            coefficient,
                            fixed,
                            current_external_damage_raw,
                            provider_external_damage_raw_delta,
                        )
                    },
                )
            });
        if let Some((numerator, denominator)) = combined_share {
            accumulator.events_with_exact_conserved_attack_external_composite_share = accumulator
                .events_with_exact_conserved_attack_external_composite_share
                .saturating_add(1);
            accumulator.exact_conserved_attack_external_composite_observed_damage = accumulator
                .exact_conserved_attack_external_composite_observed_damage
                .saturating_add(i128::from(sample.amount));
            let bucket = accumulator
                .exact_conserved_attack_external_composite_buckets
                .entry(denominator)
                .or_default();
            bucket.events = bucket.events.saturating_add(1);
            bucket.observed_damage = bucket
                .observed_damage
                .saturating_add(i128::from(sample.amount));
            bucket.provider_numerator = bucket.provider_numerator.saturating_add(numerator);
        } else {
            accumulator.events_without_exact_conserved_attack_external_composite_share =
                accumulator
                    .events_without_exact_conserved_attack_external_composite_share
                    .saturating_add(1);
        }
    }
    if let (
        Some(provider_external_damage_raw_delta),
        Some(provider_property_damage_attribute_id),
        Some(provider_property_damage_raw_delta),
        Some(required_damage_property),
    ) = (
        args.provider_external_damage_raw_delta,
        args.provider_property_damage_attribute_id,
        args.provider_property_damage_raw_delta,
        args.required_damage_property,
    ) {
        if semantic_damage_property(context.property) == required_damage_property {
            accumulator.events_matching_required_damage_property = accumulator
                .events_matching_required_damage_property
                .saturating_add(1);
            let combined_share = sample
                .external_damage
                .zip(
                    sample
                        .formula_attributes
                        .get(&provider_property_damage_attribute_id)
                        .copied(),
                )
                .and_then(
                    |(current_external_damage_raw, current_property_damage_raw)| {
                        let active_external_factor = FIXED_POINT_SCALE
                            .checked_add(i128::from(current_external_damage_raw))?;
                        let active_property_factor = FIXED_POINT_SCALE
                            .checked_add(i128::from(current_property_damage_raw))?;
                        Some((
                            i64::try_from(active_external_factor).ok()?,
                            i64::try_from(active_property_factor).ok()?,
                        ))
                    },
                )
                .and_then(|(active_external_factor, active_property_factor)| {
                    exact_external_composite_damage_fraction(
                        sample.amount,
                        active_base,
                        counterfactual_base,
                        &[
                            (active_external_factor, provider_external_damage_raw_delta),
                            (active_property_factor, provider_property_damage_raw_delta),
                        ],
                    )
                });
            if let Some((numerator, denominator)) = combined_share {
                accumulator.events_with_exact_conserved_attack_external_property_composite_share =
                    accumulator
                        .events_with_exact_conserved_attack_external_property_composite_share
                        .saturating_add(1);
                accumulator.exact_conserved_attack_external_property_composite_observed_damage =
                    accumulator
                        .exact_conserved_attack_external_property_composite_observed_damage
                        .saturating_add(i128::from(sample.amount));
                let bucket = accumulator
                    .exact_conserved_attack_external_property_composite_buckets
                    .entry(denominator)
                    .or_default();
                bucket.events = bucket.events.saturating_add(1);
                bucket.observed_damage = bucket
                    .observed_damage
                    .saturating_add(i128::from(sample.amount));
                bucket.provider_numerator = bucket.provider_numerator.saturating_add(numerator);
            } else {
                accumulator
                    .events_without_exact_conserved_attack_external_property_composite_share =
                    accumulator
                        .events_without_exact_conserved_attack_external_property_composite_share
                        .saturating_add(1);
                observe_single_event_coverage_gap(
                    accumulator,
                    "invalid_or_missing_property_composite_evidence",
                    Some(ability_id),
                    Some(hit_event_id),
                    context,
                    sample,
                    rows,
                );
            }
        } else {
            accumulator.events_rejected_by_required_damage_property = accumulator
                .events_rejected_by_required_damage_property
                .saturating_add(1);
            observe_single_event_coverage_gap(
                accumulator,
                "required_damage_property_not_satisfied",
                Some(ability_id),
                Some(hit_event_id),
                context,
                sample,
                rows,
            );
        }
    }
    let integer_factor_interval = fixed_point_factor_interval(sample.amount, active_base);
    if let Some((minimum_factor, maximum_factor)) = integer_factor_interval {
        let full_state_key = SharedPostBaseFactorStateKey {
            action_identity: None,
            session_id: sample.session_id.clone(),
            run_ordinal: context.run_ordinal,
            source_entity_uuid: context.source_entity_uuid,
            target_entity_uuid: context.target_entity_uuid,
            damage_source: context.damage_source,
            damage_type: context.damage_type,
            critical: context.critical,
            lucky: context.lucky,
            blocked: context.blocked,
            periodic: context.periodic,
            causes_lucky: sample.causes_lucky,
            missed: sample.missed,
            reported_critical: sample.reported_critical,
            type_flags: sample.type_flags,
            owner_level: context.owner_level,
            owner_stage: context.owner_stage,
            normal_hit: context.normal_hit,
            property: context.property,
            passive_uuid: context.passive_uuid,
            rainbow: context.rainbow,
            damage_mode: context.damage_mode,
            hit_part_ids: context.hit_part_ids.clone(),
            damage_weight_bits: context.damage_weight_bits,
            skill_effect_uuid: sample.skill_effect_uuid,
            skill_effect_group_index: sample.skill_effect_group_index,
            skill_effect_component_index: sample.skill_effect_component_index,
            skill_effect_component_count: sample.skill_effect_component_count,
            packet_position: sample.packet_position,
            hit_part_positions: sample.hit_part_positions.clone(),
            source_current_hp: sample.source_current_hp,
            source_max_hp: sample.source_max_hp,
            target_current_hp: sample.target_current_hp,
            target_max_hp: sample.target_max_hp,
            source_formula_attributes: sample
                .formula_attributes
                .iter()
                .map(|(attribute_id, value)| (*attribute_id, *value))
                .collect(),
            target_formula_attributes: sample.target_formula_attributes.as_ref().map(
                |attributes| {
                    attributes
                        .iter()
                        .map(|(attribute_id, value)| (*attribute_id, *value))
                        .collect()
                },
            ),
            source_statuses: sample.source_statuses.clone(),
            target_statuses: sample.target_statuses.clone(),
        };
        let observation = SharedPostBaseFactorObservation {
            sequence: sample.sequence,
            ability_id,
            hit_event_id,
            active_base,
            counterfactual_base,
            observed_damage: sample.amount,
            minimum_factor,
            maximum_factor,
        };
        accumulator
            .shared_post_base_factor_groups
            .entry(full_state_key.clone())
            .or_default()
            .push(observation.clone());
        let mut position_relaxed_state_key = full_state_key.clone();
        position_relaxed_state_key.packet_position = None;
        position_relaxed_state_key.hit_part_positions.clear();
        accumulator
            .position_relaxed_post_base_factor_groups
            .entry(position_relaxed_state_key.clone())
            .or_default()
            .push(observation.clone());
        let mut position_current_hp_relaxed_state_key = position_relaxed_state_key;
        position_current_hp_relaxed_state_key.source_current_hp = None;
        position_current_hp_relaxed_state_key.target_current_hp = None;
        position_current_hp_relaxed_state_key
            .source_formula_attributes
            .retain(|(attribute_id, _)| *attribute_id != CURRENT_HP_ATTRIBUTE_ID);
        if let Some(attributes) = position_current_hp_relaxed_state_key
            .target_formula_attributes
            .as_mut()
        {
            attributes.retain(|(attribute_id, _)| *attribute_id != CURRENT_HP_ATTRIBUTE_ID);
        }
        accumulator
            .position_current_hp_relaxed_post_base_factor_groups
            .entry(position_current_hp_relaxed_state_key.clone())
            .or_default()
            .push(observation.clone());
        let mut position_current_hp_component_relaxed_state_key =
            position_current_hp_relaxed_state_key;
        position_current_hp_component_relaxed_state_key.skill_effect_uuid = None;
        position_current_hp_component_relaxed_state_key.skill_effect_group_index = None;
        position_current_hp_component_relaxed_state_key.skill_effect_component_index = None;
        position_current_hp_component_relaxed_state_key.skill_effect_component_count = None;
        accumulator
            .position_current_hp_component_relaxed_post_base_factor_groups
            .entry(position_current_hp_component_relaxed_state_key.clone())
            .or_default()
            .push(observation.clone());

        let mut action_position_component_relaxed_state_key = full_state_key.clone();
        action_position_component_relaxed_state_key.action_identity =
            Some((ability_id, hit_event_id, row.damage_id.clone()));
        action_position_component_relaxed_state_key.packet_position = None;
        action_position_component_relaxed_state_key
            .hit_part_positions
            .clear();
        action_position_component_relaxed_state_key.skill_effect_uuid = None;
        action_position_component_relaxed_state_key.skill_effect_group_index = None;
        action_position_component_relaxed_state_key.skill_effect_component_index = None;
        action_position_component_relaxed_state_key.skill_effect_component_count = None;
        accumulator
            .action_position_component_relaxed_post_base_factor_groups
            .entry(action_position_component_relaxed_state_key.clone())
            .or_default()
            .push(observation.clone());

        let mut action_position_current_hp_component_relaxed_state_key =
            action_position_component_relaxed_state_key;
        action_position_current_hp_component_relaxed_state_key.source_current_hp = None;
        action_position_current_hp_component_relaxed_state_key.target_current_hp = None;
        action_position_current_hp_component_relaxed_state_key
            .source_formula_attributes
            .retain(|(attribute_id, _)| *attribute_id != CURRENT_HP_ATTRIBUTE_ID);
        if let Some(attributes) = action_position_current_hp_component_relaxed_state_key
            .target_formula_attributes
            .as_mut()
        {
            attributes.retain(|(attribute_id, _)| *attribute_id != CURRENT_HP_ATTRIBUTE_ID);
        }
        accumulator
            .action_position_current_hp_component_relaxed_post_base_factor_groups
            .entry(action_position_current_hp_component_relaxed_state_key)
            .or_default()
            .push(observation);
    }
    let integer_factor_counterfactual_range =
        integer_factor_interval.and_then(|(minimum, maximum)| {
            if minimum == maximum {
                accumulator.events_with_unique_integer_post_base_factor = accumulator
                    .events_with_unique_integer_post_base_factor
                    .saturating_add(1);
            }
            counterfactual_range_for_factor_interval(counterfactual_base, minimum, maximum)
        });
    let diagnostic_key = SingleEventDiagnosticKey {
        ability_id,
        hit_event_id,
        damage_id: row.damage_id.clone(),
        damage_script: expected_input.to_owned(),
        owner_stage: context.owner_stage,
        critical: context.critical,
        lucky: context.lucky,
        blocked: context.blocked,
        periodic: context.periodic,
    };
    let diagnostic = accumulator
        .diagnostics_by_action
        .entry(diagnostic_key)
        .or_default();
    diagnostic.events = diagnostic.events.saturating_add(1);
    let target_identity = diagnostic
        .target_identities
        .entry((context.target_entity_uuid, sample.target_monster_id))
        .or_default();
    target_identity.events = target_identity.events.saturating_add(1);
    if let Some(target_attributes) = &sample.target_formula_attributes {
        diagnostic.events_with_target_attribute_snapshot = diagnostic
            .events_with_target_attribute_snapshot
            .saturating_add(1);
        if target_attributes.contains_key(&DEFENSE_ATTRIBUTE_ID) {
            diagnostic.events_with_target_physical_defense = diagnostic
                .events_with_target_physical_defense
                .saturating_add(1);
        }
        if target_attributes.contains_key(&MAGIC_DEFENSE_ATTRIBUTE_ID) {
            diagnostic.events_with_target_magical_defense = diagnostic
                .events_with_target_magical_defense
                .saturating_add(1);
        }
        if target_attributes.contains_key(&SEASON_LEVEL_ATTRIBUTE_ID) {
            diagnostic.events_with_target_season_level =
                diagnostic.events_with_target_season_level.saturating_add(1);
        }
        if target_attributes.contains_key(&SEASON_STRENGTH_ATTRIBUTE_ID)
            || target_attributes.contains_key(&SEASON_WEAKNESS_ATTRIBUTE_ID)
        {
            diagnostic.events_with_target_season_strength_or_weakness = diagnostic
                .events_with_target_season_strength_or_weakness
                .saturating_add(1);
        }
        for attribute_id in target_attributes.keys() {
            let coverage = accumulator
                .target_attribute_coverage
                .entry(*attribute_id)
                .or_default();
            coverage.events_present = coverage.events_present.saturating_add(1);
            if integer_factor_interval.is_some() {
                coverage.events_with_integer_factor_interval = coverage
                    .events_with_integer_factor_interval
                    .saturating_add(1);
            } else {
                coverage.events_without_integer_factor_interval = coverage
                    .events_without_integer_factor_interval
                    .saturating_add(1);
            }
        }
    }
    if integer_factor_interval.is_some() {
        target_identity.events_with_integer_factor_interval = target_identity
            .events_with_integer_factor_interval
            .saturating_add(1);
        diagnostic.events_with_integer_factor_interval = diagnostic
            .events_with_integer_factor_interval
            .saturating_add(1);
        accumulator.events_with_integer_post_base_factor_interval = accumulator
            .events_with_integer_post_base_factor_interval
            .saturating_add(1);
        if let Some((minimum, maximum)) = integer_factor_counterfactual_range {
            if minimum == maximum {
                diagnostic.events_with_one_counterfactual_across_factor_interval = diagnostic
                    .events_with_one_counterfactual_across_factor_interval
                    .saturating_add(1);
                accumulator.events_with_one_counterfactual_across_integer_factor_interval =
                    accumulator
                        .events_with_one_counterfactual_across_integer_factor_interval
                        .saturating_add(1);
            } else {
                diagnostic.events_with_multiple_counterfactuals_across_factor_interval = diagnostic
                    .events_with_multiple_counterfactuals_across_factor_interval
                    .saturating_add(1);
                accumulator.events_with_multiple_counterfactuals_across_integer_factor_interval =
                    accumulator
                        .events_with_multiple_counterfactuals_across_integer_factor_interval
                        .saturating_add(1);
            }
        }
    } else {
        target_identity.events_without_integer_factor_interval = target_identity
            .events_without_integer_factor_interval
            .saturating_add(1);
        diagnostic.events_without_integer_factor_interval = diagnostic
            .events_without_integer_factor_interval
            .saturating_add(1);
        accumulator.events_without_integer_post_base_factor_interval = accumulator
            .events_without_integer_post_base_factor_interval
            .saturating_add(1);
    }
    let Some((minimum, maximum)) =
        counterfactual_interval(sample.amount, counterfactual_base, active_base)
    else {
        accumulator.events_with_invalid_counterfactual = accumulator
            .events_with_invalid_counterfactual
            .saturating_add(1);
        return;
    };
    let exact = (minimum == maximum).then_some(minimum);
    let marginal = exact.and_then(|value| sample.amount.checked_sub(value));
    if let Some(counterfactual) = exact {
        accumulator.events_with_one_exact_counterfactual_across_all_candidates = accumulator
            .events_with_one_exact_counterfactual_across_all_candidates
            .saturating_add(1);
        accumulator.exact_counterfactual_damage = accumulator
            .exact_counterfactual_damage
            .saturating_add(i128::from(counterfactual));
        if let Some(marginal) = marginal {
            accumulator.exact_provider_marginal = accumulator
                .exact_provider_marginal
                .saturating_add(i128::from(marginal));
        }
    } else {
        accumulator.events_with_ambiguous_counterfactual = accumulator
            .events_with_ambiguous_counterfactual
            .saturating_add(1);
    }
    if accumulator.examples.len() < args.example_limit {
        accumulator.examples.push(SingleEventCounterfactualExample {
            rlog: sample.rlog.clone(),
            session_id: sample.session_id.clone(),
            sequence: sample.sequence,
            run_ordinal: context.run_ordinal,
            source_entity_uuid: context.source_entity_uuid,
            provider_entity_uuid: provider,
            target_entity_uuid: context.target_entity_uuid,
            target_monster_id: sample.target_monster_id,
            target_level: sample.target_level,
            ability_id,
            hit_event_id,
            damage_id: row.damage_id.clone(),
            damage_script: expected_input.to_owned(),
            owner_level: context.owner_level,
            owner_stage: context.owner_stage,
            damage_type: context.damage_type,
            property: context.property,
            critical: context.critical,
            lucky: context.lucky,
            damage_weight_bits: context.damage_weight_bits,
            damage_weight_values: context
                .damage_weight_bits
                .map(|(first, second)| (first.map(f32::from_bits), second.map(f32::from_bits))),
            observed_damage: sample.amount,
            packet_normal_value: sample.normal_value,
            packet_actual_value: sample.actual_value,
            active_attack: sample.attack,
            counterfactual_attack,
            selected_coefficient: coefficient,
            fixed_parameter_candidates: if row.pve_fixed_parameter.is_empty() {
                vec![0]
            } else {
                vec![fixed]
            },
            exact_conserved_provider_share_numerator: exact_conserved_provider_share
                .map(|value| value.0.to_string()),
            exact_conserved_provider_share_denominator: exact_conserved_provider_share
                .map(|value| value.1.to_string()),
            counterfactual_minimum: minimum,
            counterfactual_maximum: maximum,
            integer_post_base_factor_minimum_basis_points: integer_factor_interval
                .map(|value| value.0),
            integer_post_base_factor_maximum_basis_points: integer_factor_interval
                .map(|value| value.1),
            integer_factor_counterfactual_minimum: integer_factor_counterfactual_range
                .map(|value| value.0),
            integer_factor_counterfactual_maximum: integer_factor_counterfactual_range
                .map(|value| value.1),
            exact_integer_factor_counterfactual: integer_factor_counterfactual_range
                .and_then(|(minimum, maximum)| (minimum == maximum).then_some(minimum)),
            exact_counterfactual: exact,
            exact_provider_marginal: marginal,
            source_formula_attributes: formula_attribute_values(&sample.formula_attributes),
            target_formula_attributes: sample
                .target_formula_attributes
                .as_deref()
                .map_or_else(Vec::new, formula_attribute_values),
            source_statuses: sample.source_statuses.clone(),
            target_statuses: sample.target_statuses.clone(),
        });
    }
}

fn attack_reversal_required_attribute_ids(
    final_attack_attribute_id: i32,
    provider_delta: AttackProviderDelta,
) -> Option<Vec<i32>> {
    if provider_delta.is_final_attack() {
        return matches!(final_attack_attribute_id, 11_330 | 11_340)
            .then_some(vec![final_attack_attribute_id]);
    }
    let (add_id, extra_add_id, percent_id) = match final_attack_attribute_id {
        11_330 => (11_332, 11_333, 11_334),
        11_340 => (11_342, 11_343, 11_344),
        _ => return None,
    };
    let mut required = vec![final_attack_attribute_id, add_id, extra_add_id, percent_id];
    if let AttackProviderDelta::DerivedPrimaryPercent {
        primary_attribute_id,
        ..
    } = provider_delta
    {
        for offset in 0..=5 {
            required.push(primary_attribute_id.checked_add(offset)?);
        }
    }
    required.sort_unstable();
    required.dedup();
    Some(required)
}

fn counterfactual_attack_without_provider_delta(
    attributes: &BTreeMap<i32, i64>,
    final_attack_attribute_id: i32,
    provider_delta: AttackProviderDelta,
) -> Result<i64, &'static str> {
    if provider_delta.raw_delta() <= 0 {
        return Err("attack_family_reversal_provider_delta_not_positive");
    }
    let (add_id, extra_add_id, percent_id) = match final_attack_attribute_id {
        11_330 => (11_332, 11_333, 11_334),
        11_340 => (11_342, 11_343, 11_344),
        _ => return Err("attack_family_reversal_unsupported_final_attack_attribute"),
    };
    let current = attributes
        .get(&final_attack_attribute_id)
        .copied()
        .ok_or("attack_family_reversal_missing_final_attack_current")?;
    if let AttackProviderDelta::FinalAttack(delta) = provider_delta {
        let counterfactual = current
            .checked_sub(delta)
            .ok_or("attack_family_reversal_final_attack_delta_underflow")?;
        return (counterfactual >= 0)
            .then_some(counterfactual)
            .ok_or("attack_family_reversal_negative_counterfactual_final_attack");
    }
    let add = attributes
        .get(&add_id)
        .copied()
        .ok_or("attack_family_reversal_missing_final_attack_add")?;
    let extra_add = attributes
        .get(&extra_add_id)
        .copied()
        .ok_or("attack_family_reversal_missing_final_attack_extra_add")?;
    let percent = attributes
        .get(&percent_id)
        .copied()
        .ok_or("attack_family_reversal_missing_final_attack_percent")?;
    let current_replay = mul_div_floor(
        add,
        10_000_i64
            .checked_add(percent)
            .ok_or("attack_family_reversal_final_attack_arithmetic_overflow")?,
        10_000,
    )
    .ok_or("attack_family_reversal_final_attack_arithmetic_overflow")?
    .checked_add(extra_add)
    .ok_or("attack_family_reversal_final_attack_arithmetic_overflow")?;
    if current_replay != current {
        return Err("attack_family_reversal_active_final_attack_replay_mismatch");
    }
    let (counterfactual_add, counterfactual_percent) = match provider_delta {
        AttackProviderDelta::FinalAttack(_) => unreachable!("handled above"),
        AttackProviderDelta::BaseAdd(delta) => (
            add.checked_sub(delta)
                .ok_or("attack_family_reversal_base_add_underflow")?,
            percent,
        ),
        AttackProviderDelta::RawPercent(delta) => (
            add,
            percent
                .checked_sub(delta)
                .ok_or("attack_family_reversal_raw_percent_underflow")?,
        ),
        AttackProviderDelta::DerivedPrimaryPercent {
            primary_attribute_id,
            raw_percent_delta,
            attack_add_numerator,
            attack_add_denominator,
        } => {
            if attack_add_numerator <= 0 || attack_add_denominator <= 0 {
                return Err("attack_family_reversal_invalid_primary_to_attack_ratio");
            }
            let primary_total_id = primary_attribute_id
                .checked_add(1)
                .ok_or("attack_family_reversal_primary_attribute_id_overflow")?;
            let primary_add_id = primary_attribute_id
                .checked_add(2)
                .ok_or("attack_family_reversal_primary_attribute_id_overflow")?;
            let primary_extra_add_id = primary_attribute_id
                .checked_add(3)
                .ok_or("attack_family_reversal_primary_attribute_id_overflow")?;
            let primary_percent_id = primary_attribute_id
                .checked_add(4)
                .ok_or("attack_family_reversal_primary_attribute_id_overflow")?;
            let primary_extra_percent_id = primary_attribute_id
                .checked_add(5)
                .ok_or("attack_family_reversal_primary_attribute_id_overflow")?;
            let primary_current = attributes
                .get(&primary_attribute_id)
                .copied()
                .ok_or("attack_family_reversal_missing_primary_current")?;
            let primary_total = attributes
                .get(&primary_total_id)
                .copied()
                .ok_or("attack_family_reversal_missing_primary_total")?;
            let primary_add = attributes
                .get(&primary_add_id)
                .copied()
                .ok_or("attack_family_reversal_missing_primary_add")?;
            let primary_extra_add = attributes
                .get(&primary_extra_add_id)
                .copied()
                .ok_or("attack_family_reversal_missing_primary_extra_add")?;
            let primary_percent = attributes
                .get(&primary_percent_id)
                .copied()
                .ok_or("attack_family_reversal_missing_primary_percent")?;
            let primary_extra_percent = attributes
                .get(&primary_extra_percent_id)
                .copied()
                .ok_or("attack_family_reversal_missing_primary_extra_percent")?;
            let counterfactual_primary_percent = primary_percent
                .checked_sub(raw_percent_delta)
                .ok_or("attack_family_reversal_primary_percent_underflow")?;
            if counterfactual_primary_percent < 0 || primary_extra_percent < 0 {
                return Err("attack_family_reversal_negative_primary_percent_component");
            }
            let replayed_primary_total = mul_div_floor(
                primary_add,
                10_000_i64
                    .checked_add(primary_percent)
                    .ok_or("attack_family_reversal_primary_arithmetic_overflow")?,
                10_000,
            )
            .ok_or("attack_family_reversal_primary_arithmetic_overflow")?;
            let replayed_primary_current = mul_div_floor(
                replayed_primary_total,
                10_000_i64
                    .checked_add(primary_extra_percent)
                    .ok_or("attack_family_reversal_primary_arithmetic_overflow")?,
                10_000,
            )
            .ok_or("attack_family_reversal_primary_arithmetic_overflow")?
            .checked_add(primary_extra_add)
            .ok_or("attack_family_reversal_primary_arithmetic_overflow")?;
            if replayed_primary_total != primary_total
                || replayed_primary_current != primary_current
            {
                return Err("attack_family_reversal_active_primary_replay_mismatch");
            }
            let counterfactual_primary_total = mul_div_floor(
                primary_add,
                10_000_i64
                    .checked_add(counterfactual_primary_percent)
                    .ok_or("attack_family_reversal_primary_arithmetic_overflow")?,
                10_000,
            )
            .ok_or("attack_family_reversal_primary_arithmetic_overflow")?;
            let counterfactual_primary_current = mul_div_floor(
                counterfactual_primary_total,
                10_000_i64
                    .checked_add(primary_extra_percent)
                    .ok_or("attack_family_reversal_primary_arithmetic_overflow")?,
                10_000,
            )
            .ok_or("attack_family_reversal_primary_arithmetic_overflow")?
            .checked_add(primary_extra_add)
            .ok_or("attack_family_reversal_primary_arithmetic_overflow")?;
            let active_primary_attack_component = mul_div_floor(
                primary_current,
                attack_add_numerator,
                attack_add_denominator,
            )
            .ok_or("attack_family_reversal_primary_to_attack_arithmetic_overflow")?;
            let counterfactual_primary_attack_component = mul_div_floor(
                counterfactual_primary_current,
                attack_add_numerator,
                attack_add_denominator,
            )
            .ok_or("attack_family_reversal_primary_to_attack_arithmetic_overflow")?;
            let counterfactual_add = add
                .checked_sub(active_primary_attack_component)
                .ok_or("attack_family_reversal_derived_attack_add_underflow")?
                .checked_add(counterfactual_primary_attack_component)
                .ok_or("attack_family_reversal_derived_attack_add_overflow")?;
            (counterfactual_add, percent)
        }
    };
    if counterfactual_add < 0 || counterfactual_percent < 0 {
        return Err("attack_family_reversal_negative_counterfactual_component");
    }
    mul_div_floor(
        counterfactual_add,
        10_000_i64
            .checked_add(counterfactual_percent)
            .ok_or("attack_family_reversal_counterfactual_arithmetic_overflow")?,
        10_000,
    )
    .ok_or("attack_family_reversal_counterfactual_arithmetic_overflow")?
    .checked_add(extra_add)
    .ok_or("attack_family_reversal_counterfactual_arithmetic_overflow")
}

fn select_stage_coefficient(coefficients: &[i64], owner_stage: Option<i32>) -> Option<i64> {
    if coefficients.len() == 1 {
        return (owner_stage.unwrap_or_default() >= 0)
            .then_some(coefficients[0])
            .filter(|value| *value > 0);
    }
    let stage = owner_stage.unwrap_or_default();
    let index = usize::try_from(stage).ok()?;
    coefficients.get(index).copied().filter(|value| *value > 0)
}

fn semantic_hit_event_id(hit_event_id: Option<i32>) -> i32 {
    hit_event_id.unwrap_or_default()
}

fn semantic_damage_property(property: Option<i32>) -> i32 {
    property.unwrap_or_default()
}

fn ceil_div(numerator: i128, denominator: i128) -> Option<i128> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    numerator
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)
}

static EMPTY_STATUS_TRACKER: StatusTracker = StatusTracker {
    active: BTreeMap::new(),
};

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
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
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index >= 10 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown.rlog")
        .to_owned()
}

fn parse_args(
    values: impl IntoIterator<Item = OsString>,
) -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    let effect_id = parse_i64(take_value(&mut values, "--effect")?, "--effect")?;
    let source_config_id = parse_i64(
        take_value(&mut values, "--source-config")?,
        "--source-config",
    )?;
    if source_config_id < 0 {
        return Err(
            "--source-config must be positive, or zero when the canonical source config is absent"
                .into(),
        );
    }
    let attack_attribute_id = parse_i32(
        take_value(&mut values, "--attack-attribute")?,
        "--attack-attribute",
    )?;
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let rlogs = take_values(&mut values, "--rlog")
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if rlogs.is_empty() {
        return Err(usage().into());
    }
    let max_gap_micros = take_optional_value(&mut values, "--max-gap-micros")
        .map(|value| parse_u64(value, "--max-gap-micros"))
        .transpose()?
        .unwrap_or(DEFAULT_MAX_GAP_MICROS);
    let example_limit = take_optional_value(&mut values, "--example-limit")
        .map(|value| parse_usize(value, "--example-limit"))
        .transpose()?
        .unwrap_or(DEFAULT_EXAMPLE_LIMIT);
    let diagnostic_ignored_status_ids = take_values(&mut values, "--diagnostic-ignore-status")
        .into_iter()
        .map(|value| parse_i64(value, "--diagnostic-ignore-status"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let damage_surface = take_optional_value(&mut values, "--damage-surface").map(PathBuf::from);
    let expected_game_build = take_optional_value(&mut values, "--expected-game-build")
        .map(|value| value.to_string_lossy().into_owned());
    if damage_surface.is_some() && expected_game_build.is_none() {
        return Err(
            "--damage-surface requires --expected-game-build for fail-closed build identity".into(),
        );
    }
    if damage_surface.is_none() && expected_game_build.is_some() {
        return Err("--expected-game-build requires --damage-surface".into());
    }
    let attack_percent_delta = take_optional_value(&mut values, "--attack-percent-delta")
        .map(|value| parse_i64(value, "--attack-percent-delta"))
        .transpose()?;
    let attack_base_add_delta = take_optional_value(&mut values, "--attack-base-add-delta")
        .map(|value| parse_i64(value, "--attack-base-add-delta"))
        .transpose()?;
    let final_attack_delta = take_optional_value(&mut values, "--final-attack-delta")
        .map(|value| parse_i64(value, "--final-attack-delta"))
        .transpose()?;
    let provider_external_damage_raw_delta =
        take_optional_value(&mut values, "--provider-external-damage-raw-delta")
            .map(|value| parse_i64(value, "--provider-external-damage-raw-delta"))
            .transpose()?;
    if provider_external_damage_raw_delta.is_some_and(|value| value <= 0) {
        return Err("--provider-external-damage-raw-delta must be positive".into());
    }
    let provider_property_damage_attribute_id =
        take_optional_value(&mut values, "--provider-property-damage-attribute")
            .map(|value| parse_i32(value, "--provider-property-damage-attribute"))
            .transpose()?;
    let provider_property_damage_raw_delta =
        take_optional_value(&mut values, "--provider-property-damage-raw-delta")
            .map(|value| parse_i64(value, "--provider-property-damage-raw-delta"))
            .transpose()?;
    let required_damage_property = take_optional_value(&mut values, "--required-damage-property")
        .map(|value| parse_i32(value, "--required-damage-property"))
        .transpose()?;
    match (
        provider_property_damage_attribute_id,
        provider_property_damage_raw_delta,
        required_damage_property,
    ) {
        (None, None, None) => {}
        (Some(attribute_id), Some(raw_delta), Some(property))
            if attribute_id > 0 && raw_delta > 0 && property > 0 => {}
        _ => {
            return Err("positive --provider-property-damage-attribute, --provider-property-damage-raw-delta, and --required-damage-property values must be supplied together".into());
        }
    }
    let required_provider_status_effect =
        take_optional_value(&mut values, "--required-provider-status-effect")
            .map(|value| parse_i64(value, "--required-provider-status-effect"))
            .transpose()?;
    let required_provider_status_source_config =
        take_optional_value(&mut values, "--required-provider-status-source-config")
            .map(|value| parse_i64(value, "--required-provider-status-source-config"))
            .transpose()?;
    let required_provider_status_state =
        take_optional_value(&mut values, "--required-provider-status-state")
            .map(|value| value.to_string_lossy().into_owned());
    let required_provider_status = match (
        required_provider_status_effect,
        required_provider_status_source_config,
    ) {
        (None, None) if required_provider_status_state.is_none() => None,
        (Some(effect_id), Some(source_config_id)) if effect_id > 0 && source_config_id > 0 => {
            let expected_active = match required_provider_status_state.as_deref() {
                None | Some("active") => true,
                Some("absent") => false,
                Some(value) => {
                    return Err(format!(
                        "--required-provider-status-state must be active or absent, got {value}"
                    )
                    .into());
                }
            };
            Some(RequiredProviderStatus {
                effect_id,
                source_config_id,
                expected_active,
            })
        }
        _ => {
            return Err("--required-provider-status-effect and --required-provider-status-source-config must be supplied together with positive values".into());
        }
    };
    let primary_attribute_id = take_optional_value(&mut values, "--primary-attribute")
        .map(|value| parse_i32(value, "--primary-attribute"))
        .transpose()?;
    let primary_percent_delta = take_optional_value(&mut values, "--primary-percent-delta")
        .map(|value| parse_i64(value, "--primary-percent-delta"))
        .transpose()?;
    let primary_to_attack_numerator =
        take_optional_value(&mut values, "--primary-to-attack-numerator")
            .map(|value| parse_i64(value, "--primary-to-attack-numerator"))
            .transpose()?;
    let primary_to_attack_denominator =
        take_optional_value(&mut values, "--primary-to-attack-denominator")
            .map(|value| parse_i64(value, "--primary-to-attack-denominator"))
            .transpose()?;
    let derived_primary = match (
        primary_attribute_id,
        primary_percent_delta,
        primary_to_attack_numerator,
        primary_to_attack_denominator,
    ) {
        (None, None, None, None) => None,
        (Some(attribute_id), Some(delta), Some(numerator), Some(denominator))
            if attribute_id > 0 && delta > 0 && numerator > 0 && denominator > 0 =>
        {
            Some(AttackProviderDelta::DerivedPrimaryPercent {
                primary_attribute_id: attribute_id,
                raw_percent_delta: delta,
                attack_add_numerator: numerator,
                attack_add_denominator: denominator,
            })
        }
        _ => {
            return Err("derived primary-stat reversal requires positive --primary-attribute, --primary-percent-delta, --primary-to-attack-numerator, and --primary-to-attack-denominator values together".into());
        }
    };
    let selected_delta_count = [
        final_attack_delta.is_some(),
        attack_base_add_delta.is_some(),
        attack_percent_delta.is_some(),
        derived_primary.is_some(),
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if selected_delta_count > 1 {
        return Err(
            "final Attack, Attack base-add, Attack percent, and derived primary-stat provider deltas are mutually exclusive".into(),
        );
    }
    let attack_provider_delta = if let Some(delta) = final_attack_delta {
        Some(
            (delta > 0)
                .then_some(AttackProviderDelta::FinalAttack(delta))
                .ok_or("the selected final Attack provider delta must be positive")?,
        )
    } else if let Some(delta) = attack_base_add_delta {
        Some(
            (delta > 0)
                .then_some(AttackProviderDelta::BaseAdd(delta))
                .ok_or("the selected Attack base-add provider delta must be positive")?,
        )
    } else if let Some(delta) = attack_percent_delta {
        Some(
            (delta > 0)
                .then_some(AttackProviderDelta::RawPercent(delta))
                .ok_or("the selected Attack percent provider delta must be positive")?,
        )
    } else {
        derived_primary
    };
    if attack_provider_delta.is_some() && damage_surface.is_none() {
        return Err(
            "an Attack-family provider delta requires --damage-surface; --damage-surface may be supplied alone for strict-pair damage-stage diagnostics"
                .into(),
        );
    }
    if provider_external_damage_raw_delta.is_some()
        && (damage_surface.is_none() || attack_provider_delta.is_none())
    {
        return Err(
            "--provider-external-damage-raw-delta requires --damage-surface and one Attack-family provider delta"
                .into(),
        );
    }
    if provider_property_damage_attribute_id.is_some()
        && (provider_external_damage_raw_delta.is_none() || damage_surface.is_none())
    {
        return Err("a property-specific provider delta requires --provider-external-damage-raw-delta, --damage-surface, and one Attack-family provider delta".into());
    }
    if required_provider_status.is_some() && damage_surface.is_none() {
        return Err(
            "a required provider status requires --damage-surface and one fixed provider vector"
                .into(),
        );
    }
    let source_entity_uuid = take_optional_value(&mut values, "--source-entity-uuid")
        .map(|value| parse_i64(value, "--source-entity-uuid"))
        .transpose()?;
    if source_entity_uuid.is_some_and(|value| value <= 0) {
        return Err("--source-entity-uuid must be positive".into());
    }
    let transition_seeds =
        take_optional_value(&mut values, "--transition-seeds").map(PathBuf::from);
    let transition_window_micros = take_optional_value(&mut values, "--transition-window-micros")
        .map(|value| parse_u64(value, "--transition-window-micros"))
        .transpose()?
        .unwrap_or(2_000_000);
    let effective_stat_windows =
        take_optional_value(&mut values, "--effective-stat-windows").map(PathBuf::from);
    if transition_seeds.is_some() && source_entity_uuid.is_none() {
        return Err("--transition-seeds requires --source-entity-uuid".into());
    }
    if attack_provider_delta.is_some_and(AttackProviderDelta::is_final_attack)
        && transition_seeds.is_none()
    {
        return Err(
            "--final-attack-delta requires complete occurrence-scoped --transition-seeds and --source-entity-uuid"
                .into(),
        );
    }
    let pair_proof_only = take_switch(&mut values, "--pair-proof-only");
    if diagnostic_ignored_status_ids.contains(&effect_id) {
        return Err("the selected effect cannot also be a diagnostic ignored status".into());
    }
    if !values.is_empty() {
        return Err(format!("unrecognized arguments: {values:?}\n{}", usage()).into());
    }
    Ok(Arguments {
        effect_id,
        source_config_id,
        attack_attribute_id,
        rlogs,
        output,
        max_gap_micros,
        example_limit,
        diagnostic_ignored_status_ids,
        damage_surface,
        expected_game_build,
        attack_provider_delta,
        provider_external_damage_raw_delta,
        provider_property_damage_attribute_id,
        provider_property_damage_raw_delta,
        required_damage_property,
        required_provider_status,
        source_entity_uuid,
        transition_seeds,
        transition_window_micros,
        effective_stat_windows,
        pair_proof_only,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag).ok_or_else(|| format!("missing {flag}\n{}", usage()))
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return None;
    }
    values.remove(position);
    Some(values.remove(position))
}

fn take_switch(values: &mut Vec<OsString>, flag: &str) -> bool {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return false;
    };
    values.remove(position);
    true
}

fn take_values(values: &mut Vec<OsString>, flag: &str) -> Vec<OsString> {
    let mut found = Vec::new();
    while let Some(value) = take_optional_value(values, flag) {
        found.push(value);
    }
    found
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

fn parse_u64(value: OsString, flag: &str) -> Result<u64, String> {
    value
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|error| format!("invalid {flag}: {error}"))
}

fn parse_usize(value: OsString, flag: &str) -> Result<usize, String> {
    value
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|error| format!("invalid {flag}: {error}"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-external-attack-damage-proof --effect <status-id> --source-config <id-or-0-when-absent> --attack-attribute <id> --rlog <current-decoder.rlog> [--rlog <current-decoder.rlog> ...] --output <audit.json> [--damage-surface <exact-packet-build.json> --expected-game-build <id> [--final-attack-delta <occurrence-proven-raw-value> | --attack-base-add-delta <raw-value> | --attack-percent-delta <basis-points> | (--primary-attribute <id> --primary-percent-delta <basis-points> --primary-to-attack-numerator <n> --primary-to-attack-denominator <d>)] [--provider-external-damage-raw-delta <packet-proven-raw-value> [--provider-property-damage-attribute <id> --provider-property-damage-raw-delta <packet-proven-raw-value> --required-damage-property <enum>]] [--required-provider-status-effect <id> --required-provider-status-source-config <id> [--required-provider-status-state <active|absent>]]] [--source-entity-uuid <uuid> [--transition-seeds <complete-seeds.json> --transition-window-micros <micros>]] [--effective-stat-windows <exact-lifecycle-proof.json>] [--pair-proof-only] [--max-gap-micros <micros>] [--example-limit <count>] [--diagnostic-ignore-status <status-id> ...]".to_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::Arc,
    };

    use super::{
        AttackProviderDelta, DamageAttrRow, DamageContext, DamageSample, DamageSurface,
        EffectPresence, EffectiveStatClassification, EffectiveStatWindow,
        EffectiveStatWindowFilter, OFFENSIVE_VECTOR_FORMULAS, SemanticStatusEntry, StatusKey,
        attack_reversal_required_attribute_ids, ceil_div,
        counterfactual_attack_without_provider_delta, counterfactual_interval,
        counterfactual_range_for_factor_interval, damage_attr_row_from_surface_value,
        damage_surface_from_runtime_catalog, ensure_selected_status_lifecycle,
        fixed_point_factor_interval, gate_effect_presence_by_effective_stat_window, mul_div_floor,
        pair_damage_stage_proof, pair_packet_input_deltas, select_stage_coefficient,
        selected_status_key_matches, semantic_damage_property, semantic_hit_event_id,
        semantic_snapshots_equal_ignoring, target_formula_attributes_match,
    };

    fn effective_window_filter() -> EffectiveStatWindowFilter {
        EffectiveStatWindowFilter {
            source: "test.json".to_owned(),
            game_build: "24687926".to_owned(),
            exact_lifecycle_windows: 1,
            candidate_status_window_damage_actions: 3,
            effective_stat_window_damage_actions: 1,
            excluded_before_attribute_activation: 1,
            excluded_at_or_after_attribute_deactivation: 1,
            windows: BTreeMap::from([(
                ("test".to_owned(), 1, 20),
                vec![EffectiveStatWindow {
                    provider_entity_uuid: 10,
                    application_sequence: 100,
                    removal_sequence: 200,
                    first_exclusive_sequence: 110,
                    last_exclusive_sequence: 190,
                }],
            )]),
        }
    }

    #[test]
    fn effective_stat_window_excludes_status_only_lead_and_tail() {
        let filter = effective_window_filter();
        assert_eq!(
            filter.classify("test", 1, 20, 105),
            EffectiveStatClassification::SelectedLifecycleOutsideEffectiveWindow
        );
        assert_eq!(
            filter.classify("test", 1, 20, 111),
            EffectiveStatClassification::EffectiveExternal(10)
        );
        assert_eq!(
            filter.classify("test", 1, 20, 195),
            EffectiveStatClassification::SelectedLifecycleOutsideEffectiveWindow
        );
    }

    #[test]
    fn effective_stat_window_requires_matching_packet_provider_lifecycle() {
        let filter = effective_window_filter();
        assert_eq!(
            gate_effect_presence_by_effective_stat_window(
                EffectPresence::External(10),
                Some(&filter),
                "test",
                1,
                20,
                111,
            ),
            EffectPresence::External(10)
        );
        assert_eq!(
            gate_effect_presence_by_effective_stat_window(
                EffectPresence::External(11),
                Some(&filter),
                "test",
                1,
                20,
                111,
            ),
            EffectPresence::Ambiguous
        );
        assert_eq!(
            gate_effect_presence_by_effective_stat_window(
                EffectPresence::External(10),
                Some(&filter),
                "test",
                1,
                20,
                105,
            ),
            EffectPresence::Ambiguous
        );
    }

    #[test]
    fn damage_type_uses_numeric_field_and_ignores_same_offset_string_collision() {
        let row = serde_json::json!({
            "aligned_scalars_by_offset": {
                "16": { "i32": 2 }
            },
            "string_pool_6_candidates_by_offset": {
                "16": { "value": "MstSpSkillAttack" },
                "24": { "value": "AttackLucky" }
            },
            "int_array_pool_1_candidates_by_offset": {
                "28": { "values": [] },
                "32": { "values": [] }
            }
        });

        let decoded = damage_attr_row_from_surface_value("2203110903".to_owned(), &row)
            .expect("current-build DamageAttr surface row should decode");
        assert_eq!(decoded.damage_type, Some(2));
        assert_eq!(decoded.damage_script.as_deref(), Some("AttackLucky"));
    }

    #[test]
    fn offensive_formula_candidates_never_double_count_versatility_aliases() {
        for (name, _include_mastery, include_ext_damage, include_versatility) in
            OFFENSIVE_VECTOR_FORMULAS
        {
            assert!(
                !(include_ext_damage && include_versatility),
                "{name} compounds AttrVersatilityPct with derived AttrExtDamInc"
            );
        }
    }

    fn damage_sample_with_target(
        target_formula_attributes: Option<BTreeMap<i32, i64>>,
    ) -> DamageSample {
        DamageSample {
            rlog: "test.rlog".to_owned(),
            session_id: "test".to_owned(),
            sequence: 1,
            observed_micros: 1,
            amount: 1,
            normal_value: Some(1),
            lucky_value: None,
            actual_value: None,
            hp_loss: None,
            shield_loss: None,
            causes_lucky: None,
            missed: None,
            reported_critical: None,
            type_flags: None,
            skill_effect_uuid: None,
            skill_effect_total_damage: None,
            skill_effect_group_index: None,
            skill_effect_component_index: None,
            skill_effect_component_count: None,
            hit_part_damage_values: Vec::new(),
            packet_position: None,
            hit_part_positions: Vec::new(),
            target_monster_id: None,
            target_level: None,
            source_current_hp: None,
            source_max_hp: None,
            target_current_hp: None,
            target_max_hp: None,
            attack: 1,
            critical_damage: None,
            lucky_damage: None,
            external_damage: None,
            mastery: None,
            versatility: None,
            formula_attributes: Arc::new(BTreeMap::new()),
            target_formula_attributes: target_formula_attributes.map(Arc::new),
            effect_presence: EffectPresence::Inactive,
            source_statuses: Vec::new(),
            target_statuses: Vec::new(),
        }
    }

    #[test]
    fn strict_pair_damage_stage_share_uses_observed_attack_delta_and_exact_row() {
        let mut surface = DamageSurface::default();
        surface.identity.build_identity_verified = true;
        surface.rows_by_key.insert(
            (2_203_531, 1),
            vec![DamageAttrRow {
                authority: "test_current_build_candidate",
                damage_id: "2220353101".to_owned(),
                required_damage_source: None,
                damage_type: Some(2),
                damage_script: Some("Attack".to_owned()),
                pve_damage_ratio: vec![30_000],
                pve_fixed_parameter: vec![40],
            }],
        );
        let context = DamageContext {
            run_ordinal: 1,
            source_entity_uuid: 216_009_015_936,
            direct_source_entity_uuid: None,
            raw_attacker_uuid: None,
            raw_top_summoner_uuid: None,
            raw_owner_id: None,
            target_entity_uuid: 7_108_755_520,
            ability_id: Some(2_203_531),
            hit_event_id: Some(1),
            damage_source: None,
            damage_type: Some(2),
            critical: true,
            lucky: false,
            blocked: false,
            periodic: false,
            owner_level: Some(1),
            owner_stage: Some(0),
            normal_hit: None,
            property: Some(0),
            hit_part_ids: Vec::new(),
            damage_weight_bits: None,
            passive_uuid: None,
            rainbow: None,
            damage_mode: None,
        };
        let mut inactive = damage_sample_with_target(None);
        inactive.amount = 94_420;
        inactive.attack = 7_182;
        let mut active = damage_sample_with_target(None);
        active.amount = 126_983;
        active.attack = 10_166;

        let proof = pair_damage_stage_proof(11_330, &surface, &context, &inactive, &active)
            .expect("strict pair and exact standard row should resolve");
        assert_eq!(proof.active_base, 30_538);
        assert_eq!(proof.inactive_base, 21_586);
        assert_eq!(proof.provider_base_marginal, 8_952);
        assert_eq!(
            proof.exact_conserved_attack_stage_share_numerator,
            "568375908"
        );
        assert_eq!(
            proof.exact_conserved_attack_stage_share_denominator,
            "15269"
        );
        assert!(!proof.paired_inactive_matches_attack_only_counterfactual);
    }

    #[test]
    fn current_build_damage_stage_catalog_retains_script_and_source_route() {
        let catalog = serde_json::json!({
            "rules": [{
                "ability_id": 2203531,
                "hit_event_id": 1,
                "damage_source": 2,
                "damage_attr_id": 2_220_353_101_u64,
                "damage_type": 2,
                "damage_script": "Attack",
                "coefficient_basis_points_by_stage": [30000],
                "fixed_parameter_by_level": [40]
            }]
        });

        let surface = damage_surface_from_runtime_catalog(&catalog).unwrap();
        let rows = surface.rows_by_key.get(&(2_203_531, 1)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].required_damage_source, Some(2));
        assert_eq!(rows[0].damage_script.as_deref(), Some("Attack"));
        assert_eq!(rows[0].pve_damage_ratio, vec![30_000]);
        assert_eq!(rows[0].pve_fixed_parameter, vec![40]);
    }

    #[test]
    fn ratio_prediction_uses_integer_floor() {
        assert_eq!(mul_div_floor(1000, 1036, 1000), Some(1036));
        assert_eq!(mul_div_floor(1001, 1036, 1000), Some(1037));
    }

    #[test]
    fn counterfactual_interval_can_be_unique() {
        assert_eq!(
            counterfactual_interval(1036, 1000, 1036),
            Some((1000, 1000))
        );
    }

    #[test]
    fn integer_factor_interval_inverts_one_floor_stage() {
        assert_eq!(
            fixed_point_factor_interval(150, 100),
            Some((15_000, 15_099))
        );
        assert_eq!(
            fixed_point_factor_interval(15_000, 10_000),
            Some((15_000, 15_000))
        );
    }

    #[test]
    fn integer_factor_counterfactual_requires_all_factors_to_agree() {
        assert_eq!(
            counterfactual_range_for_factor_interval(95, 15_000, 15_000),
            Some((142, 142))
        );
        assert_eq!(
            counterfactual_range_for_factor_interval(100, 15_000, 15_099),
            Some((150, 150))
        );
        assert_eq!(
            counterfactual_range_for_factor_interval(10_000, 15_000, 15_099),
            Some((15_000, 15_099))
        );
    }

    #[test]
    fn counterfactual_interval_retains_zero_as_the_exact_lower_result() {
        assert_eq!(counterfactual_interval(1, 1, 3), Some((0, 0)));
    }

    #[test]
    fn functional_amp_attack_family_reversal_uses_percent_before_extra_add() {
        let attributes = BTreeMap::from([
            (11_330, 5_569),
            (11_332, 4_620),
            (11_333, 210),
            (11_334, 1_600),
        ]);
        assert_eq!(
            counterfactual_attack_without_provider_delta(
                &attributes,
                11_330,
                AttackProviderDelta::RawPercent(360),
            ),
            Ok(5_402)
        );
        assert_eq!(
            counterfactual_attack_without_provider_delta(
                &attributes,
                11_330,
                AttackProviderDelta::BaseAdd(360),
            ),
            Ok(5_151)
        );
    }

    #[test]
    fn occurrence_scoped_final_attack_delta_needs_only_packet_current_attack() {
        let attributes = BTreeMap::from([(11_330, 8_072)]);
        assert_eq!(
            counterfactual_attack_without_provider_delta(
                &attributes,
                11_330,
                AttackProviderDelta::FinalAttack(346),
            ),
            Ok(7_726)
        );
        assert_eq!(
            attack_reversal_required_attribute_ids(11_330, AttackProviderDelta::FinalAttack(346),),
            Some(vec![11_330])
        );
    }

    #[test]
    fn derived_primary_percent_reversal_preserves_both_integer_floor_stages() {
        let attributes = BTreeMap::from([
            (11_030, 7_856),
            (11_031, 7_160),
            (11_032, 6_120),
            (11_033, 696),
            (11_034, 1_700),
            (11_035, 0),
            (11_330, 6_527),
            (11_332, 4_983),
            (11_333, 0),
            (11_334, 3_100),
        ]);
        assert_eq!(
            counterfactual_attack_without_provider_delta(
                &attributes,
                11_330,
                AttackProviderDelta::DerivedPrimaryPercent {
                    primary_attribute_id: 11_030,
                    raw_percent_delta: 200,
                    attack_add_numerator: 58,
                    attack_add_denominator: 100,
                },
            ),
            Ok(6_434)
        );
    }

    #[test]
    fn attack_family_reversal_does_not_zero_fill_unobserved_components() {
        let attributes = BTreeMap::from([
            (11_030, 7_856),
            (11_031, 7_160),
            (11_032, 6_120),
            (11_033, 696),
            (11_034, 1_700),
            (11_035, 0),
            (11_330, 6_527),
            (11_332, 4_983),
            (11_334, 3_100),
        ]);
        assert_eq!(
            counterfactual_attack_without_provider_delta(
                &attributes,
                11_330,
                AttackProviderDelta::DerivedPrimaryPercent {
                    primary_attribute_id: 11_030,
                    raw_percent_delta: 200,
                    attack_add_numerator: 58,
                    attack_add_denominator: 100,
                },
            ),
            Err("attack_family_reversal_missing_final_attack_extra_add")
        );
        assert_eq!(
            attack_reversal_required_attribute_ids(
                11_330,
                AttackProviderDelta::DerivedPrimaryPercent {
                    primary_attribute_id: 11_030,
                    raw_percent_delta: 200,
                    attack_add_numerator: 58,
                    attack_add_denominator: 100,
                },
            ),
            Some(vec![
                11_030, 11_031, 11_032, 11_033, 11_034, 11_035, 11_330, 11_332, 11_333, 11_334,
            ])
        );
    }

    #[test]
    fn zero_source_config_selector_matches_only_canonical_absence() {
        let absent = StatusKey {
            effect_id: 3_003_052,
            instance_id: None,
            source_entity_uuid: Some(40_581_726_848),
            source_config_id: None,
        };
        let present = StatusKey {
            source_config_id: Some(3_003_053),
            ..absent.clone()
        };
        assert!(selected_status_key_matches(&absent, 3_003_052, 0));
        assert!(!selected_status_key_matches(&present, 3_003_052, 0));
        assert!(selected_status_key_matches(&present, 3_003_052, 3_003_053));
    }

    #[test]
    fn omitted_damage_property_has_protobuf_general_zero_semantics() {
        assert_eq!(semantic_damage_property(None), 0);
        assert_eq!(semantic_damage_property(Some(0)), 0);
        assert_eq!(semantic_damage_property(Some(7)), 7);
    }

    #[test]
    fn audit_rejects_replays_without_selected_status_lifecycle() {
        let error = ensure_selected_status_lifecycle(0, 2_110_143, 2_110_151)
            .expect_err("damage-only compact replay must not be accepted as lifecycle proof");
        assert!(error.contains("status-aware current-decoder replay"));
        assert!(ensure_selected_status_lifecycle(1, 2_110_143, 2_110_151).is_ok());
    }

    #[test]
    fn owner_stage_selects_zero_based_damage_ratio_and_absent_is_stage_zero() {
        let coefficients = [15_000, 15_600, 16_200, 18_000, 18_600, 19_200, 21_000];
        assert_eq!(select_stage_coefficient(&coefficients, None), Some(15_000));
        assert_eq!(
            select_stage_coefficient(&coefficients, Some(0)),
            Some(15_000)
        );
        assert_eq!(
            select_stage_coefficient(&coefficients, Some(3)),
            Some(18_000)
        );
        assert_eq!(select_stage_coefficient(&coefficients, Some(7)), None);
        assert_eq!(select_stage_coefficient(&coefficients, Some(-1)), None);
    }

    #[test]
    fn one_coefficient_damage_rows_are_stage_invariant() {
        assert_eq!(select_stage_coefficient(&[50_000], None), Some(50_000));
        assert_eq!(select_stage_coefficient(&[50_000], Some(0)), Some(50_000));
        assert_eq!(select_stage_coefficient(&[50_000], Some(1)), Some(50_000));
        assert_eq!(select_stage_coefficient(&[50_000], Some(99)), Some(50_000));
        assert_eq!(select_stage_coefficient(&[50_000], Some(-1)), None);
    }

    #[test]
    fn omitted_optional_hit_event_id_has_protobuf_zero_semantics() {
        assert_eq!(semantic_hit_event_id(None), 0);
        assert_eq!(semantic_hit_event_id(Some(0)), 0);
        assert_eq!(semantic_hit_event_id(Some(7)), 7);
    }

    #[test]
    fn ceiling_division_is_exact() {
        assert_eq!(ceil_div(10, 3), Some(4));
        assert_eq!(ceil_div(9, 3), Some(3));
    }

    #[test]
    fn diagnostic_status_filter_does_not_mutate_strict_snapshot_equality() {
        let stable = SemanticStatusEntry {
            effect_id: 10,
            source_entity_uuid: Some(1),
            source_config_id: Some(10),
            stacks: Some(1),
            level: Some(1),
            part_id: None,
            count: None,
        };
        let companion_before = SemanticStatusEntry {
            effect_id: 20,
            source_entity_uuid: Some(2),
            source_config_id: Some(20),
            stacks: Some(1),
            level: Some(1),
            part_id: None,
            count: None,
        };
        let mut companion_after = companion_before.clone();
        companion_after.count = Some(1);
        let left = vec![stable.clone(), companion_before];
        let right = vec![stable, companion_after];

        assert_ne!(left, right);
        assert!(!semantic_snapshots_equal_ignoring(
            &left,
            &right,
            &BTreeSet::new()
        ));
        assert!(semantic_snapshots_equal_ignoring(
            &left,
            &right,
            &BTreeSet::from([20])
        ));
    }

    #[test]
    fn target_formula_control_requires_equal_non_empty_vectors_without_inventing_omissions() {
        let missing = damage_sample_with_target(None);
        let empty = damage_sample_with_target(Some(BTreeMap::new()));
        let unrelated = damage_sample_with_target(Some(BTreeMap::from([(11_440, 100)])));
        let stable = damage_sample_with_target(Some(BTreeMap::from([(11350, 100)])));
        let stable_copy = damage_sample_with_target(Some(BTreeMap::from([(11350, 100)])));
        let changed = damage_sample_with_target(Some(BTreeMap::from([(11350, 101)])));
        let stable_magic = damage_sample_with_target(Some(BTreeMap::from([(11360, 100)])));
        let stable_magic_copy = damage_sample_with_target(Some(BTreeMap::from([(11360, 100)])));

        assert!(!target_formula_attributes_match(&missing, &stable));
        assert!(!target_formula_attributes_match(&empty, &empty));
        assert!(!target_formula_attributes_match(&empty, &stable));
        assert!(target_formula_attributes_match(&unrelated, &unrelated));
        assert!(target_formula_attributes_match(&stable, &stable_copy));
        assert!(target_formula_attributes_match(
            &stable_magic,
            &stable_magic_copy
        ));
        assert!(!target_formula_attributes_match(&stable, &changed));
    }

    #[test]
    fn paired_packet_input_control_exposes_hp_and_component_deltas() {
        let inactive = damage_sample_with_target(None);
        let mut active = inactive.clone();
        assert!(pair_packet_input_deltas(&inactive, &active).is_empty());

        active.target_current_hp = Some(123);
        active.skill_effect_component_index = Some(2);
        assert_eq!(
            pair_packet_input_deltas(&inactive, &active),
            vec!["skill_effect_component_index", "target_current_hp"]
        );
    }
}
