#![allow(clippy::too_many_arguments)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorEvent, CanonicalEvent, DamagePacketDetail, EntityAttributeUpdateKind,
    EntityAttributeValue, EvidenceSource, RunState, StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::{
    ShieldInstanceSnapshot, ShieldListSnapshot, combat_action_presentation,
    decode_known_entity_attribute_value, decode_shield_list,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 47;
const MAX_INLINE_RAW_BYTES: usize = 64;
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11310;
const MAX_HP_ATTRIBUTE_ID: i32 = 11320;
const MAX_HP_TOTAL_ATTRIBUTE_ID: i32 = 11321;
const MAX_HP_ADD_ATTRIBUTE_ID: i32 = 11322;
const MAX_HP_EXTRA_ADD_ATTRIBUTE_ID: i32 = 11323;
const MAX_HP_PERCENT_ATTRIBUTE_ID: i32 = 11324;
const MAX_HP_EXTRA_PERCENT_ATTRIBUTE_ID: i32 = 11325;
const SHIELD_LIST_ATTRIBUTE_ID: i32 = 60050;
const PHYSICAL_DEFENSE_ATTRIBUTE_ID: i32 = 11350;
const PHYSICAL_DEFENSE_TOTAL_ATTRIBUTE_ID: i32 = 11351;
const PHYSICAL_DEFENSE_ADD_ATTRIBUTE_ID: i32 = 11352;
const PHYSICAL_DEFENSE_EXTRA_ADD_ATTRIBUTE_ID: i32 = 11353;
const PHYSICAL_DEFENSE_PERCENT_ATTRIBUTE_ID: i32 = 11354;
const PHYSICAL_DEFENSE_EXTRA_PERCENT_ATTRIBUTE_ID: i32 = 11355;
const PHYSICAL_ATTACK_ATTRIBUTE_ID: i32 = 11330;
const MAGICAL_ATTACK_ATTRIBUTE_ID: i32 = 11340;
const MAGICAL_DEFENSE_ATTRIBUTE_ID: i32 = 11360;
const PHYSICAL_IGNORE_DEFENSE_ATTRIBUTE_ID: i32 = 11370;
const MAGICAL_IGNORE_DEFENSE_ATTRIBUTE_ID: i32 = 11380;
const PHYSICAL_IGNORE_DEFENSE_PERCENT_ATTRIBUTE_ID: i32 = 11390;
const MAGICAL_IGNORE_DEFENSE_PERCENT_ATTRIBUTE_ID: i32 = 11400;
const REFINED_PHYSICAL_ATTACK_ATTRIBUTE_ID: i32 = 11410;
const REFINED_MAGICAL_ATTACK_ATTRIBUTE_ID: i32 = 11430;
const ELEMENT_ATTACK_ATTRIBUTE_ID: i32 = 11500;
const SEASON_STRENGTH_ATTRIBUTE_ID: i32 = 11440;
const SEASON_TARGET_INPUT_ATTRIBUTE_ID: i32 = 11450;
const CRITICAL_CHANCE_ATTRIBUTE_ID: i32 = 11710;
const LUCKY_CHANCE_ATTRIBUTE_ID: i32 = 11780;
const EXTERNAL_DAMAGE_ATTRIBUTE_ID: i32 = 11840;
const HASTE_ATTRIBUTE_ID: i32 = 11930;
const MASTERY_ATTRIBUTE_ID: i32 = 11940;
const VERSATILITY_ATTRIBUTE_ID: i32 = 11950;
const DERIVED_LIGHT_DAMAGE_ATTRIBUTE_ID: i32 = 13170;
const GENERIC_ELEMENT_DAMAGE_ATTRIBUTE_ID: i32 = 13100;
const GENERIC_ELEMENT_DAMAGE_ATTRIBUTE_FAMILY: std::ops::RangeInclusive<i32> = 13100..=13105;
const FATAL_SPIRAL_TEAM_STATUS_EFFECT_ID: i64 = 2_110_125;
const SHIELD_ADD_PERCENT_ATTRIBUTE_IDS: std::ops::RangeInclusive<i32> = 11810..=11815;
const SHIELD_GAIN_PERCENT_ATTRIBUTE_IDS: std::ops::RangeInclusive<i32> = 11820..=11825;
const SHIELD_DAMAGE_PERCENT_ATTRIBUTE_IDS: std::ops::RangeInclusive<i32> = 12650..=12655;
const SHIELD_DAMAGE_REDUCTION_PERCENT_ATTRIBUTE_IDS: std::ops::RangeInclusive<i32> = 12660..=12665;
const CURRENT_ENERGY_ATTRIBUTE_ID: i32 = 20010;
const MAX_ENERGY_ATTRIBUTE_ID: i32 = 20020;
const MAX_ENERGY_TOTAL_ATTRIBUTE_ID: i32 = 20021;
const MAX_ENERGY_ADD_ATTRIBUTE_ID: i32 = 20022;
const MAX_ENERGY_EXTRA_ADD_ATTRIBUTE_ID: i32 = 20023;
const MAX_ENERGY_PERCENT_ATTRIBUTE_ID: i32 = 20024;
const MAX_ENERGY_EXTRA_PERCENT_ATTRIBUTE_ID: i32 = 20025;
const OUTGOING_DAMAGE_ATTRIBUTE_RANGES: [std::ops::RangeInclusive<i32>; 11] = [
    11840..=11845,
    12550..=12555,
    12570..=12575,
    12630..=12635,
    12670..=12675,
    12690..=12695,
    12710..=12715,
    12730..=12735,
    12750..=12755,
    13100..=13105,
    13170..=13175,
];
const DEFAULT_EXAMPLE_LIMIT: usize = 12;
const STATUS_SIGNATURE_REPORT_LIMIT: usize = 64;
const CROSS_STATE_CANDIDATE_MINIMUM_COVERAGE_BASIS_POINTS: u64 = 2_500;
const SINGLE_STATE_CANDIDATE_MINIMUM_COVERAGE_BASIS_POINTS: u64 = 5_000;
const BASH_PURSUIT_ABILITY_ID: i64 = 2_201_540;
const BASH_PURSUIT_SHIELD_CAP_BASIS_POINTS: i64 = 15_000;
const BASH_PURSUIT_SHIELD_DAMAGE_MULTIPLIER: i64 = 3;
const BASH_PURSUIT_OBSERVED_LATER_MULTIPLIER_NUMERATOR: i64 = 3;
const BASH_PURSUIT_OBSERVED_LATER_MULTIPLIER_DENOMINATOR: i64 = 2;
const JUDGMENT_PURSUIT_ABILITY_ID: i64 = 2_206_290;
const JUDGMENT_PURSUIT_MAX_HP_MULTIPLIER: i64 = 3;
const JUDGMENT_PURSUIT_INTEGER_OFFSET: i64 = 1;
const COEFFICIENT_PAIR_ABILITY_ID: i64 = 2_352;
const COEFFICIENT_PAIR_HIGH_EVENT_ID: i32 = 1;
const COEFFICIENT_PAIR_LOW_EVENT_ID: i32 = 3;
const COEFFICIENT_PAIR_HIGH_BASIS_POINTS: i64 = 50_000;
const COEFFICIENT_PAIR_LOW_BASIS_POINTS: i64 = 8_000;
const FORMULA_PROOF_EXAMPLE_LIMIT: usize = 24;

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    source_entities: BTreeSet<i64>,
    effects: BTreeSet<i64>,
    abilities: BTreeSet<i64>,
    sequences: BTreeSet<u64>,
    all_abilities: bool,
    output: Option<PathBuf>,
    proof_only: bool,
    inventory_output: Option<PathBuf>,
    effect_ability_inventory_output: Option<PathBuf>,
    example_limit: usize,
    formula_cohort_output: Option<PathBuf>,
    formula_proof_output: Option<PathBuf>,
    formula_sample_limit: usize,
    formula_target_effects: BTreeSet<i64>,
    formula_effect_locus: FormulaEffectLocus,
    formula_gap_window_audit: Option<PathBuf>,
    formula_transition_seeds: Option<PathBuf>,
    formula_transition_window_micros: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FormulaEffectLocus {
    Source,
    #[default]
    Target,
}

#[derive(Debug, Deserialize)]
struct FormulaTransitionSeedBundle {
    schema_version: u16,
    expected_game_build: Option<String>,
    source_rlogs: Vec<String>,
    selected_effect_ids: Vec<i64>,
    selected_attribute_ids: Vec<i32>,
    exact_single_term_equation_occurrences: u64,
    retained_transition_seeds: u64,
    all_equation_occurrences_retained: bool,
    transitions: Vec<FormulaTransitionSeed>,
}

#[derive(Debug, Deserialize)]
struct FormulaTransitionSeed {
    effect_id: i64,
    attribute_id: i32,
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    target_entity_uuid: i64,
    wire_observed_micros: u64,
}

#[derive(Debug)]
struct FormulaTransitionSeedFilter {
    metadata: FormulaTransitionSeedFilterMetadata,
    seeds_by_recipient: BTreeMap<(String, u32, i64), Vec<u64>>,
}

#[derive(Debug, Serialize)]
struct FormulaTransitionSeedFilterMetadata {
    source: String,
    source_sha256: String,
    window_micros_before_and_after: u64,
    retained_transition_seeds: u64,
    selected_effect_ids: Vec<i64>,
    selected_attribute_ids: Vec<i32>,
    selection_policy: &'static str,
    formula_authority: bool,
}

#[derive(Debug, Deserialize)]
struct FormulaGapWindowAuditBundle {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    effect_id: i64,
    #[serde(default)]
    damage_relationship: FormulaEffectLocus,
    policy: FormulaGapWindowAuditPolicy,
    summary: FormulaGapWindowAuditSummary,
    sessions: Vec<FormulaGapWindowAuditSession>,
}

#[derive(Debug, Deserialize)]
struct FormulaGapWindowAuditPolicy {
    every_data_gap_and_recorder_pause_is_an_exclusion_boundary: bool,
    status_lifecycles_never_cross_exclusion_or_run_boundaries: bool,
    packet_absence_is_not_zero: bool,
    structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: bool,
    current_snapshots_are_never_backfilled_into_historical_windows: bool,
    #[serde(default)]
    damage_relationship_is_explicit: bool,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Deserialize)]
struct FormulaGapWindowAuditSummary {
    source_rlog_count: u64,
    selected_effect_complete_gap_bounded_lifecycle_count: u64,
    selected_effect_damage_events_while_active: u64,
    exact_gap_bounded_lifecycle_windows_identified: bool,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Deserialize)]
struct FormulaGapWindowAuditSession {
    path: String,
    session_id: String,
    sealed_content_sha256: String,
    complete_gap_bounded_windows: Vec<FormulaGapWindowAuditWindow>,
}

#[derive(Debug, Clone, Deserialize)]
struct FormulaGapWindowAuditWindow {
    target_entity_uuid: i64,
    applied_envelope_sequence: u64,
    applied_observed_micros: u64,
    terminal_envelope_sequence: u64,
    terminal_observed_micros: u64,
    damage_events_while_active: u64,
    gap_bounded: bool,
    controlled_counterfactual_pair_proven: bool,
    formula_authority: bool,
    #[serde(default)]
    effect_endpoint_damage_role: Option<String>,
}

#[derive(Debug)]
struct FormulaGapWindowFilter {
    metadata: FormulaGapWindowFilterMetadata,
    sessions_by_file: BTreeMap<String, FormulaGapWindowSessionFilter>,
    matched_damage_events: u64,
    matched_window_damage_memberships: u64,
}

#[derive(Debug)]
struct FormulaGapWindowSessionFilter {
    session_id: String,
    sealed_content_sha256: String,
    windows_by_target: BTreeMap<i64, Vec<FormulaGapWindowAuditWindow>>,
}

#[derive(Debug, Clone, Serialize)]
struct FormulaGapWindowFilterMetadata {
    source: String,
    source_sha256: String,
    effect_id: i64,
    effect_locus: FormulaEffectLocus,
    complete_gap_bounded_lifecycles: u64,
    audited_damage_events_while_active: u64,
    matched_damage_events: u64,
    matched_window_damage_memberships: u64,
    selection_policy: &'static str,
    formula_authority: bool,
}

impl FormulaTransitionSeedFilter {
    fn matches(
        &self,
        session_id: &str,
        run_ordinal: u32,
        source_entity_uuid: i64,
        observed_micros: u64,
    ) -> bool {
        self.seeds_by_recipient
            .get(&(session_id.to_owned(), run_ordinal, source_entity_uuid))
            .is_some_and(|seeds| {
                seeds.iter().any(|seed| {
                    observed_micros.abs_diff(*seed) <= self.metadata.window_micros_before_and_after
                })
            })
    }
}

impl FormulaGapWindowFilter {
    fn matches(
        &mut self,
        path: &Path,
        session_id: &str,
        target_entity_uuid: i64,
        sequence: u64,
        observed_micros: u64,
    ) -> bool {
        let Some(session) = self.sessions_by_file.get(&normalized_file_label(path)) else {
            return false;
        };
        if session.session_id != session_id {
            return false;
        }
        let matched_windows = session
            .windows_by_target
            .get(&target_entity_uuid)
            .map(|windows| {
                windows
                    .iter()
                    .filter(|window| {
                        sequence > window.applied_envelope_sequence
                            && sequence < window.terminal_envelope_sequence
                            && observed_micros >= window.applied_observed_micros
                            && observed_micros <= window.terminal_observed_micros
                    })
                    .count() as u64
            })
            .unwrap_or(0);
        let matched = matched_windows > 0;
        self.matched_damage_events = self
            .matched_damage_events
            .saturating_add(u64::from(matched));
        self.matched_window_damage_memberships = self
            .matched_window_damage_memberships
            .saturating_add(matched_windows);
        matched
    }

    fn validate_replay(
        &self,
        path: &Path,
        session_id: &str,
        content_sha256: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let session = self
            .sessions_by_file
            .get(&normalized_file_label(path))
            .ok_or_else(|| format!("gap-window audit does not declare {}", path.display()))?;
        if session.session_id != session_id || session.sealed_content_sha256 != content_sha256 {
            return Err(format!(
                "gap-window audit replay identity mismatch for {}",
                path.display()
            )
            .into());
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.matched_window_damage_memberships
            != self.metadata.audited_damage_events_while_active
        {
            return Err(format!(
                "gap-window audit selected {} damage-window memberships, but replay matched {}",
                self.metadata.audited_damage_events_while_active,
                self.matched_window_damage_memberships
            )
            .into());
        }
        self.metadata.matched_damage_events = self.matched_damage_events;
        self.metadata.matched_window_damage_memberships = self.matched_window_damage_memberships;
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
struct ActorSnapshot {
    entity_type_id: Option<i32>,
    monster_id: Option<i64>,
    character_id: Option<String>,
    name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
}

#[derive(Debug, Default)]
struct AbilityAccumulator {
    events: u64,
    amount_sum: i128,
    events_with_normal_value: u64,
    event_state_ratios: StateRatioAccumulator,
    wire_start_state_ratios: StateRatioAccumulator,
    wire_to_event_state_transitions: BTreeMap<&'static str, ScalarTransitionAccumulator>,
    source_hp_transition_semantics: SourceHpTransitionAccumulator,
    source_status_signatures: BTreeMap<Vec<i64>, u64>,
    target_status_signatures: BTreeMap<Vec<i64>, u64>,
    shield_provider_observations: BTreeMap<ShieldProviderKey, ShieldProviderAccumulator>,
    bash_pursuit_formula: Option<BashPursuitFormulaAccumulator>,
    judgment_pursuit_formula: Option<JudgmentPursuitFormulaAccumulator>,
    examples: Vec<DamageExample>,
}

#[derive(Debug, Default)]
struct BashPursuitFormulaAccumulator {
    events_with_exact_packet_inputs: u64,
    events_without_exact_packet_inputs: u64,
    events_using_shield_cap: u64,
    formula_base_sum: i128,
    best_exact_snapshot_formula_base_sum: i128,
    normal_to_formula_base_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_formula_base_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_best_exact_snapshot_formula_base_basis_points: BTreeMap<i64, RatioAccumulator>,
    events_with_exact_inferred_transient_snapshot: u64,
    events_with_resolved_transient_provider_ownership: u64,
    events_with_unresolved_transient_provider_ownership: u64,
    inferred_transient_snapshot_examples: Vec<InferredTransientBashPursuitExample>,
    external_provider_counterfactuals:
        BTreeMap<ShieldProviderKey, ExternalShieldCounterfactualAccumulator>,
}

#[derive(Debug, Default)]
struct JudgmentPursuitFormulaAccumulator {
    events: u64,
    events_with_exact_integer_solution: u64,
    events_without_exact_integer_solution: u64,
    events_matching_wire_start_max_hp: u64,
    events_matching_event_order_max_hp: u64,
    events_matching_both_packet_states: u64,
    inferred_calculation_time_max_hp_values: BTreeSet<i64>,
}

#[derive(Debug, Default)]
struct ExternalShieldCounterfactualAccumulator {
    damage_events: u64,
    events_with_exact_current_value: u64,
    events_without_exact_current_value: u64,
    events_with_exact_zero_without_provider_current_value: u64,
    provider_current_shield_sum: i128,
    provider_removed_formula_base_delta_sum: i128,
    events_with_positive_formula_base_delta: u64,
    events_with_zero_formula_base_delta_due_to_cap: u64,
    provider_entity_uuids: BTreeSet<i64>,
    recipient_entity_uuids: BTreeSet<i64>,
    shield_uuids: BTreeSet<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum ShieldProviderRelation {
    SelfProvided,
    ExternallyProvided,
    UnresolvedProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ShieldProviderKey {
    relation: ShieldProviderRelation,
    shield_type: Option<i32>,
    effect_id: Option<i64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Default)]
struct ShieldProviderAccumulator {
    damage_events: u64,
    shield_instance_observations: u64,
    observed_damage_amount_sum: i128,
    current_value_observations: u64,
    current_value_sum: i128,
    current_value_min: Option<i64>,
    current_value_max: Option<i64>,
    shield_uuids: BTreeSet<i64>,
    provider_entity_uuids: BTreeSet<i64>,
    damage_source_entity_uuids: BTreeSet<i64>,
}

#[derive(Debug, Default)]
struct StateRatioAccumulator {
    events_with_source_current_hp: u64,
    events_with_source_max_hp: u64,
    events_with_source_current_shield: u64,
    events_with_source_max_hp_plus_three_current_shield: u64,
    events_with_source_missing_hp: u64,
    events_with_source_physical_defense: u64,
    events_with_target_current_hp: u64,
    events_with_target_max_hp: u64,
    events_with_target_missing_hp: u64,
    amount_to_source_current_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    normal_to_source_max_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_source_max_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    normal_to_source_current_shield_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_source_current_shield_basis_points: BTreeMap<i64, RatioAccumulator>,
    normal_to_source_max_hp_plus_three_current_shield_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_source_max_hp_plus_three_current_shield_basis_points: BTreeMap<i64, RatioAccumulator>,
    normal_to_source_missing_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_source_missing_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    normal_to_source_physical_defense_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_source_physical_defense_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_target_current_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    normal_to_target_max_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_target_max_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    normal_to_target_missing_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_target_missing_hp_basis_points: BTreeMap<i64, RatioAccumulator>,
    amount_to_source_max_hp_near_integer_multiples: BTreeMap<(i64, i64), u64>,
    amount_to_source_physical_defense_near_integer_multiples: BTreeMap<(i64, i64), u64>,
}

#[derive(Debug, Default, Clone)]
struct RatioAccumulator {
    count: u64,
    numerators: BTreeSet<i64>,
    denominators: BTreeSet<i64>,
}

#[derive(Debug, Default, Clone)]
struct ScalarTransitionAccumulator {
    events_with_both_states: u64,
    unchanged_events: u64,
    increased_events: u64,
    decreased_events: u64,
    signed_changes: BTreeMap<i64, u64>,
    amount_to_absolute_change_basis_points: BTreeMap<i64, RatioAccumulator>,
    normal_to_absolute_change_basis_points: BTreeMap<i64, RatioAccumulator>,
}

#[derive(Debug, Default)]
struct SourceHpTransitionAccumulator {
    events_with_current_and_max_hp_at_both_timings: u64,
    semantic_counts: BTreeMap<SourceHpTransitionSemantic, u64>,
    signed_change_triplets: BTreeMap<(i64, i64, i64), u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum SourceHpTransitionSemantic {
    Unchanged,
    CurrentHpChangedMaxHpStable,
    CurrentHpStableMaxHpChanged,
    CurrentAndMaxHpChangedSameDeltaPreservingMissingHp,
    CurrentAndMaxHpChangedDifferentDelta,
}

#[derive(Debug, Default)]
struct SourceAccumulator {
    events: u64,
    actor_keys: BTreeSet<String>,
    abilities: BTreeMap<i64, AbilityAccumulator>,
}

#[derive(Debug, Default)]
struct EffectAccumulator {
    status_events: u64,
    actor_keys: BTreeSet<String>,
    source_damage_events_while_active: u64,
    direct_source_damage_events_while_active: u64,
    packet_origins: BTreeMap<(i32, i64), u64>,
    status_examples: Vec<StatusExample>,
    abilities: BTreeMap<i64, AbilityAccumulator>,
}

#[derive(Debug, Serialize)]
struct ProofBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: ProofPolicy,
    inputs: Vec<String>,
    summary: ProofSummary,
    hp_shield_dependency_inventory: Vec<HpShieldDependencyCandidateReport>,
    selected_damage_abilities: Vec<AbilityReport>,
    passive_sources: Vec<PassiveSourceReport>,
    active_status_effects: Vec<StatusEffectReport>,
    selected_sequence_examples: Vec<DamageExample>,
}

#[derive(Debug, Serialize)]
struct FormulaCohortBundle<'a> {
    schema_version: u16,
    generated_by: &'static str,
    game_build: &'a str,
    policy: FormulaCohortPolicy,
    selection: FormulaSelectionReceipt,
    gap_window_filter: Option<&'a FormulaGapWindowFilterMetadata>,
    transition_seed_filter: Option<&'a FormulaTransitionSeedFilterMetadata>,
    inputs: &'a [String],
    attribute_states: &'a [Vec<CompactFormulaAttribute>],
    status_states: &'a [Vec<CompactFormulaStatus>],
    samples: &'a [FormulaCohortSample],
}

#[derive(Debug, Serialize)]
struct FormulaCohortPolicy {
    state_timing: &'static str,
    attribute_retention: &'static str,
    direct_source_attribute_retention: &'static str,
    status_retention: &'static str,
    status_provider_attribute_retention: &'static str,
    actor_and_scene_identity_retention: &'static str,
    position_retention: &'static str,
    packet_retention: &'static str,
    formula_authority: bool,
}

#[derive(Debug, Serialize)]
struct FormulaSelectionReceipt {
    all_abilities: bool,
    ability_ids: Vec<i64>,
    selected_effect_ids: Vec<i64>,
    source_effect_ids: Vec<i64>,
    target_effect_ids: Vec<i64>,
    effect_locus: FormulaEffectLocus,
    target_effect_scope: &'static str,
    target_effect_timing: &'static str,
    gap_policy: &'static str,
    formula_authority: bool,
}

#[derive(Debug, Serialize)]
struct EffectAbilityInventoryBundle<'a> {
    schema_version: u16,
    generated_by: &'static str,
    game_build: &'a str,
    policy: &'static str,
    inputs: &'a [String],
    effects: Vec<EffectAbilityInventoryEffect>,
}

#[derive(Debug, Serialize)]
struct EffectAbilityInventoryEffect {
    effect_id: i64,
    status_events: u64,
    source_damage_events_while_active: u64,
    direct_source_damage_events_while_active: u64,
    abilities: Vec<EffectAbilityInventoryAbility>,
}

#[derive(Debug, Serialize)]
struct EffectAbilityInventoryAbility {
    ability_id: i64,
    events: u64,
    amount_sum: String,
}

#[derive(Debug, Default)]
struct FormulaCohortAccumulator {
    attribute_state_ids: BTreeMap<Vec<CompactFormulaAttribute>, u32>,
    attribute_states: Vec<Vec<CompactFormulaAttribute>>,
    status_state_ids: BTreeMap<Vec<CompactFormulaStatus>, u32>,
    status_states: Vec<Vec<CompactFormulaStatus>>,
    samples: Vec<FormulaCohortSample>,
}

#[derive(Debug, Clone, Serialize)]
struct FormulaCohortSample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    scene_id: Option<i32>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    source_actor_identity: Option<CompactFormulaActorIdentity>,
    direct_source_actor_identity: Option<CompactFormulaActorIdentity>,
    target_actor_identity: Option<CompactFormulaActorIdentity>,
    source_position_at_wire_message_start: Option<CompactFormulaPositionObservation>,
    direct_source_position_at_wire_message_start: Option<CompactFormulaPositionObservation>,
    target_position_at_wire_message_start: Option<CompactFormulaPositionObservation>,
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
    direct_source_attribute_state_id: Option<u32>,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
    status_provider_attribute_states: Vec<CompactFormulaProviderAttributeState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CompactFormulaActorIdentity {
    entity_type_id: i32,
    monster_id: Option<i64>,
    character_id: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct CompactFormulaPositionObservation {
    x: f32,
    y: f32,
    z: f32,
    facing_radians: Option<f32>,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
}

#[derive(Debug, Serialize)]
struct FormulaProofBundle {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    policy: FormulaProofPolicy,
    selection: FormulaSelectionReceipt,
    gap_window_filter: Option<FormulaGapWindowFilterMetadata>,
    inputs: Vec<String>,
    sample_count: usize,
    season_vectors: Vec<SeasonVectorCount>,
    season_target_input_proof: SeasonTargetInputProof,
    input_determinism: FormulaInputDeterminismReport,
    message_scope_determinism: FormulaMessageScopeReport,
    formula_surface: FormulaSurfaceInventoryReport,
    coefficient_pair_proof: FormulaCoefficientPairReport,
    post_coefficient_stage: FormulaPostCoefficientStageReport,
    candidates: Vec<FormulaCandidateReport>,
    candidate_bundles: Vec<FormulaCandidateBundleReport>,
}

#[derive(Debug, Serialize)]
struct FormulaSurfaceInventoryReport {
    scope: &'static str,
    group_count: usize,
    sample_count: u64,
    samples_with_normal_value: u64,
    samples_with_lucky_value: u64,
    samples_flagged_critical: u64,
    samples_flagged_lucky: u64,
    samples_with_source_physical_attack: u64,
    samples_with_source_magical_attack: u64,
    samples_with_target_physical_defense: u64,
    samples_with_target_magical_defense: u64,
    source_formula_attributes: Vec<FormulaSurfaceAttributeReport>,
    target_formula_attributes: Vec<FormulaSurfaceAttributeReport>,
    groups: Vec<FormulaSurfaceGroupReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct FormulaSurfaceGroupKey {
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
}

#[derive(Debug, Default)]
struct FormulaSurfaceGroupAccumulator {
    samples: u64,
    normal_value_samples: u64,
    lucky_value_samples: u64,
    actual_amount_samples: u64,
    hp_loss_samples: u64,
    shield_loss_samples: u64,
    source_attributes: BTreeMap<i32, FormulaSurfaceAttributeAccumulator>,
    target_attributes: BTreeMap<i32, FormulaSurfaceAttributeAccumulator>,
    examples: Vec<FormulaSurfaceExample>,
}

#[derive(Debug, Default)]
struct FormulaSurfaceAttributeAccumulator {
    samples: u64,
    distinct_values: BTreeSet<i64>,
}

#[derive(Debug, Serialize)]
struct FormulaSurfaceAttributeReport {
    attribute_id: i32,
    samples: u64,
    distinct_values: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct FormulaSurfaceGroupReport {
    #[serde(flatten)]
    key: FormulaSurfaceGroupKey,
    samples: u64,
    normal_value_samples: u64,
    lucky_value_samples: u64,
    actual_amount_samples: u64,
    hp_loss_samples: u64,
    shield_loss_samples: u64,
    source_attributes: Vec<FormulaSurfaceAttributeReport>,
    target_attributes: Vec<FormulaSurfaceAttributeReport>,
    examples: Vec<FormulaSurfaceExample>,
}

#[derive(Debug, Clone, Serialize)]
struct FormulaSurfaceExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    wire_capture_sequence: Option<u64>,
    observed_micros: u64,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    amount: i64,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
}

#[derive(Debug, Serialize)]
struct FormulaProofPolicy {
    state_timing: &'static str,
    strict_scope: &'static str,
    diagnostic_scope: &'static str,
    output_fields_removed_from_control_key: &'static str,
    promotion_rule: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct FormulaInputDeterminismReport {
    proof_authority: bool,
    scope: &'static str,
    input_groups: u64,
    repeated_input_groups: u64,
    repeated_input_samples: u64,
    invariant_repeated_groups: u64,
    divergent_repeated_groups: u64,
    divergent_repeated_samples: u64,
    examples: Vec<FormulaInputDeterminismExample>,
}

#[derive(Debug, Default, Serialize)]
struct FormulaMessageScopeReport {
    proof_authority: bool,
    scope: &'static str,
    ability_id: i64,
    wire_groups: u64,
    multi_target_wire_groups: u64,
    invariant_multi_target_wire_groups: u64,
    divergent_multi_target_wire_groups: u64,
    cross_wire_control_groups: u64,
    divergent_cross_wire_control_groups: u64,
    invariant_wires_in_divergent_control_groups: u64,
    target_samples_in_divergent_control_groups: u64,
    shared_scalar: FormulaMessageSharedScalarReport,
    examples: Vec<FormulaMessageScopeExample>,
}

#[derive(Debug, Default, Serialize)]
struct FormulaMessageSharedScalarReport {
    proof_authority: bool,
    scope: &'static str,
    wire_pair_candidates: u64,
    multi_signature_wire_pairs: u64,
    exact_target_state_overlap_wire_pairs: u64,
    identity_ratio_wire_pairs: u64,
    changed_ratio_wire_pairs: u64,
    floor_interval_consistent_wire_pairs: u64,
    floor_interval_inconsistent_wire_pairs: u64,
    exact_target_state_floor_interval_consistent_wire_pairs: u64,
    exact_target_state_floor_interval_inconsistent_wire_pairs: u64,
    maximum_signature_support: u64,
    maximum_exact_target_state_signature_support: u64,
    examples: Vec<FormulaMessageSharedScalarExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormulaMessageSharedScalarContext {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    source_attribute_state_id: u32,
    source_status_state_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormulaMessageSharedScalarPairKey {
    context: FormulaMessageSharedScalarContext,
    from_wire_capture_sequence: u64,
    to_wire_capture_sequence: u64,
}

#[derive(Debug)]
struct FormulaMessageSharedScalarSignature {
    control_signature: String,
    from_output: i64,
    to_output: i64,
    exact_target_state_overlap: u64,
}

#[derive(Debug, Serialize)]
struct FormulaMessageSharedScalarExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    ability_id: i64,
    hit_event_id: Option<i32>,
    source_attribute_state_id: u32,
    source_status_state_id: u32,
    from_wire_capture_sequence: u64,
    to_wire_capture_sequence: u64,
    signature_support: u64,
    exact_target_state_signature_support: u64,
    floor_interval_consistent: bool,
    exact_target_state_floor_interval_consistent: Option<bool>,
    observed_ratio_parts_per_million_min: i64,
    observed_ratio_parts_per_million_max: i64,
    observed_ratio_parts_per_million_spread: i64,
    common_floor_ratio_lower_parts_per_million: i64,
    common_floor_ratio_upper_parts_per_million: i64,
    signatures: Vec<FormulaMessageSharedScalarSignatureExample>,
}

#[derive(Debug, Serialize)]
struct FormulaMessageSharedScalarSignatureExample {
    control_signature: String,
    from_output: i64,
    to_output: i64,
    exact_target_state_overlap: u64,
    observed_ratio_parts_per_million: i64,
    floor_ratio_lower_parts_per_million: i64,
    floor_ratio_upper_parts_per_million: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormulaMessageControlKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    packet_formula_inputs: String,
    source_attribute_state_id: u32,
    source_status_state_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormulaMessageKey {
    control: FormulaMessageControlKey,
    wire_capture_sequence: u64,
}

#[derive(Debug, Clone, Default)]
struct FormulaMessageAccumulator {
    observed_micros: BTreeSet<u64>,
    target_entity_uuids: BTreeSet<i64>,
    target_state_tuples: BTreeSet<(i64, u32, u32)>,
    target_attribute_state_ids: BTreeSet<u32>,
    target_status_state_ids: BTreeSet<u32>,
    outcomes: BTreeMap<FormulaOutcome, u64>,
}

#[derive(Debug, Serialize)]
struct FormulaMessageScopeExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    ability_id: i64,
    hit_event_id: Option<i32>,
    source_attribute_state_id: u32,
    source_status_state_id: u32,
    wires: Vec<FormulaMessageWireExample>,
}

#[derive(Debug, Serialize)]
struct FormulaMessageWireExample {
    wire_capture_sequence: u64,
    observed_micros: Vec<u64>,
    target_entity_count: u64,
    target_attribute_state_count: u64,
    target_status_state_count: u64,
    target_samples: u64,
    outcomes: Vec<FormulaOutcomeCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormulaExactInputKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    packet_inputs: String,
    source_attribute_state_id: u32,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
}

#[derive(Debug, Serialize)]
struct FormulaInputDeterminismExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    source_attribute_state_id: u32,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
    samples: u64,
    outcomes: Vec<FormulaOutcomeCount>,
}

#[derive(Debug, Serialize)]
struct FormulaOutcomeCount {
    #[serde(flatten)]
    outcome: FormulaOutcome,
    samples: u64,
}

#[derive(Debug, Default, Serialize)]
struct FormulaCoefficientPairReport {
    formula_stage_authority: bool,
    ability_id: i64,
    high_event_id: i32,
    low_event_id: i32,
    high_coefficient_basis_points: i64,
    low_coefficient_basis_points: i64,
    matching_scope: &'static str,
    candidate_groups: u64,
    candidate_comparisons: u64,
    evaluated_nearest_sequence_comparisons: u64,
    exact_proportional_comparisons: u64,
    near_proportional_residual_exclusive: i64,
    near_proportional_comparisons: u64,
    packet_inputs_equal_comparisons: u64,
    residual_min: Option<i64>,
    residual_max: Option<i64>,
    residuals: Vec<FormulaCoefficientResidualCount>,
    packet_input_pair_variants: u64,
    packet_input_pairs: Vec<FormulaPacketInputPairCount>,
    examples: Vec<FormulaCoefficientPairExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormulaCoefficientPairKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    wire_capture_sequence: Option<u64>,
    observed_micros: u64,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    passive_uuid: Option<u32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    source_attribute_state_id: u32,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
}

#[derive(Debug, Serialize)]
struct FormulaCoefficientResidualCount {
    cross_product_residual: i64,
    comparisons: u64,
}

#[derive(Debug, Serialize)]
struct FormulaPacketInputPairCount {
    high_event_packet_inputs: String,
    low_event_packet_inputs: String,
    comparisons: u64,
    residuals: Vec<FormulaCoefficientResidualCount>,
}

#[derive(Debug, Serialize)]
struct FormulaCoefficientPairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    wire_capture_sequence: Option<u64>,
    observed_micros: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    source_attribute_state_id: u32,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
    high_sequence: u64,
    low_sequence: u64,
    sequence_gap: u64,
    high_amount: i64,
    low_amount: i64,
    high_normal_value: Option<i64>,
    low_normal_value: Option<i64>,
    cross_product_residual: i64,
    ratio_basis_points: Option<i64>,
    high_event_packet_inputs: String,
    low_event_packet_inputs: String,
}

#[derive(Debug, Default, Serialize)]
struct FormulaPostCoefficientStageReport {
    proof_authority: bool,
    applicable: bool,
    not_applicable_reason: Option<&'static str>,
    scope: &'static str,
    fixed_point_denominator: i64,
    ability_id: i64,
    high_event_id: i32,
    low_event_id: i32,
    high_coefficient_basis_points: i64,
    low_coefficient_basis_points: i64,
    samples_with_source_attack: u64,
    samples_with_positive_coefficient_body_and_normal_output: u64,
    individual_integer_factor_interval_compatible_samples: u64,
    individual_integer_factor_interval_incompatible_samples: u64,
    paired_groups_with_source_attack: u64,
    paired_groups_with_positive_bodies_and_normal_outputs: u64,
    paired_integer_factor_interval_consistent_groups: u64,
    paired_integer_factor_interval_inconsistent_groups: u64,
    paired_exact_integer_factor_groups: u64,
    factor_intervals: Vec<FormulaPostCoefficientFactorIntervalCount>,
    examples: Vec<FormulaPostCoefficientStageExample>,
}

#[derive(Debug, Serialize)]
struct FormulaPostCoefficientFactorIntervalCount {
    minimum_factor_basis_points: i64,
    maximum_factor_basis_points: i64,
    groups: u64,
}

#[derive(Debug, Serialize)]
struct FormulaPostCoefficientStageExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    wire_capture_sequence: Option<u64>,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    source_attribute_state_id: u32,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
    source_attack: i64,
    source_mastery_basis_points: Option<i64>,
    source_versatility_basis_points: Option<i64>,
    source_light_damage_basis_points: Option<i64>,
    source_season_strength: Option<i64>,
    target_physical_defense: Option<i64>,
    target_season_strength: Option<i64>,
    critical: Option<bool>,
    lucky: Option<bool>,
    high_sequence: u64,
    low_sequence: u64,
    high_coefficient_body: i64,
    low_coefficient_body: i64,
    high_normal_value: i64,
    low_normal_value: i64,
    high_factor_interval_minimum_basis_points: Option<i64>,
    high_factor_interval_maximum_basis_points: Option<i64>,
    low_factor_interval_minimum_basis_points: Option<i64>,
    low_factor_interval_maximum_basis_points: Option<i64>,
    shared_factor_interval_minimum_basis_points: Option<i64>,
    shared_factor_interval_maximum_basis_points: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
enum FormulaAttributeLocus {
    Source,
    Target,
}

#[derive(Debug, Serialize)]
struct FormulaCandidateReport {
    name: &'static str,
    locus: &'static str,
    attribute_id: i32,
    samples_with_attribute: u64,
    distinct_values: Vec<i64>,
    minimum_value: Option<i64>,
    maximum_value: Option<i64>,
    strict_all_observed_state: FormulaControlledScopeReport,
    target_current_hp_excluded_diagnostic: FormulaControlledScopeReport,
}

#[derive(Debug, Serialize)]
struct FormulaCandidateBundleReport {
    name: &'static str,
    locus: &'static str,
    primary_attribute_id: i32,
    removed_attribute_ids: Vec<i32>,
    removed_source_status_effect_ids: Vec<i64>,
    samples_with_primary_attribute: u64,
    distinct_primary_values: Vec<i64>,
    strict_all_observed_state: FormulaControlledScopeReport,
    target_current_hp_excluded_diagnostic: FormulaControlledScopeReport,
    position_excluded_diagnostic: FormulaControlledScopeReport,
    position_and_target_current_hp_excluded_diagnostic: FormulaControlledScopeReport,
    position_hp_and_non_candidate_statuses_excluded_diagnostic: FormulaControlledScopeReport,
    near_pair_diagnostics: FormulaBundleNearPairDiagnostics,
    basis_point_multiplier_check: FormulaBasisPointMultiplierCheck,
}

#[derive(Debug, Default, Serialize)]
struct FormulaBundleNearPairDiagnostics {
    proof_authority: bool,
    scope: &'static str,
    controlled_groups: u64,
    comparisons: u64,
    status_difference_signatures: Vec<FormulaStatusDifferenceSignatureCount>,
    examples: Vec<FormulaBundleNearPairExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct FormulaStatusDifferenceSignature {
    source_only_from: Vec<CompactFormulaStatus>,
    source_only_to: Vec<CompactFormulaStatus>,
    target_only_from: Vec<CompactFormulaStatus>,
    target_only_to: Vec<CompactFormulaStatus>,
}

#[derive(Debug, Serialize)]
struct FormulaStatusDifferenceSignatureCount {
    #[serde(flatten)]
    signature: FormulaStatusDifferenceSignature,
    comparisons: u64,
}

#[derive(Debug, Serialize)]
struct FormulaBundleNearPairExample {
    candidate_from: i64,
    candidate_to: i64,
    from: FormulaNearPairSample,
    to: FormulaNearPairSample,
    status_differences: FormulaStatusDifferenceSignature,
}

#[derive(Debug, Serialize)]
struct FormulaNearPairSample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    damage_property: Option<i32>,
    target_current_hp: Option<i64>,
    source_attributes: Vec<CompactFormulaAttribute>,
    target_attributes: Vec<CompactFormulaAttribute>,
    outcome: FormulaOutcome,
}

#[derive(Debug, Default, Serialize)]
struct FormulaBasisPointMultiplierCheck {
    proof_authority: bool,
    hypothesis: &'static str,
    evaluated_normal_comparisons: u64,
    exact_normal_comparisons: u64,
    evaluated_amount_comparisons: u64,
    exact_amount_comparisons: u64,
    normal_cross_product_residuals: Vec<FormulaResidualCount>,
    amount_cross_product_residuals: Vec<FormulaResidualCount>,
}

#[derive(Debug, Serialize)]
struct FormulaResidualCount {
    residual: i128,
    comparisons: u64,
}

#[derive(Debug, Default, Serialize)]
struct FormulaControlledScopeReport {
    proof_authority: bool,
    controlled_groups: u64,
    controlled_samples: u64,
    transitions: Vec<FormulaValueTransitionCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct FormulaValueTransition {
    candidate_from: i64,
    candidate_to: i64,
    normal_from: Option<i64>,
    normal_to: Option<i64>,
    lucky_from: Option<i64>,
    lucky_to: Option<i64>,
    amount_from: i64,
    amount_to: i64,
    actual_from: Option<i64>,
    actual_to: Option<i64>,
    hp_loss_from: Option<i64>,
    hp_loss_to: Option<i64>,
    shield_loss_from: Option<i64>,
    shield_loss_to: Option<i64>,
}

#[derive(Debug, Serialize)]
struct FormulaValueTransitionCount {
    #[serde(flatten)]
    transition: FormulaValueTransition,
    comparisons: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct FormulaOutcome {
    normal: Option<i64>,
    lucky: Option<i64>,
    amount: i64,
    actual: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FormulaControlKey {
    ability_id: i64,
    passive_uuid: Option<u32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    packet_inputs: String,
    source_attributes: Vec<CompactFormulaAttribute>,
    target_attributes: Vec<CompactFormulaAttribute>,
    source_statuses: Vec<CompactFormulaStatus>,
    target_statuses: Vec<CompactFormulaStatus>,
    status_provider_attributes: Vec<CompactFormulaProviderAttributeState>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct SeasonVector {
    source_season_strength: Option<i64>,
    source_target_season_input: Option<i64>,
    target_season_strength: Option<i64>,
    target_target_season_input: Option<i64>,
    source_minus_target_season_strength: Option<i64>,
}

#[derive(Debug, Serialize)]
struct SeasonVectorCount {
    #[serde(flatten)]
    vector: SeasonVector,
    samples: u64,
}

#[derive(Debug, Serialize)]
struct SeasonTargetInputProof {
    proof_authority: bool,
    source_attribute_id: i32,
    target_attribute_id: i32,
    samples: u64,
    samples_with_source_target_input: u64,
    samples_with_target_season_strength: u64,
    comparable_samples: u64,
    exact_match_samples: u64,
    mismatch_samples: u64,
    source_target_input_without_target_strength_samples: u64,
    target_strength_without_source_target_input_samples: u64,
    distinct_mismatches: Vec<SeasonTargetInputMismatchCount>,
    conclusion: &'static str,
    damage_formula_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct SeasonTargetInputMismatchCount {
    source_target_input: i64,
    target_season_strength: i64,
    samples: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CompactFormulaAttribute {
    attribute_id: i32,
    value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CompactFormulaStatus {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CompactFormulaProviderAttributeState {
    provider_entity_uuid: i64,
    attribute_state_id: Option<u32>,
}

#[derive(Debug)]
struct StatusProviderAttributeSnapshot {
    provider_entity_uuid: i64,
    state_at_wire_message_start: Option<StateSnapshot>,
}

#[derive(Debug, Serialize)]
struct HpShieldDependencyInventoryBundle<'a> {
    schema_version: u16,
    generated_by: &'static str,
    game_build: &'a str,
    policy: &'static str,
    inputs: &'a [String],
    summary: HpShieldDependencyInventorySummary,
    abilities: &'a [HpShieldDependencyCandidateReport],
}

#[derive(Debug, Default, Serialize)]
struct HpShieldDependencyInventorySummary {
    abilities: usize,
    exact_formula_proved_at_aggregate_level: usize,
    cross_state_fixed_point_candidates: usize,
    repeated_single_state_candidates: usize,
    event_order_target_state_consequences_observed: usize,
    packet_state_observed_formula_unresolved: usize,
    packet_state_unavailable_in_current_captures: usize,
    retained_abilities: usize,
    runtime_rdps_attribution_enabled: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum StateDependencyEvidenceStatus {
    ExactFormulaProvedAtAggregateLevel,
    CrossStateFixedPointCandidate,
    RepeatedSingleStateCandidate,
    EventOrderTargetStateConsequenceObserved,
    PacketStateObservedFormulaUnresolved,
    PacketStateUnavailableInCurrentCaptures,
}

#[derive(Debug, Serialize)]
struct HpShieldDependencyCandidateReport {
    ability_id: i64,
    presentation_kind: Option<String>,
    presentation_resolution: Option<String>,
    recount_group_id: Option<i64>,
    damage_events: u64,
    evidence_status: StateDependencyEvidenceStatus,
    retained_in_canonical_timeline: bool,
    runtime_rdps_attribution_enabled: bool,
    exact_formula_proof: Option<&'static str>,
    state_observations: Vec<StateObservationCount>,
    wire_to_event_state_transitions: Vec<StateTransitionReport>,
    source_hp_transition_semantics: SourceHpTransitionReport,
    fixed_point_candidates: Vec<FixedPointCandidate>,
    interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct StateObservationCount {
    timing: StateObservationTiming,
    locus: &'static str,
    events: u64,
}

#[derive(Debug, Serialize)]
struct StateTransitionReport {
    locus: &'static str,
    events_with_both_states: u64,
    unchanged_events: u64,
    increased_events: u64,
    decreased_events: u64,
    signed_change_examples: Vec<StateTransitionCount>,
    amount_to_absolute_change_basis_points: Vec<RatioCount>,
    normal_to_absolute_change_basis_points: Vec<RatioCount>,
    post_event_consequence_risk: bool,
    interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct StateTransitionCount {
    signed_change: i64,
    count: u64,
}

#[derive(Debug, Clone, Serialize)]
struct SourceHpTransitionReport {
    events_with_current_and_max_hp_at_both_timings: u64,
    semantic_counts: Vec<SourceHpTransitionSemanticCount>,
    signed_change_examples: Vec<SourceHpTransitionCount>,
    hp_dependent_events_retained: bool,
    interpretation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SourceHpTransitionSemanticCount {
    semantic: SourceHpTransitionSemantic,
    count: u64,
}

#[derive(Debug, Clone, Serialize)]
struct SourceHpTransitionCount {
    current_hp_change: i64,
    max_hp_change: i64,
    missing_hp_change: i64,
    count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum StateObservationTiming {
    EventOrder,
    WireMessageStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FixedPointCandidateStrength {
    CrossState,
    RepeatedSingleState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FixedPointTimingAssessment {
    PresentAtBothPacketTimings,
    WireMessageStartOnly,
    EventOrderOnly,
    EventOrderTargetStatePostHitConsequenceRisk,
}

#[derive(Debug, Serialize)]
struct FixedPointCandidate {
    timing: StateObservationTiming,
    locus: &'static str,
    numerator: &'static str,
    basis_points_floor: i64,
    count: u64,
    locus_observation_events: u64,
    coverage_basis_points: u64,
    distinct_numerators: usize,
    distinct_denominators: usize,
    strength: FixedPointCandidateStrength,
    retained_for_evidence: bool,
    matching_other_timing_candidate: bool,
    eligible_for_formula_investigation: bool,
    post_hit_consequence_risk: bool,
    timing_assessment: FixedPointTimingAssessment,
}

#[derive(Debug, Serialize)]
struct ProofPolicy {
    packet_passive_uuid_is_exact_source_evidence: bool,
    active_status_is_formula_identity_proof: bool,
    hp_shield_or_resource_state_is_discarded: bool,
    outgoing_damage_attributes_are_discarded: bool,
    all_packet_observed_abilities: bool,
    all_packet_attribute_examples: &'static str,
    state_timing_policy: &'static str,
    ratio_semantics: &'static str,
    compact_candidate_promotion: &'static str,
    enablement_rule: &'static str,
    formula_lifecycle_completeness: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct ProofSummary {
    status_events_scanned: u64,
    damage_events_scanned: u64,
    selected_ability_damage_events: u64,
    selected_passive_source_damage_events: u64,
    selected_effect_status_events: u64,
    selected_effect_damage_events_while_active: u64,
    selected_passive_damage_events_with_source_max_hp: u64,
    selected_effect_damage_events_with_source_max_hp: u64,
    selected_effect_damage_events_with_source_physical_defense: u64,
    selected_effect_damage_events_with_target_max_hp: u64,
}

#[derive(Debug, Serialize)]
struct PassiveSourceReport {
    source_entity_id: i64,
    damage_events: u64,
    actor_keys: Vec<String>,
    abilities: Vec<AbilityReport>,
}

#[derive(Debug, Serialize)]
struct StatusEffectReport {
    effect_id: i64,
    status_events: u64,
    actor_keys: Vec<String>,
    source_damage_events_while_active: u64,
    direct_source_damage_events_while_active: u64,
    packet_origins: Vec<PacketOriginCount>,
    status_examples: Vec<StatusExample>,
    abilities: Vec<AbilityReport>,
}

#[derive(Debug, Serialize)]
struct PacketOriginCount {
    source_type_id: i32,
    source_config_id: i64,
    count: u64,
}

#[derive(Debug, Clone, Serialize)]
struct StatusExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    effect_id: i64,
    state: StatusState,
    source_entity_uuid: Option<i64>,
    source_name: Option<String>,
    source_class_id: Option<i32>,
    source_specialization_id: Option<i32>,
    target_entity_uuid: i64,
    target_name: Option<String>,
    target_class_id: Option<i32>,
    target_specialization_id: Option<i32>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    duration_millis: Option<u64>,
}

#[derive(Debug, Serialize)]
struct AbilityReport {
    ability_id: i64,
    events: u64,
    amount_sum: String,
    events_with_normal_value: u64,
    event_state_ratios: StateRatioReport,
    wire_start_state_ratios: StateRatioReport,
    wire_to_event_state_transitions: Vec<StateTransitionReport>,
    source_hp_transition_semantics: SourceHpTransitionReport,
    source_status_signatures: StatusSignatureReport,
    target_status_signatures: StatusSignatureReport,
    shield_provider_observations: Vec<ShieldProviderReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bash_pursuit_formula: Option<BashPursuitFormulaReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    judgment_pursuit_formula: Option<JudgmentPursuitFormulaReport>,
    examples: Vec<DamageExample>,
}

#[derive(Debug, Serialize)]
struct BashPursuitFormulaReport {
    formula_status: &'static str,
    formula: &'static str,
    packet_input_timing: &'static str,
    max_hp_coefficient_basis_points: i64,
    current_shield_coefficient_basis_points: i64,
    current_shield_input_cap_basis_points_of_max_hp: i64,
    events_with_exact_packet_inputs: u64,
    events_without_exact_packet_inputs: u64,
    events_using_shield_cap: u64,
    formula_base_sum: String,
    best_exact_snapshot_formula_base_sum: String,
    normal_to_formula_base_basis_points: Vec<RatioCount>,
    amount_to_formula_base_basis_points: Vec<RatioCount>,
    amount_to_best_exact_snapshot_formula_base_basis_points: Vec<RatioCount>,
    events_with_exact_inferred_transient_snapshot: u64,
    events_with_resolved_transient_provider_ownership: u64,
    events_with_unresolved_transient_provider_ownership: u64,
    inferred_transient_snapshot_examples: Vec<InferredTransientBashPursuitExample>,
    external_provider_counterfactuals: Vec<ExternalShieldCounterfactualReport>,
}

#[derive(Debug, Serialize)]
struct JudgmentPursuitFormulaReport {
    formula_status: &'static str,
    formula: &'static str,
    calculation_time_state_policy: &'static str,
    events: u64,
    events_with_exact_integer_solution: u64,
    events_without_exact_integer_solution: u64,
    events_matching_wire_start_max_hp: u64,
    events_matching_event_order_max_hp: u64,
    events_matching_both_packet_states: u64,
    distinct_inferred_calculation_time_max_hp_values: usize,
    inferred_calculation_time_max_hp_values: Vec<i64>,
    runtime_rdps_attribution_enabled: bool,
    provider_attribution_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ExternalShieldCounterfactualReport {
    provider_relation: ShieldProviderRelation,
    shield_type: Option<i32>,
    effect_id: Option<i64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    damage_events: u64,
    events_with_exact_current_value: u64,
    events_without_exact_current_value: u64,
    events_with_exact_zero_without_provider_current_value: u64,
    provider_current_shield_sum: String,
    provider_removed_formula_base_delta_sum: String,
    events_with_positive_formula_base_delta: u64,
    events_with_zero_formula_base_delta_due_to_cap: u64,
    exact_zero_contribution_when_formula_base_delta_is_zero: bool,
    positive_final_damage_attribution_policy: &'static str,
    provider_entity_uuids: Vec<i64>,
    recipient_entity_uuids: Vec<i64>,
    shield_uuid_examples: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct ShieldProviderReport {
    provider_relation: ShieldProviderRelation,
    shield_type: Option<i32>,
    effect_id: Option<i64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    damage_events: u64,
    shield_instance_observations: u64,
    observed_damage_amount_sum: String,
    current_value_observations: u64,
    current_value_sum: String,
    current_value_per_event_min: Option<i64>,
    current_value_per_event_max: Option<i64>,
    distinct_shield_uuids: usize,
    shield_uuid_examples: Vec<i64>,
    provider_entity_uuids: Vec<i64>,
    damage_source_entity_uuids: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct StateRatioReport {
    events_with_source_current_hp: u64,
    events_with_source_max_hp: u64,
    events_with_source_current_shield: u64,
    events_with_source_max_hp_plus_three_current_shield: u64,
    events_with_source_missing_hp: u64,
    events_with_source_physical_defense: u64,
    events_with_target_current_hp: u64,
    events_with_target_max_hp: u64,
    events_with_target_missing_hp: u64,
    amount_to_source_current_hp_basis_points: Vec<RatioCount>,
    normal_to_source_max_hp_basis_points: Vec<RatioCount>,
    amount_to_source_max_hp_basis_points: Vec<RatioCount>,
    normal_to_source_current_shield_basis_points: Vec<RatioCount>,
    amount_to_source_current_shield_basis_points: Vec<RatioCount>,
    normal_to_source_max_hp_plus_three_current_shield_basis_points: Vec<RatioCount>,
    amount_to_source_max_hp_plus_three_current_shield_basis_points: Vec<RatioCount>,
    normal_to_source_missing_hp_basis_points: Vec<RatioCount>,
    amount_to_source_missing_hp_basis_points: Vec<RatioCount>,
    normal_to_source_physical_defense_basis_points: Vec<RatioCount>,
    amount_to_source_physical_defense_basis_points: Vec<RatioCount>,
    amount_to_target_current_hp_basis_points: Vec<RatioCount>,
    normal_to_target_max_hp_basis_points: Vec<RatioCount>,
    amount_to_target_max_hp_basis_points: Vec<RatioCount>,
    normal_to_target_missing_hp_basis_points: Vec<RatioCount>,
    amount_to_target_missing_hp_basis_points: Vec<RatioCount>,
    amount_to_source_max_hp_near_integer_multiples: Vec<NearIntegerMultipleCount>,
    amount_to_source_physical_defense_near_integer_multiples: Vec<NearIntegerMultipleCount>,
}

#[derive(Debug, Serialize)]
struct RatioCount {
    basis_points_floor: i64,
    count: u64,
    distinct_numerators: usize,
    distinct_denominators: usize,
    numerator_examples: Vec<i64>,
    denominator_examples: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct NearIntegerMultipleCount {
    multiplier: i64,
    residual: i64,
    count: u64,
}

#[derive(Debug, Serialize)]
struct StatusSignatureCount {
    effect_ids: Vec<i64>,
    count: u64,
}

#[derive(Debug, Serialize)]
struct StatusSignatureReport {
    unique_signatures: usize,
    observations: u64,
    top_signatures: Vec<StatusSignatureCount>,
}

#[derive(Debug, Clone, Serialize)]
struct DamageExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    source_name: Option<String>,
    source_class_id: Option<i32>,
    source_specialization_id: Option<i32>,
    target_entity_uuid: i64,
    ability_id: i64,
    passive_uuid: Option<u32>,
    amount: i64,
    actual_amount: Option<i64>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    packet: DamagePacketDetail,
    source_state_at_wire_message_start: Option<StateSnapshot>,
    target_state_at_wire_message_start: Option<StateSnapshot>,
    source_shield_origins_at_wire_message_start: Option<Vec<ShieldOriginEvidence>>,
    source_state: StateSnapshot,
    target_state: StateSnapshot,
    source_shield_origins: Vec<ShieldOriginEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bash_pursuit_formula: Option<BashPursuitFormulaEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    judgment_pursuit_formula: Option<JudgmentPursuitFormulaEvent>,
    critical: Option<bool>,
    lucky: Option<bool>,
    active_source_statuses: Vec<ActiveStatusEvidence>,
    active_direct_source_statuses: Vec<ActiveStatusEvidence>,
    active_target_statuses: Vec<ActiveStatusEvidence>,
    source_statuses_at_wire_message_start: Option<Vec<ActiveStatusEvidence>>,
    target_statuses_at_wire_message_start: Option<Vec<ActiveStatusEvidence>>,
    active_selected_effects: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct BashPursuitFormulaEvent {
    max_hp: i64,
    current_shield_total: i64,
    current_shield_input_cap: i64,
    capped_current_shield_input: i64,
    formula_base_before_later_multipliers: i64,
    normal_to_formula_base_basis_points_floor: Option<i64>,
    amount_to_formula_base_basis_points_floor: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    inferred_transient_snapshot: Option<InferredTransientBashPursuitSnapshot>,
    external_provider_counterfactuals: Vec<ExternalShieldCounterfactualEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct JudgmentPursuitFormulaEvent {
    inferred_calculation_time_max_hp: i64,
    max_hp_multiplier: i64,
    integer_offset: i64,
    exact_recomputed_amount: i64,
    exact_amount_match: bool,
    wire_start_max_hp: Option<i64>,
    event_order_max_hp: Option<i64>,
    matches_wire_start_max_hp: bool,
    matches_event_order_max_hp: bool,
    calculation_time_state_evidence: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct InferredTransientBashPursuitSnapshot {
    inference_status: &'static str,
    inference_basis: &'static str,
    max_hp_basis_attribute_id: i32,
    max_hp_basis_value: i64,
    observed_wire_start_current_shield_total: i64,
    observed_event_order_current_shield_total: Option<i64>,
    inferred_current_shield_total: i64,
    wire_start_to_inferred_shield_delta: i64,
    inferred_to_event_order_shield_delta: Option<i64>,
    current_shield_input_cap: i64,
    inferred_formula_base_before_later_multiplier: i64,
    later_multiplier_numerator: i64,
    later_multiplier_denominator: i64,
    exact_recomputed_amount: i64,
    exact_amount_match: bool,
    observed_wire_start_shields: Vec<ShieldInstanceSnapshot>,
    observed_event_order_shields: Option<Vec<ShieldInstanceSnapshot>>,
    candidate_transient_instance_bounds: Vec<ShieldInstanceTransientBounds>,
    transient_instance_allocation_constraint: &'static str,
    transient_instance_allocation_unique: bool,
    shield_provider_decomposition_status: &'static str,
    candidate_provider_entity_uuids: Vec<i64>,
    provider_ownership_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_provider_entity_uuid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_provider_relation: Option<ShieldProviderRelation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_rdps_transfer_required: Option<bool>,
    provider_attribution_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransientShieldProviderOwnership {
    candidate_provider_entity_uuids: Vec<i64>,
    provider_ownership_complete: bool,
    resolved_provider_entity_uuid: Option<i64>,
    resolved_provider_relation: Option<ShieldProviderRelation>,
    external_rdps_transfer_required: Option<bool>,
    status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct ShieldInstanceTransientBounds {
    shield_uuid: Option<i64>,
    shield_type: Option<i32>,
    observed_wire_start_current_value: i64,
    observed_event_order_current_value: i64,
    inferred_current_value_min: i64,
    inferred_current_value_max: i64,
    inferred_delta_from_wire_min: i64,
    inferred_delta_from_wire_max: i64,
}

#[derive(Debug, Clone, Serialize)]
struct InferredTransientBashPursuitExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    amount: i64,
    snapshot: InferredTransientBashPursuitSnapshot,
}

#[derive(Debug, Clone, Serialize)]
struct ExternalShieldCounterfactualEvent {
    shield_type: Option<i32>,
    effect_id: Option<i64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    provider_entity_uuids: Vec<i64>,
    shield_uuids: Vec<i64>,
    known_non_provider_current_shield_lower_bound: i64,
    provider_current_shield: Option<i64>,
    current_shield_without_provider: Option<i64>,
    capped_current_shield_without_provider: Option<i64>,
    provider_removed_formula_base_delta: Option<i64>,
    zero_due_to_existing_shield_cap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zero_proof_basis: Option<&'static str>,
}

#[derive(Debug, Default, Clone, Serialize)]
struct StateSnapshot {
    current_hp: Option<StateScalar>,
    max_hp_final: Option<StateScalar>,
    max_hp_total: Option<StateScalar>,
    max_hp_add: Option<StateScalar>,
    max_hp_extra_add: Option<StateScalar>,
    max_hp_percent: Option<StateScalar>,
    max_hp_extra_percent: Option<StateScalar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_shields: Option<ShieldListSnapshot>,
    current_shield_total: Option<i64>,
    physical_defense_final: Option<StateScalar>,
    physical_defense_total: Option<StateScalar>,
    physical_defense_add: Option<StateScalar>,
    physical_defense_extra_add: Option<StateScalar>,
    physical_defense_percent: Option<StateScalar>,
    physical_defense_extra_percent: Option<StateScalar>,
    current_energy: Option<StateScalar>,
    max_energy_final: Option<StateScalar>,
    max_energy_total: Option<StateScalar>,
    max_energy_add: Option<StateScalar>,
    max_energy_extra_add: Option<StateScalar>,
    max_energy_percent: Option<StateScalar>,
    max_energy_extra_percent: Option<StateScalar>,
    shield_add_percent_family: Vec<StateScalar>,
    shield_gain_percent_family: Vec<StateScalar>,
    shield_damage_percent_family: Vec<StateScalar>,
    shield_damage_reduction_percent_family: Vec<StateScalar>,
    outgoing_damage_formula_attributes: Vec<StateScalar>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    all_packet_attributes: Vec<StateScalar>,
}

#[derive(Debug, Clone, Serialize)]
struct StateScalar {
    attribute_id: i32,
    raw_length: usize,
    raw_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_hex: Option<String>,
    integer_varint: Option<i64>,
    float32_little_endian: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decoded_shield_list: Option<ShieldListSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ActiveStatusKey {
    effect_id: i64,
    instance_id: i64,
    source_entity_uuid: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ActiveStatusMetadata {
    stacks: Option<u32>,
    level: Option<i32>,
    duration_millis: Option<u64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    last_observed_micros: u64,
    expires_at_observed_micros: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WireMessageKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ActiveStatusEvidence {
    effect_id: i64,
    instance_id: Option<i64>,
    source_entity_uuid: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    duration_millis: Option<u64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    last_observed_micros: u64,
    expires_at_observed_micros: Option<u64>,
}

fn merge_active_status_metadata(
    previous: Option<ActiveStatusMetadata>,
    incoming: ActiveStatusMetadata,
) -> ActiveStatusMetadata {
    ActiveStatusMetadata {
        stacks: incoming
            .stacks
            .or_else(|| previous.and_then(|metadata| metadata.stacks)),
        level: incoming
            .level
            .or_else(|| previous.and_then(|metadata| metadata.level)),
        duration_millis: incoming
            .duration_millis
            .or_else(|| previous.and_then(|metadata| metadata.duration_millis)),
        origin_source_type_id: incoming
            .origin_source_type_id
            .or_else(|| previous.and_then(|metadata| metadata.origin_source_type_id)),
        origin_source_config_id: incoming
            .origin_source_config_id
            .or_else(|| previous.and_then(|metadata| metadata.origin_source_config_id)),
        last_observed_micros: incoming.last_observed_micros,
        expires_at_observed_micros: incoming.expires_at_observed_micros,
    }
}

fn update_active_status(
    active: &mut BTreeMap<ActiveStatusKey, ActiveStatusMetadata>,
    key: ActiveStatusKey,
    incoming: ActiveStatusMetadata,
    state: StatusState,
) {
    let mut metadata = merge_active_status_metadata(active.get(&key).copied(), incoming);
    metadata.expires_at_observed_micros = metadata.duration_millis.map(|duration_millis| {
        metadata
            .last_observed_micros
            .saturating_add(duration_millis.saturating_mul(1_000))
    });
    match state {
        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
            active.insert(key, metadata);
        }
        // BPSR uses Consumed for a stack decrement as well as final consumption.
        // The effect remains active until the packet reports zero remaining stacks.
        StatusState::Consumed if metadata.stacks != Some(0) => {
            active.insert(key, metadata);
        }
        StatusState::Consumed | StatusState::Removed => {
            if active.remove(&key).is_none() {
                active.retain(|candidate, _| candidate.effect_id != key.effect_id);
            }
        }
    }
}

fn status_is_active_at(metadata: &ActiveStatusMetadata, observed_micros: u64) -> bool {
    metadata.last_observed_micros <= observed_micros
        && metadata
            .expires_at_observed_micros
            .is_none_or(|expires_at| observed_micros <= expires_at)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShieldOriginRecord {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct ShieldOriginEvidence {
    shield_uuid: Option<i64>,
    shield_type: Option<i32>,
    current_value: Option<i64>,
    effect_id: Option<i64>,
    source_entity_uuid: Option<i64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR state-scaling damage proof failed: {error}");
        std::process::exit(1);
    }
}

fn ensure_formula_status_lifecycle(
    status_events_scanned: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    if status_events_scanned == 0 {
        return Err("formula proof inputs contain no status lifecycle events; a damage-only compact replay cannot prove that otherwise-identical damage contexts have identical status state".into());
    }
    Ok(())
}

fn common_input_game_build(paths: &[PathBuf]) -> Result<String, Box<dyn std::error::Error>> {
    let mut expected: Option<(String, &Path)> = None;
    for path in paths {
        let reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        let build = reader.header().region.client_build.trim();
        if build.is_empty() {
            return Err(format!(
                "{} has an empty client build in its rlog header",
                path.display()
            )
            .into());
        }
        if let Some((expected_build, expected_path)) = &expected {
            if build != expected_build {
                return Err(format!(
                    "input build mismatch: {} declares {}, while {} declares {}",
                    expected_path.display(),
                    expected_build,
                    path.display(),
                    build
                )
                .into());
            }
        } else {
            expected = Some((build.to_owned(), path));
        }
    }
    expected
        .map(|(build, _)| build)
        .ok_or_else(|| "at least one rlog input is required".into())
}

fn normalized_input_path(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

fn normalized_file_label(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| path.to_string_lossy().to_ascii_lowercase())
}

fn formula_selection_receipt(args: &Arguments) -> FormulaSelectionReceipt {
    let selected_effect_ids = args
        .formula_target_effects
        .iter()
        .copied()
        .collect::<Vec<_>>();
    FormulaSelectionReceipt {
        all_abilities: args.all_abilities,
        ability_ids: args.abilities.iter().copied().collect(),
        selected_effect_ids: selected_effect_ids.clone(),
        source_effect_ids: if args.formula_effect_locus == FormulaEffectLocus::Source {
            selected_effect_ids.clone()
        } else {
            Vec::new()
        },
        target_effect_ids: if args.formula_effect_locus == FormulaEffectLocus::Target {
            selected_effect_ids
        } else {
            Vec::new()
        },
        effect_locus: args.formula_effect_locus,
        target_effect_scope: match args.formula_effect_locus {
            FormulaEffectLocus::Source => {
                "exact numeric effect ID must be active on the canonical damage actor; provider identity, localized name, and remote-player action packets are not selection inputs"
            }
            FormulaEffectLocus::Target => {
                "exact numeric effect ID must be active on the damage target; provider identity, localized name, and remote-player action packets are not selection inputs"
            }
        },
        target_effect_timing: "wire-message start, before same-message status mutations; expiry is evaluated at the damage event observed time",
        gap_policy: if args.formula_gap_window_audit.is_some() {
            "selection is intersected with an exact-build canonical gap-window audit whose sealed RLOG identities and complete lifecycle totals are replay-validated; gap-bounded selection remains evidence rather than formula authority"
        } else {
            "this analyzer does not prove that a selected status lifecycle is gap-bounded; join the receipt to an exact-build canonical gap-window audit before using it as lifecycle or counterfactual authority"
        },
        formula_authority: false,
    }
}

fn load_formula_gap_window_filter(
    path: &Path,
    rlogs: &[PathBuf],
    game_build: &str,
    selected_effects: &BTreeSet<i64>,
    effect_locus: FormulaEffectLocus,
) -> Result<FormulaGapWindowFilter, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let bundle: FormulaGapWindowAuditBundle = serde_json::from_slice(&bytes)?;
    if !matches!(bundle.schema_version, 1..=3)
        || bundle.generated_by != "rlogs-bpsr-rlog-gap-window-audit"
        || bundle.game_build != game_build
        || selected_effects != &BTreeSet::from([bundle.effect_id])
        || bundle.damage_relationship != effect_locus
        || (effect_locus == FormulaEffectLocus::Source
            && (bundle.schema_version != 3 || !bundle.policy.damage_relationship_is_explicit))
        || (bundle.schema_version == 3 && !bundle.policy.damage_relationship_is_explicit)
        || !bundle
            .policy
            .every_data_gap_and_recorder_pause_is_an_exclusion_boundary
        || !bundle
            .policy
            .status_lifecycles_never_cross_exclusion_or_run_boundaries
        || !bundle.policy.packet_absence_is_not_zero
        || !bundle
            .policy
            .structurally_unobservable_remote_player_packets_are_not_acquisition_requirements
        || !bundle
            .policy
            .current_snapshots_are_never_backfilled_into_historical_windows
        || bundle.policy.formula_authority
        || bundle.policy.runtime_authority
        || bundle.policy.provider_rdps_credit_allowed
        || !bundle
            .summary
            .exact_gap_bounded_lifecycle_windows_identified
        || bundle.summary.formula_authority
        || bundle.summary.runtime_authority
        || bundle.summary.provider_rdps_credit_allowed
        || bundle.summary.source_rlog_count != rlogs.len() as u64
        || bundle.sessions.len() != rlogs.len()
    {
        return Err(
            "formula gap-window audit is not exact-build fail-closed evidence for the selected effect endpoint".into(),
        );
    }

    let actual_files = rlogs
        .iter()
        .map(|rlog| normalized_file_label(rlog))
        .collect::<BTreeSet<_>>();
    if actual_files.len() != rlogs.len() {
        return Err(
            "formula RLOG inputs must have unique file names for gap receipt binding".into(),
        );
    }

    let mut sessions_by_file = BTreeMap::new();
    let mut lifecycle_count = 0_u64;
    let mut audited_damage_events = 0_u64;
    for session in bundle.sessions {
        let label = normalized_file_label(Path::new(&session.path));
        if !actual_files.contains(&label) || sessions_by_file.contains_key(&label) {
            return Err(format!(
                "gap-window audit session {} does not uniquely match an input RLOG",
                session.path
            )
            .into());
        }
        let mut windows_by_target = BTreeMap::<i64, Vec<FormulaGapWindowAuditWindow>>::new();
        for window in session.complete_gap_bounded_windows {
            if !window.gap_bounded
                || window.controlled_counterfactual_pair_proven
                || window.formula_authority
                || window.terminal_envelope_sequence <= window.applied_envelope_sequence
                || window.terminal_observed_micros < window.applied_observed_micros
                || (bundle.schema_version == 3
                    && window.effect_endpoint_damage_role.as_deref()
                        != Some(match effect_locus {
                            FormulaEffectLocus::Source => "damage_actor",
                            FormulaEffectLocus::Target => "damage_target",
                        }))
            {
                return Err("gap-window audit contains an invalid or authoritative window".into());
            }
            lifecycle_count = lifecycle_count.saturating_add(1);
            audited_damage_events =
                audited_damage_events.saturating_add(window.damage_events_while_active);
            windows_by_target
                .entry(window.target_entity_uuid)
                .or_default()
                .push(window);
        }
        for windows in windows_by_target.values_mut() {
            windows.sort_by_key(|window| window.applied_envelope_sequence);
        }
        sessions_by_file.insert(
            label,
            FormulaGapWindowSessionFilter {
                session_id: session.session_id,
                sealed_content_sha256: session.sealed_content_sha256,
                windows_by_target,
            },
        );
    }
    if sessions_by_file.keys().cloned().collect::<BTreeSet<_>>() != actual_files
        || lifecycle_count
            != bundle
                .summary
                .selected_effect_complete_gap_bounded_lifecycle_count
        || audited_damage_events != bundle.summary.selected_effect_damage_events_while_active
    {
        return Err("gap-window audit session or window totals are inconsistent".into());
    }

    Ok(FormulaGapWindowFilter {
        metadata: FormulaGapWindowFilterMetadata {
            source: display_path(path),
            source_sha256: hex_bytes(&Sha256::digest(&bytes)),
            effect_id: bundle.effect_id,
            effect_locus,
            complete_gap_bounded_lifecycles: lifecycle_count,
            audited_damage_events_while_active: audited_damage_events,
            matched_damage_events: 0,
            matched_window_damage_memberships: 0,
            selection_policy: match effect_locus {
                FormulaEffectLocus::Source => {
                    "damage actor must equal the status recipient; session, sealed RLOG identity, canonical sequence, and observed time must fall strictly after apply and strictly before terminal inside a complete gap-bounded lifecycle; overlapping lifecycle memberships are conserved while each ordinary damage event is retained once; remote-player action packet presence is not required"
                }
                FormulaEffectLocus::Target => {
                    "damage target must equal the status recipient; session, sealed RLOG identity, canonical sequence, and observed time must fall strictly after apply and strictly before terminal inside a complete gap-bounded lifecycle; overlapping lifecycle memberships are conserved while each ordinary damage event is retained once; remote-player action packet presence is not required"
                }
            },
            formula_authority: false,
        },
        sessions_by_file,
        matched_damage_events: 0,
        matched_window_damage_memberships: 0,
    })
}

fn load_formula_transition_seed_filter(
    path: &Path,
    rlogs: &[PathBuf],
    game_build: &str,
    window_micros: u64,
) -> Result<FormulaTransitionSeedFilter, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let bundle: FormulaTransitionSeedBundle = serde_json::from_slice(&bytes)?;
    if bundle.schema_version != 1 {
        return Err(format!(
            "formula transition seed schema must be exactly 1, got {}",
            bundle.schema_version
        )
        .into());
    }
    if let Some(expected_game_build) = &bundle.expected_game_build
        && expected_game_build != game_build
    {
        return Err(format!(
            "formula transition seed build mismatch: seed expects {expected_game_build}, inputs declare {game_build}"
        )
        .into());
    }
    if !bundle.all_equation_occurrences_retained
        || bundle.exact_single_term_equation_occurrences != bundle.retained_transition_seeds
        || bundle.retained_transition_seeds != bundle.transitions.len() as u64
    {
        return Err("formula transition seed input is incomplete; every exact single-term equation occurrence must be retained".into());
    }
    if bundle.transitions.is_empty() {
        return Err("formula transition seed input contains no transitions".into());
    }

    let declared_inputs = bundle
        .source_rlogs
        .iter()
        .map(|value| normalized_input_path(value))
        .collect::<BTreeSet<_>>();
    let actual_inputs = rlogs
        .iter()
        .map(|value| normalized_input_path(&value.to_string_lossy()))
        .collect::<BTreeSet<_>>();
    if declared_inputs != actual_inputs {
        return Err(
            "formula transition seed source_rlogs must exactly match the supplied rlog inputs"
                .into(),
        );
    }

    let selected_effect_ids = bundle
        .selected_effect_ids
        .into_iter()
        .collect::<BTreeSet<_>>();
    let selected_attribute_ids = bundle
        .selected_attribute_ids
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut seeds_by_recipient = BTreeMap::<(String, u32, i64), Vec<u64>>::new();
    for transition in bundle.transitions {
        if !selected_effect_ids.contains(&transition.effect_id)
            || !selected_attribute_ids.contains(&transition.attribute_id)
        {
            return Err(format!(
                "transition effect {} attribute {} is absent from the seed selection",
                transition.effect_id, transition.attribute_id
            )
            .into());
        }
        if !declared_inputs.contains(&normalized_input_path(&transition.rlog)) {
            return Err(format!(
                "transition rlog {} is absent from source_rlogs",
                transition.rlog
            )
            .into());
        }
        seeds_by_recipient
            .entry((
                transition.session_id,
                transition.run_ordinal,
                transition.target_entity_uuid,
            ))
            .or_default()
            .push(transition.wire_observed_micros);
    }
    for seeds in seeds_by_recipient.values_mut() {
        seeds.sort_unstable();
    }

    let source_sha256 = hex_bytes(&Sha256::digest(&bytes));
    Ok(FormulaTransitionSeedFilter {
        metadata: FormulaTransitionSeedFilterMetadata {
            source: display_path(path),
            source_sha256,
            window_micros_before_and_after: window_micros,
            retained_transition_seeds: bundle.retained_transition_seeds,
            selected_effect_ids: selected_effect_ids.into_iter().collect(),
            selected_attribute_ids: selected_attribute_ids.into_iter().collect(),
            selection_policy: "damage source entity must equal the locally observed transition recipient in the same session and run, and event time must fall within the declared symmetric window; temporal adjacency selects a diagnostic cohort only and never proves causality or attribution",
            formula_authority: false,
        },
        seeds_by_recipient,
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_arguments(env::args_os().skip(1))?;
    let game_build = common_input_game_build(&args.rlogs)?;
    let mut formula_gap_window_filter = args
        .formula_gap_window_audit
        .as_deref()
        .map(|path| {
            load_formula_gap_window_filter(
                path,
                &args.rlogs,
                &game_build,
                &args.formula_target_effects,
                args.formula_effect_locus,
            )
        })
        .transpose()?;
    let formula_transition_seed_filter = args
        .formula_transition_seeds
        .as_deref()
        .map(|path| {
            load_formula_transition_seed_filter(
                path,
                &args.rlogs,
                &game_build,
                args.formula_transition_window_micros,
            )
        })
        .transpose()?;
    let mut summary = ProofSummary::default();
    let mut passive = args
        .source_entities
        .iter()
        .map(|id| (*id, SourceAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut effects = args
        .effects
        .iter()
        .map(|id| (*id, EffectAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut abilities = args
        .abilities
        .iter()
        .map(|id| (*id, AbilityAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut selected_sequence_examples = Vec::new();
    let mut formula_cohort = FormulaCohortAccumulator::default();

    for path in &args.rlogs {
        scan_rlog(
            path,
            &args,
            &mut summary,
            &mut abilities,
            &mut passive,
            &mut effects,
            &mut selected_sequence_examples,
            &mut formula_cohort,
            formula_transition_seed_filter.as_ref(),
            formula_gap_window_filter.as_mut(),
        )?;
    }
    if let Some(filter) = formula_gap_window_filter.as_mut() {
        filter.finish()?;
    }

    if args.formula_cohort_output.is_some() || args.formula_proof_output.is_some() {
        ensure_formula_status_lifecycle(summary.status_events_scanned)?;
    }

    let input_paths = args
        .rlogs
        .iter()
        .map(|path| display_path(path))
        .collect::<Vec<_>>();
    if let Some(output) = &args.effect_ability_inventory_output {
        let inventory = EffectAbilityInventoryBundle {
            schema_version: SCHEMA_VERSION,
            generated_by: "rlogs-bpsr-state-scaling-damage-proof",
            game_build: &game_build,
            policy: "exact selected numeric effect IDs and packet-observed outgoing ability IDs while the effect is active; compact candidate routing only, never formula, attribution, runtime, or UI authority",
            inputs: &input_paths,
            effects: effect_ability_inventory(&effects),
        };
        write_json(output, &inventory)?;
    }
    if args.proof_only {
        if let Some(output) = &args.formula_cohort_output {
            let cohort = FormulaCohortBundle {
                schema_version: SCHEMA_VERSION,
                generated_by: "rlogs-bpsr-state-scaling-damage-proof",
                game_build: &game_build,
                policy: FormulaCohortPolicy {
                    state_timing: "source and target attributes and statuses are the state at the start of the enclosing decoded wire message, before any attribute or status mutation in that message is applied",
                    attribute_retention: "the packet monster/config identity attribute 10 plus fight attributes 10000 through 39999 that decode as integer varints are retained as compact id/value pairs; arbitrary low-ID protobuf messages are not mislabeled as scalar values, and every raw payload remains in the lossless source rlog",
                    direct_source_attribute_retention: "when the packet attacker differs from the credited top-level source, its complete compact wire-start attribute state is retained separately; absence remains explicit and schema-47 counterfactual analysis excludes that sample rather than substituting the credited owner's state",
                    status_retention: "formula-relevant semantic status identity is retained as effect, provider, stack, level, and origin metadata for source and target; transient instance and timestamp fields remain in the lossless source rlog and are intentionally excluded from cohort interning",
                    status_provider_attribute_retention: "for every distinct provider referenced by the source or target status state, the provider's complete compact fight-attribute state at the same wire-message start is retained by entity UUID; absence of an observed provider state remains explicit and is never backfilled from a later snapshot",
                    actor_and_scene_identity_retention: "the most recent packet-observed ActorEvent numeric entity type, monster ID, stable character ID when available, class, specialization, and level are retained for source, direct source, and target together with the current packet-observed run scene ID; missing fields remain absent and are never backfilled from current character snapshots",
                    position_retention: "the last packet-observed source, direct-source, and target positions at the start of the enclosing wire message are retained with their original sequence, timestamp, and capture sequence; missing or stale positions remain explicit and are never synthesized",
                    packet_retention: "the complete canonical DamagePacketDetail is retained so hit flags and opaque decoded dimensions can be audited without rescanning the rlog; decoder-generated SkillEffect damage-array index/count remain evidence but are not formula identity",
                    formula_authority: false,
                },
                selection: formula_selection_receipt(&args),
                gap_window_filter: formula_gap_window_filter
                    .as_ref()
                    .map(|filter| &filter.metadata),
                transition_seed_filter: formula_transition_seed_filter
                    .as_ref()
                    .map(|filter| &filter.metadata),
                inputs: &input_paths,
                attribute_states: &formula_cohort.attribute_states,
                status_states: &formula_cohort.status_states,
                samples: &formula_cohort.samples,
            };
            write_compact_json(output, &cohort)?;
        }
        if let Some(output) = &args.formula_proof_output {
            let proof = formula_proof_bundle(
                &formula_cohort,
                input_paths,
                game_build.clone(),
                &args,
                formula_gap_window_filter.as_ref(),
            )?;
            write_json(output, &proof)?;
        }
        return Ok(());
    }

    let passive_sources = passive
        .into_iter()
        .map(|(source_entity_id, accumulator)| PassiveSourceReport {
            source_entity_id,
            damage_events: accumulator.events,
            actor_keys: accumulator.actor_keys.into_iter().collect(),
            abilities: ability_reports(accumulator.abilities),
        })
        .collect();
    let active_status_effects = effects
        .into_iter()
        .map(|(effect_id, accumulator)| StatusEffectReport {
            effect_id,
            status_events: accumulator.status_events,
            actor_keys: accumulator.actor_keys.into_iter().collect(),
            source_damage_events_while_active: accumulator.source_damage_events_while_active,
            direct_source_damage_events_while_active: accumulator
                .direct_source_damage_events_while_active,
            packet_origins: accumulator
                .packet_origins
                .into_iter()
                .map(
                    |((source_type_id, source_config_id), count)| PacketOriginCount {
                        source_type_id,
                        source_config_id,
                        count,
                    },
                )
                .collect(),
            status_examples: accumulator.status_examples,
            abilities: ability_reports(accumulator.abilities),
        })
        .collect();
    let hp_shield_dependency_inventory = hp_shield_dependency_inventory(&abilities)?;
    if let Some(output) = &args.inventory_output {
        let compact = HpShieldDependencyInventoryBundle {
            schema_version: SCHEMA_VERSION,
            generated_by: "rlogs-bpsr-state-scaling-damage-proof",
            game_build: &game_build,
            policy: "exhaustive packet-observed ability inventory; promotion is candidate prioritization only, never filtering; event-order-only target current/missing HP ratios remain retained but are labeled as possible post-hit consequences unless the same ratio exists at wire-message start; every ability and original canonical event remains retained",
            inputs: &input_paths,
            summary: hp_shield_dependency_inventory_summary(&hp_shield_dependency_inventory),
            abilities: &hp_shield_dependency_inventory,
        };
        write_json(output, &compact)?;
    }
    if let Some(output) = &args.formula_cohort_output {
        let cohort = FormulaCohortBundle {
            schema_version: SCHEMA_VERSION,
            generated_by: "rlogs-bpsr-state-scaling-damage-proof",
            game_build: &game_build,
            policy: FormulaCohortPolicy {
                state_timing: "source and target attributes and statuses are the state at the start of the enclosing decoded wire message, before any attribute or status mutation in that message is applied",
                attribute_retention: "the packet monster/config identity attribute 10 plus fight attributes 10000 through 39999 that decode as integer varints are retained as compact id/value pairs; arbitrary low-ID protobuf messages are not mislabeled as scalar values, and every raw payload remains in the lossless source rlog",
                direct_source_attribute_retention: "when the packet attacker differs from the credited top-level source, its complete compact wire-start attribute state is retained separately; absence remains explicit and schema-47 counterfactual analysis excludes that sample rather than substituting the credited owner's state",
                status_retention: "formula-relevant semantic status identity is retained as effect, provider, stack, level, and origin metadata for source and target; transient instance and timestamp fields remain in the lossless source rlog and are intentionally excluded from cohort interning",
                status_provider_attribute_retention: "for every distinct provider referenced by the source or target status state, the provider's complete compact fight-attribute state at the same wire-message start is retained by entity UUID; absence of an observed provider state remains explicit and is never backfilled from a later snapshot",
                actor_and_scene_identity_retention: "the most recent packet-observed ActorEvent numeric entity type, monster ID, stable character ID when available, class, specialization, and level are retained for source, direct source, and target together with the current packet-observed run scene ID; missing fields remain absent and are never backfilled from current character snapshots",
                position_retention: "the last packet-observed source, direct-source, and target positions at the start of the enclosing wire message are retained with their original sequence, timestamp, and capture sequence; missing or stale positions remain explicit and are never synthesized",
                packet_retention: "the complete canonical DamagePacketDetail is retained so hit flags and opaque decoded dimensions can be audited without rescanning the rlog; decoder-generated SkillEffect damage-array index/count remain evidence but are not formula identity",
                formula_authority: false,
            },
            selection: formula_selection_receipt(&args),
            gap_window_filter: formula_gap_window_filter
                .as_ref()
                .map(|filter| &filter.metadata),
            transition_seed_filter: formula_transition_seed_filter
                .as_ref()
                .map(|filter| &filter.metadata),
            inputs: &input_paths,
            attribute_states: &formula_cohort.attribute_states,
            status_states: &formula_cohort.status_states,
            samples: &formula_cohort.samples,
        };
        write_compact_json(output, &cohort)?;
    }
    if let Some(output) = &args.formula_proof_output {
        let proof = formula_proof_bundle(
            &formula_cohort,
            input_paths.clone(),
            game_build.clone(),
            &args,
            formula_gap_window_filter.as_ref(),
        )?;
        write_json(output, &proof)?;
    }
    let bundle = ProofBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-state-scaling-damage-proof",
        policy: ProofPolicy {
            packet_passive_uuid_is_exact_source_evidence: true,
            active_status_is_formula_identity_proof: false,
            hp_shield_or_resource_state_is_discarded: false,
            outgoing_damage_attributes_are_discarded: false,
            all_packet_observed_abilities: args.all_abilities,
            all_packet_attribute_examples: "retained for explicitly selected abilities and passive source IDs; exhaustive all-ability mode keeps every named state family and timing stage while the source rlog remains the lossless all-attribute authority",
            state_timing_policy: "event_state and wire_start_state ratios are reported separately, and signed wire-start-to-event HP, MaxHP, missing-HP, and shield transitions are retained; either state or transition may be a canonical serialization consequence or may precede the server calculation, so timing alone never proves a formula input; an event-order-only final or normal-value ratio to target current or missing HP is retained and explicitly marked as post-hit consequence risk",
            ratio_semantics: "floor(packet normal or final amount * 10000 / packet-current source or target current HP, missing HP, final Max HP, or source final physical Defense); every available family stage is retained and a ratio is observation evidence, not a formula assertion",
            compact_candidate_promotion: "positive fixed-point ratios only; cross-state candidates require at least two distinct numerators, two distinct state denominators, and 25% coverage at that exact timing/locus; repeated single-state candidates require at least three observations and 50% coverage; event-order-only final or normal-value ratios to target current or missing HP are retained as timing evidence but excluded from formula promotion unless matched at wire-message start; non-promoted ratios and every original event remain retained",
            enablement_rule: "requires an exact packet source identity plus repeated state-scaled numeric equality under controlled hit flags and target conditions",
            formula_lifecycle_completeness: "formula cohort and proof output abort when all supplied replay inputs contain zero status lifecycle events; damage-only compact replays cannot establish complete controlled status state",
        },
        inputs: input_paths,
        summary,
        hp_shield_dependency_inventory,
        selected_damage_abilities: ability_reports(abilities),
        passive_sources,
        active_status_effects,
        selected_sequence_examples,
    };
    write_json(
        args.output
            .as_deref()
            .ok_or("--output is required unless --proof-only is used")?,
        &bundle,
    )?;
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn write_compact_json<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = BufWriter::new(File::create(path)?);
    serde_json::to_writer(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn scan_rlog(
    path: &Path,
    args: &Arguments,
    summary: &mut ProofSummary,
    abilities: &mut BTreeMap<i64, AbilityAccumulator>,
    passive: &mut BTreeMap<i64, SourceAccumulator>,
    effects: &mut BTreeMap<i64, EffectAccumulator>,
    selected_sequence_examples: &mut Vec<DamageExample>,
    formula_cohort: &mut FormulaCohortAccumulator,
    formula_transition_seed_filter: Option<&FormulaTransitionSeedFilter>,
    mut formula_gap_window_filter: Option<&mut FormulaGapWindowFilter>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut observed_session_id = None::<String>;
    let mut run_ordinal = 0_u32;
    let mut scene_ids = HashMap::<u32, i32>::new();
    let mut actors = HashMap::<(u32, i64), ActorSnapshot>::new();
    let mut positions = HashMap::<(u32, i64), CompactFormulaPositionObservation>::new();
    let mut attributes = HashMap::<(u32, i64), BTreeMap<i32, StateScalar>>::new();
    let mut statuses =
        HashMap::<(u32, i64), BTreeMap<ActiveStatusKey, ActiveStatusMetadata>>::new();
    let mut shield_origins = HashMap::<(u32, i64), BTreeMap<i64, ShieldOriginRecord>>::new();
    let mut active_wire_message = None;
    let mut attributes_at_wire_message_start =
        HashMap::<(u32, i64), BTreeMap<i32, StateScalar>>::new();
    let mut positions_at_wire_message_start =
        HashMap::<(u32, i64), Option<CompactFormulaPositionObservation>>::new();
    let mut statuses_at_wire_message_start =
        HashMap::<(u32, i64), BTreeMap<ActiveStatusKey, ActiveStatusMetadata>>::new();
    let mut shield_origins_at_wire_message_start =
        HashMap::<(u32, i64), BTreeMap<i64, ShieldOriginRecord>>::new();

    while let Some(envelope) = reader.next_event()? {
        if let Some(expected) = &observed_session_id {
            if expected != &envelope.session_id {
                return Err(format!("{} contains multiple session IDs", path.display()).into());
            }
        } else {
            observed_session_id = Some(envelope.session_id.clone());
        }
        let wire_message = wire_message_key(&envelope.provenance.source);
        if wire_message != active_wire_message {
            active_wire_message = wire_message;
            attributes_at_wire_message_start.clear();
            positions_at_wire_message_start.clear();
            statuses_at_wire_message_start.clear();
            shield_origins_at_wire_message_start.clear();
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary {
                state, scene_id, ..
            } => {
                match state {
                    RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                    RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                    _ => {}
                }
                if let Some(scene_id) = scene_id {
                    scene_ids.insert(run_ordinal, scene_id.0);
                }
            }
            TimelineEventKind::Actor(actor) => {
                observe_actor(&mut actors, run_ordinal, actor);
            }
            TimelineEventKind::Position(position) => {
                let key = (run_ordinal, position.actor.entity_uuid.0);
                if active_wire_message.is_some() {
                    positions_at_wire_message_start
                        .entry(key)
                        .or_insert_with(|| positions.get(&key).cloned());
                }
                positions.insert(
                    key,
                    CompactFormulaPositionObservation {
                        x: position.x,
                        y: position.y,
                        z: position.z,
                        facing_radians: position.facing_radians,
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        wire_capture_sequence: active_wire_message
                            .map(|message| message.capture_sequence),
                    },
                );
            }
            TimelineEventKind::EntityAttributes(event) => {
                let actor_key = (run_ordinal, event.actor.entity_uuid.0);
                if active_wire_message.is_some() {
                    attributes_at_wire_message_start
                        .entry(actor_key)
                        .or_insert_with(|| attributes.get(&actor_key).cloned().unwrap_or_default());
                }
                let values = attributes.entry(actor_key).or_default();
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    values.clear();
                }
                for attribute in &event.attributes {
                    values.insert(attribute.attribute_id, preserve_attribute(attribute));
                }
            }
            TimelineEventKind::Status(status) => {
                summary.status_events_scanned = summary.status_events_scanned.saturating_add(1);
                let target_uuid = status.target.entity_uuid.0;
                let key = ActiveStatusKey {
                    effect_id: status.effect.0,
                    instance_id: status.instance_id.map(|value| value.0).unwrap_or(i64::MIN),
                    source_entity_uuid: status
                        .source
                        .map(|value| value.entity_uuid.0)
                        .unwrap_or(i64::MIN),
                };
                let metadata = ActiveStatusMetadata {
                    stacks: status.stacks,
                    level: status.level,
                    duration_millis: status.duration_millis,
                    origin_source_type_id: status.origin.map(|origin| origin.source_type_id),
                    origin_source_config_id: status.origin.map(|origin| origin.source_config_id),
                    last_observed_micros: envelope.time.observed_micros,
                    expires_at_observed_micros: None,
                };
                let target_key = (run_ordinal, target_uuid);
                if active_wire_message.is_some() {
                    statuses_at_wire_message_start
                        .entry(target_key)
                        .or_insert_with(|| statuses.get(&target_key).cloned().unwrap_or_default());
                    shield_origins_at_wire_message_start
                        .entry(target_key)
                        .or_insert_with(|| {
                            shield_origins.get(&target_key).cloned().unwrap_or_default()
                        });
                }
                if let Some(instance_id) = status.instance_id.map(|value| value.0) {
                    let record = ShieldOriginRecord {
                        effect_id: status.effect.0,
                        source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                        origin_source_type_id: status.origin.map(|origin| origin.source_type_id),
                        origin_source_config_id: status
                            .origin
                            .map(|origin| origin.source_config_id),
                    };
                    let origins = shield_origins.entry(target_key).or_default();
                    match status.state {
                        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                            origins.insert(instance_id, record);
                        }
                        StatusState::Consumed | StatusState::Removed => {
                            origins.entry(instance_id).or_insert(record);
                        }
                    }
                }
                let active = statuses.entry(target_key).or_default();
                update_active_status(active, key, metadata, status.state);
                if !args.effects.contains(&status.effect.0) {
                    continue;
                }
                let report = effects.get_mut(&status.effect.0).expect("selected effect");
                report.status_events = report.status_events.saturating_add(1);
                report
                    .actor_keys
                    .insert(actor_key(&envelope.session_id, run_ordinal, target_uuid));
                if let Some(origin) = status.origin {
                    let count = report
                        .packet_origins
                        .entry((origin.source_type_id, origin.source_config_id))
                        .or_default();
                    *count = count.saturating_add(1);
                }
                if report.status_examples.len() < args.example_limit {
                    let source_uuid = status.source.map(|value| value.entity_uuid.0);
                    let source_actor =
                        source_uuid.and_then(|uuid| actors.get(&(run_ordinal, uuid)));
                    let target_actor = actors.get(&(run_ordinal, target_uuid));
                    report.status_examples.push(StatusExample {
                        rlog: file_label(path),
                        session_id: envelope.session_id.clone(),
                        run_ordinal,
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        effect_id: status.effect.0,
                        state: status.state,
                        source_entity_uuid: source_uuid,
                        source_name: source_actor.and_then(|value| value.name.clone()),
                        source_class_id: source_actor.and_then(|value| value.class_id),
                        source_specialization_id: source_actor
                            .and_then(|value| value.specialization_id),
                        target_entity_uuid: target_uuid,
                        target_name: target_actor.and_then(|value| value.name.clone()),
                        target_class_id: target_actor.and_then(|value| value.class_id),
                        target_specialization_id: target_actor
                            .and_then(|value| value.specialization_id),
                        origin_source_type_id: status.origin.map(|value| value.source_type_id),
                        origin_source_config_id: status.origin.map(|value| value.source_config_id),
                        stacks: status.stacks,
                        level: status.level,
                        duration_millis: status.duration_millis,
                    });
                }
                summary.selected_effect_status_events =
                    summary.selected_effect_status_events.saturating_add(1);
            }
            TimelineEventKind::Damage(damage) => {
                summary.damage_events_scanned = summary.damage_events_scanned.saturating_add(1);
                let ability_id = damage.ability.map(|value| value.0).unwrap_or(0);
                let source_uuid = damage.source.entity_uuid.0;
                let direct_source_uuid = damage.direct_source.map(|value| value.entity_uuid.0);
                let target_uuid = damage.target.entity_uuid.0;
                let target_key = (run_ordinal, target_uuid);
                let formula_effect_entity_uuid = match args.formula_effect_locus {
                    FormulaEffectLocus::Source => source_uuid,
                    FormulaEffectLocus::Target => target_uuid,
                };
                let formula_effect_key = (run_ordinal, formula_effect_entity_uuid);
                let formula_ability_selected = if args.formula_target_effects.is_empty() {
                    args.all_abilities || args.abilities.contains(&ability_id)
                } else {
                    args.all_abilities
                        || args.abilities.is_empty()
                        || args.abilities.contains(&ability_id)
                };
                let formula_target_effect_selected = args.formula_target_effects.is_empty()
                    || has_active_selected_effect(
                        statuses_at_wire_message_start
                            .get(&formula_effect_key)
                            .or_else(|| statuses.get(&formula_effect_key)),
                        &args.formula_target_effects,
                        envelope.time.observed_micros,
                    );
                let formula_gap_window_selected = match formula_gap_window_filter.as_deref_mut() {
                    Some(filter) => filter.matches(
                        path,
                        &envelope.session_id,
                        formula_effect_entity_uuid,
                        envelope.sequence,
                        envelope.time.observed_micros,
                    ),
                    None => true,
                };
                let retain_formula_state = (args.formula_cohort_output.is_some()
                    || args.formula_proof_output.is_some())
                    && formula_ability_selected
                    && formula_target_effect_selected
                    && formula_gap_window_selected
                    && formula_transition_seed_filter.is_none_or(|filter| {
                        filter.matches(
                            &envelope.session_id,
                            run_ordinal,
                            source_uuid,
                            envelope.time.observed_micros,
                        )
                    })
                    && formula_cohort.samples.len() < args.formula_sample_limit;
                let formula_only = args.proof_only
                    && args.effect_ability_inventory_output.is_none()
                    && (args.formula_cohort_output.is_some()
                        || args.formula_proof_output.is_some());
                if formula_only && !retain_formula_state {
                    continue;
                }
                let source_attributes = attributes.get(&(run_ordinal, source_uuid));
                let target_attributes = attributes.get(&target_key);
                let actor = actors.get(&(run_ordinal, source_uuid));
                let active_source_statuses = active_statuses_for(
                    &statuses,
                    run_ordinal,
                    source_uuid,
                    envelope.time.observed_micros,
                );
                let active_direct_source_statuses = direct_source_uuid
                    .map(|uuid| {
                        active_statuses_for(
                            &statuses,
                            run_ordinal,
                            uuid,
                            envelope.time.observed_micros,
                        )
                    })
                    .unwrap_or_default();
                let active_target_statuses = active_statuses_for(
                    &statuses,
                    run_ordinal,
                    target_uuid,
                    envelope.time.observed_micros,
                );
                let active_effects = active_selected_effects(
                    &statuses,
                    run_ordinal,
                    source_uuid,
                    direct_source_uuid,
                    &args.effects,
                    envelope.time.observed_micros,
                );
                let retain_complete_source_state = damage
                    .packet
                    .passive_uuid
                    .map(i64::from)
                    .is_some_and(|value| args.source_entities.contains(&value))
                    || args.abilities.contains(&ability_id)
                    || args.sequences.contains(&envelope.sequence)
                    || retain_formula_state;
                let retain_complete_target_state = args.abilities.contains(&ability_id)
                    || args.sequences.contains(&envelope.sequence)
                    || retain_formula_state;
                let retain_source_wire_state = args.all_abilities || retain_complete_source_state;
                let retain_target_wire_state = args.all_abilities || retain_complete_target_state;
                let source_key = (run_ordinal, source_uuid);
                let source_state_at_wire_message_start = retain_source_wire_state.then(|| {
                    state_snapshot(
                        attributes_at_wire_message_start
                            .get(&source_key)
                            .or(source_attributes),
                        retain_complete_source_state,
                    )
                });
                let target_state_at_wire_message_start = retain_target_wire_state.then(|| {
                    state_snapshot(
                        attributes_at_wire_message_start
                            .get(&target_key)
                            .or(target_attributes),
                        retain_complete_target_state,
                    )
                });
                let source_shield_origins_at_wire_message_start =
                    source_state_at_wire_message_start.as_ref().map(|state| {
                        shield_origin_evidence(
                            state,
                            shield_origins_at_wire_message_start
                                .get(&source_key)
                                .or_else(|| shield_origins.get(&source_key)),
                        )
                    });
                let source_statuses_at_wire_message_start = retain_source_wire_state.then(|| {
                    active_statuses_from_set(
                        statuses_at_wire_message_start
                            .get(&source_key)
                            .or_else(|| statuses.get(&source_key)),
                        envelope.time.observed_micros,
                    )
                });
                let target_statuses_at_wire_message_start = retain_target_wire_state.then(|| {
                    active_statuses_from_set(
                        statuses_at_wire_message_start
                            .get(&target_key)
                            .or_else(|| statuses.get(&target_key)),
                        envelope.time.observed_micros,
                    )
                });
                let status_provider_attribute_snapshots = status_provider_attribute_snapshots(
                    run_ordinal,
                    source_statuses_at_wire_message_start.as_deref(),
                    target_statuses_at_wire_message_start.as_deref(),
                    &attributes_at_wire_message_start,
                    &attributes,
                );
                let source_state = state_snapshot(source_attributes, retain_complete_source_state);
                let source_shield_origins =
                    shield_origin_evidence(&source_state, shield_origins.get(&source_key));
                let mut example = DamageExample {
                    rlog: file_label(path),
                    session_id: envelope.session_id.clone(),
                    run_ordinal,
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    wire_capture_sequence: active_wire_message
                        .map(|message| message.capture_sequence),
                    source_entity_uuid: source_uuid,
                    direct_source_entity_uuid: direct_source_uuid,
                    source_name: actor.and_then(|value| value.name.clone()),
                    source_class_id: actor.and_then(|value| value.class_id),
                    source_specialization_id: actor.and_then(|value| value.specialization_id),
                    target_entity_uuid: target_uuid,
                    ability_id,
                    passive_uuid: damage.packet.passive_uuid,
                    amount: damage.amount,
                    actual_amount: damage.actual_amount,
                    normal_value: damage.packet.normal_value,
                    lucky_value: damage.packet.lucky_value,
                    hp_loss: damage.hp_loss,
                    shield_loss: damage.shield_loss,
                    hit_event_id: damage.hit_event_id,
                    damage_source: damage.damage_source,
                    damage_type: damage.damage_type,
                    packet: damage.packet.clone(),
                    source_state_at_wire_message_start,
                    target_state_at_wire_message_start,
                    source_shield_origins_at_wire_message_start,
                    source_state,
                    target_state: state_snapshot(target_attributes, retain_complete_target_state),
                    source_shield_origins,
                    bash_pursuit_formula: None,
                    judgment_pursuit_formula: None,
                    critical: damage.flags.critical,
                    lucky: damage.flags.lucky,
                    active_source_statuses,
                    active_direct_source_statuses,
                    active_target_statuses,
                    source_statuses_at_wire_message_start,
                    target_statuses_at_wire_message_start,
                    active_selected_effects: active_effects.clone(),
                };
                example.bash_pursuit_formula = bash_pursuit_formula_event(&example);
                example.judgment_pursuit_formula = judgment_pursuit_formula_event(&example);
                if retain_formula_state {
                    let source_position_at_wire_message_start = position_at_wire_message_start(
                        &positions_at_wire_message_start,
                        &positions,
                        source_key,
                    );
                    let direct_source_position_at_wire_message_start =
                        direct_source_uuid.and_then(|uuid| {
                            position_at_wire_message_start(
                                &positions_at_wire_message_start,
                                &positions,
                                (run_ordinal, uuid),
                            )
                        });
                    let target_position_at_wire_message_start = position_at_wire_message_start(
                        &positions_at_wire_message_start,
                        &positions,
                        target_key,
                    );
                    let direct_source_state_at_wire_message_start =
                        direct_source_uuid.and_then(|uuid| {
                            let key = (run_ordinal, uuid);
                            attributes_at_wire_message_start
                                .get(&key)
                                .or_else(|| attributes.get(&key))
                                .map(|values| state_snapshot(Some(values), true))
                        });
                    formula_cohort.push(
                        &example,
                        &status_provider_attribute_snapshots,
                        scene_ids.get(&run_ordinal).copied(),
                        actors.get(&(run_ordinal, source_uuid)),
                        direct_source_uuid.and_then(|uuid| actors.get(&(run_ordinal, uuid))),
                        actors.get(&target_key),
                        direct_source_state_at_wire_message_start.as_ref(),
                        source_position_at_wire_message_start,
                        direct_source_position_at_wire_message_start,
                        target_position_at_wire_message_start,
                    );
                }
                if formula_only {
                    continue;
                }
                if args.sequences.contains(&envelope.sequence) {
                    selected_sequence_examples.push(example.clone());
                }

                if args.all_abilities {
                    observe_damage(
                        abilities.entry(ability_id).or_default(),
                        &example,
                        args.example_limit,
                    );
                    summary.selected_ability_damage_events =
                        summary.selected_ability_damage_events.saturating_add(1);
                } else if let Some(report) = abilities.get_mut(&ability_id) {
                    observe_damage(report, &example, args.example_limit);
                    summary.selected_ability_damage_events =
                        summary.selected_ability_damage_events.saturating_add(1);
                }

                if let Some(passive_uuid) = damage.packet.passive_uuid.map(i64::from)
                    && let Some(report) = passive.get_mut(&passive_uuid)
                {
                    report.events = report.events.saturating_add(1);
                    report.actor_keys.insert(actor_key(
                        &envelope.session_id,
                        run_ordinal,
                        source_uuid,
                    ));
                    observe_damage(
                        report.abilities.entry(ability_id).or_default(),
                        &example,
                        args.example_limit,
                    );
                    summary.selected_passive_source_damage_events = summary
                        .selected_passive_source_damage_events
                        .saturating_add(1);
                    if state_integer(&example.source_state.max_hp_final).is_some() {
                        summary.selected_passive_damage_events_with_source_max_hp = summary
                            .selected_passive_damage_events_with_source_max_hp
                            .saturating_add(1);
                    }
                    if state_integer(&example.source_state.physical_defense_final).is_some() {
                        summary.selected_effect_damage_events_with_source_physical_defense =
                            summary
                                .selected_effect_damage_events_with_source_physical_defense
                                .saturating_add(1);
                    }
                }

                for effect_id in active_effects {
                    let report = effects.get_mut(&effect_id).expect("selected effect");
                    report.source_damage_events_while_active =
                        report.source_damage_events_while_active.saturating_add(1);
                    if direct_source_uuid.is_some_and(|direct| {
                        statuses.get(&(run_ordinal, direct)).is_some_and(|active| {
                            active.iter().any(|(key, metadata)| {
                                key.effect_id == effect_id
                                    && status_is_active_at(metadata, envelope.time.observed_micros)
                            })
                        })
                    }) {
                        report.direct_source_damage_events_while_active = report
                            .direct_source_damage_events_while_active
                            .saturating_add(1);
                    }
                    observe_damage(
                        report.abilities.entry(ability_id).or_default(),
                        &example,
                        args.example_limit,
                    );
                    summary.selected_effect_damage_events_while_active = summary
                        .selected_effect_damage_events_while_active
                        .saturating_add(1);
                    if state_integer(&example.source_state.max_hp_final).is_some() {
                        summary.selected_effect_damage_events_with_source_max_hp = summary
                            .selected_effect_damage_events_with_source_max_hp
                            .saturating_add(1);
                    }
                    if state_integer(&example.target_state.max_hp_final).is_some() {
                        summary.selected_effect_damage_events_with_target_max_hp = summary
                            .selected_effect_damage_events_with_target_max_hp
                            .saturating_add(1);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(filter) = formula_gap_window_filter {
        let replay = reader
            .summary()
            .ok_or("sealed RLOG replay summary is missing for gap-window validation")?;
        filter.validate_replay(
            path,
            observed_session_id
                .as_deref()
                .ok_or("RLOG has no canonical session ID")?,
            &replay.content_sha256,
        )?;
    }
    Ok(())
}

fn observe_actor(
    actors: &mut HashMap<(u32, i64), ActorSnapshot>,
    run_ordinal: u32,
    event: &ActorEvent,
) {
    let actor = actors
        .entry((run_ordinal, event.actor.entity_uuid.0))
        .or_default();
    actor.entity_type_id = Some(event.entity_type_id);
    if event.monster_id.is_some() {
        actor.monster_id = event.monster_id.map(|value| value.0);
    }
    if event.character_id.is_some() {
        actor.character_id.clone_from(&event.character_id);
    }
    if event.display_name.is_some() {
        actor.name.clone_from(&event.display_name);
    }
    if event.class_id.is_some() {
        actor.class_id = event.class_id;
    }
    if event.specialization_id.is_some() {
        actor.specialization_id = event.specialization_id;
    }
    if event.level.is_some() {
        actor.level = event.level;
    }
}

fn active_selected_effects(
    statuses: &HashMap<(u32, i64), BTreeMap<ActiveStatusKey, ActiveStatusMetadata>>,
    run_ordinal: u32,
    source_uuid: i64,
    direct_source_uuid: Option<i64>,
    selected_effects: &BTreeSet<i64>,
    observed_micros: u64,
) -> Vec<i64> {
    let mut effects = BTreeSet::new();
    for entity_uuid in [Some(source_uuid), direct_source_uuid]
        .into_iter()
        .flatten()
    {
        if let Some(active) = statuses.get(&(run_ordinal, entity_uuid)) {
            effects.extend(
                active
                    .iter()
                    .filter(|(_, metadata)| status_is_active_at(metadata, observed_micros))
                    .map(|(key, _)| key.effect_id)
                    .filter(|effect_id| selected_effects.contains(effect_id)),
            );
        }
    }
    effects.into_iter().collect()
}

fn has_active_selected_effect(
    active: Option<&BTreeMap<ActiveStatusKey, ActiveStatusMetadata>>,
    selected_effects: &BTreeSet<i64>,
    observed_micros: u64,
) -> bool {
    active.is_some_and(|entries| {
        entries.iter().any(|(key, metadata)| {
            selected_effects.contains(&key.effect_id)
                && status_is_active_at(metadata, observed_micros)
        })
    })
}

fn active_statuses_for(
    statuses: &HashMap<(u32, i64), BTreeMap<ActiveStatusKey, ActiveStatusMetadata>>,
    run_ordinal: u32,
    entity_uuid: i64,
    observed_micros: u64,
) -> Vec<ActiveStatusEvidence> {
    active_statuses_from_set(statuses.get(&(run_ordinal, entity_uuid)), observed_micros)
}

fn active_statuses_from_set(
    statuses: Option<&BTreeMap<ActiveStatusKey, ActiveStatusMetadata>>,
    observed_micros: u64,
) -> Vec<ActiveStatusEvidence> {
    statuses
        .into_iter()
        .flat_map(|active| active.iter())
        .filter(|(_, metadata)| status_is_active_at(metadata, observed_micros))
        .map(|(key, metadata)| ActiveStatusEvidence {
            effect_id: key.effect_id,
            instance_id: (key.instance_id != i64::MIN).then_some(key.instance_id),
            source_entity_uuid: (key.source_entity_uuid != i64::MIN)
                .then_some(key.source_entity_uuid),
            stacks: metadata.stacks,
            level: metadata.level,
            duration_millis: metadata.duration_millis,
            origin_source_type_id: metadata.origin_source_type_id,
            origin_source_config_id: metadata.origin_source_config_id,
            last_observed_micros: metadata.last_observed_micros,
            expires_at_observed_micros: metadata.expires_at_observed_micros,
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

fn bash_pursuit_formula_event(event: &DamageExample) -> Option<BashPursuitFormulaEvent> {
    if event.ability_id != BASH_PURSUIT_ABILITY_ID {
        return None;
    }
    let source_state = event
        .source_state_at_wire_message_start
        .as_ref()
        .unwrap_or(&event.source_state);
    let max_hp = state_integer(&source_state.max_hp_final).filter(|value| *value > 0)?;
    let current_shield_total = source_state
        .current_shield_total
        .filter(|value| *value >= 0)?;
    let origins = event
        .source_shield_origins_at_wire_message_start
        .as_deref()
        .unwrap_or(&event.source_shield_origins);
    let mut formula = evaluate_bash_pursuit_formula(
        max_hp,
        current_shield_total,
        event.normal_value,
        event.amount,
        event.source_entity_uuid,
        origins,
    )?;
    formula.inferred_transient_snapshot = infer_bash_pursuit_transient_snapshot(event);
    Some(formula)
}

fn judgment_pursuit_formula_event(event: &DamageExample) -> Option<JudgmentPursuitFormulaEvent> {
    if event.ability_id != JUDGMENT_PURSUIT_ABILITY_ID {
        return None;
    }
    let wire_start_max_hp = event
        .source_state_at_wire_message_start
        .as_ref()
        .and_then(|state| state_integer(&state.max_hp_final));
    let event_order_max_hp = state_integer(&event.source_state.max_hp_final);
    evaluate_judgment_pursuit_formula(event.amount, wire_start_max_hp, event_order_max_hp)
}

fn evaluate_judgment_pursuit_formula(
    amount: i64,
    wire_start_max_hp: Option<i64>,
    event_order_max_hp: Option<i64>,
) -> Option<JudgmentPursuitFormulaEvent> {
    let adjusted_amount = amount.checked_add(JUDGMENT_PURSUIT_INTEGER_OFFSET)?;
    if adjusted_amount <= 0 || adjusted_amount % JUDGMENT_PURSUIT_MAX_HP_MULTIPLIER != 0 {
        return None;
    }
    let inferred_calculation_time_max_hp = adjusted_amount / JUDGMENT_PURSUIT_MAX_HP_MULTIPLIER;
    let matches_wire_start_max_hp = wire_start_max_hp == Some(inferred_calculation_time_max_hp);
    let matches_event_order_max_hp = event_order_max_hp == Some(inferred_calculation_time_max_hp);
    if !matches_wire_start_max_hp && !matches_event_order_max_hp {
        return None;
    }
    let exact_recomputed_amount = inferred_calculation_time_max_hp
        .checked_mul(JUDGMENT_PURSUIT_MAX_HP_MULTIPLIER)?
        .checked_sub(JUDGMENT_PURSUIT_INTEGER_OFFSET)?;
    Some(JudgmentPursuitFormulaEvent {
        inferred_calculation_time_max_hp,
        max_hp_multiplier: JUDGMENT_PURSUIT_MAX_HP_MULTIPLIER,
        integer_offset: JUDGMENT_PURSUIT_INTEGER_OFFSET,
        exact_recomputed_amount,
        exact_amount_match: exact_recomputed_amount == amount,
        wire_start_max_hp,
        event_order_max_hp,
        matches_wire_start_max_hp,
        matches_event_order_max_hp,
        calculation_time_state_evidence: "the MaxHP inferred exactly from the retained damage component must match at least one packet-observed source MaxHP state surrounding the same wire message; this preserves cases where canonical notification order differs from server calculation order",
    })
}

fn infer_bash_pursuit_transient_snapshot(
    event: &DamageExample,
) -> Option<InferredTransientBashPursuitSnapshot> {
    let wire_state = event.source_state_at_wire_message_start.as_ref()?;
    let max_hp_scalar = wire_state
        .max_hp_total
        .as_ref()
        .or(wire_state.max_hp_final.as_ref())?;
    let max_hp = max_hp_scalar.integer_varint.filter(|value| *value > 0)?;
    let observed_wire_shield = wire_state
        .current_shield_total
        .filter(|value| *value >= 0)?;
    let observed_event_shield = event
        .source_state
        .current_shield_total
        .filter(|value| *value >= 0);
    let shield_cap = i64::try_from(
        i128::from(max_hp) * i128::from(BASH_PURSUIT_SHIELD_CAP_BASIS_POINTS) / 10_000,
    )
    .ok()?;
    let wire_formula_base = i64::try_from(
        i128::from(max_hp)
            + i128::from(observed_wire_shield.min(shield_cap))
                * i128::from(BASH_PURSUIT_SHIELD_DAMAGE_MULTIPLIER),
    )
    .ok()?;
    let wire_expected_amount = floor_mul_ratio(
        wire_formula_base,
        BASH_PURSUIT_OBSERVED_LATER_MULTIPLIER_NUMERATOR,
        BASH_PURSUIT_OBSERVED_LATER_MULTIPLIER_DENOMINATOR,
    )?;

    // A lower result can be target mitigation or another later multiplier. It is never safe to
    // rewrite the source state from that evidence. Equal wire-start results already have exact
    // packet inputs and need no inferred snapshot.
    if event.amount <= wire_expected_amount {
        return None;
    }

    let mut snapshot = infer_transient_bash_pursuit_snapshot(
        max_hp_scalar.attribute_id,
        max_hp,
        observed_wire_shield,
        observed_event_shield,
        event.amount,
        BASH_PURSUIT_OBSERVED_LATER_MULTIPLIER_NUMERATOR,
        BASH_PURSUIT_OBSERVED_LATER_MULTIPLIER_DENOMINATOR,
        shield_cap,
    )?;
    snapshot.observed_wire_start_shields = wire_state
        .current_shields
        .as_ref()
        .map(|shields| shields.shields.clone())
        .unwrap_or_default();
    snapshot.observed_event_order_shields = event
        .source_state
        .current_shields
        .as_ref()
        .map(|shields| shields.shields.clone());
    snapshot.candidate_transient_instance_bounds = transient_shield_instance_bounds(
        wire_state.current_shields.as_ref(),
        event.source_state.current_shields.as_ref(),
        snapshot.inferred_current_shield_total,
    );
    snapshot.transient_instance_allocation_unique = snapshot
        .candidate_transient_instance_bounds
        .iter()
        .all(|bounds| bounds.inferred_current_value_min == bounds.inferred_current_value_max);
    let ownership = resolve_transient_shield_provider_ownership(
        event.source_entity_uuid,
        &snapshot.candidate_transient_instance_bounds,
        event
            .source_shield_origins_at_wire_message_start
            .as_deref()
            .unwrap_or_default(),
        &event.source_shield_origins,
    );
    snapshot.shield_provider_decomposition_status = ownership.status;
    snapshot.candidate_provider_entity_uuids = ownership.candidate_provider_entity_uuids;
    snapshot.provider_ownership_complete = ownership.provider_ownership_complete;
    snapshot.resolved_provider_entity_uuid = ownership.resolved_provider_entity_uuid;
    snapshot.resolved_provider_relation = ownership.resolved_provider_relation;
    snapshot.external_rdps_transfer_required = ownership.external_rdps_transfer_required;
    snapshot.provider_attribution_allowed = ownership.resolved_provider_entity_uuid.is_some();
    Some(snapshot)
}

fn infer_transient_bash_pursuit_snapshot(
    max_hp_basis_attribute_id: i32,
    max_hp: i64,
    observed_wire_shield: i64,
    observed_event_shield: Option<i64>,
    amount: i64,
    later_multiplier_numerator: i64,
    later_multiplier_denominator: i64,
    shield_cap: i64,
) -> Option<InferredTransientBashPursuitSnapshot> {
    if max_hp <= 0
        || observed_wire_shield < 0
        || amount < 0
        || later_multiplier_numerator <= 0
        || later_multiplier_denominator <= 0
        || shield_cap < 0
    {
        return None;
    }

    let minimum_formula_base = ceil_div_positive(
        i128::from(amount) * i128::from(later_multiplier_denominator),
        i128::from(later_multiplier_numerator),
    )?;
    let maximum_formula_base = ceil_div_positive(
        i128::from(amount.saturating_add(1)) * i128::from(later_multiplier_denominator),
        i128::from(later_multiplier_numerator),
    )?
    .checked_sub(1)?;

    let mut candidates = Vec::new();
    for formula_base in minimum_formula_base..=maximum_formula_base {
        let shield_term = formula_base.checked_sub(i128::from(max_hp))?;
        if shield_term < 0 || shield_term % i128::from(BASH_PURSUIT_SHIELD_DAMAGE_MULTIPLIER) != 0 {
            continue;
        }
        let shield = shield_term / i128::from(BASH_PURSUIT_SHIELD_DAMAGE_MULTIPLIER);
        let Ok(shield) = i64::try_from(shield) else {
            continue;
        };
        if shield < 0 || shield > shield_cap {
            continue;
        }
        if let Some(event_shield) = observed_event_shield {
            let lower = observed_wire_shield.min(event_shield);
            let upper = observed_wire_shield.max(event_shield);
            if shield < lower || shield > upper {
                continue;
            }
        }
        let Ok(formula_base) = i64::try_from(formula_base) else {
            continue;
        };
        let Some(recomputed) = floor_mul_ratio(
            formula_base,
            later_multiplier_numerator,
            later_multiplier_denominator,
        ) else {
            continue;
        };
        if recomputed == amount {
            candidates.push((shield, formula_base, recomputed));
        }
    }

    let [(inferred_shield, inferred_formula_base, recomputed_amount)] = candidates.as_slice()
    else {
        return None;
    };
    if *inferred_shield == observed_wire_shield || observed_event_shield == Some(*inferred_shield) {
        return None;
    }

    Some(InferredTransientBashPursuitSnapshot {
        inference_status: "exact_integer_solution_between_packet_observed_states",
        inference_basis: "unique current-shield total that exactly reproduces the packet amount under the separately observed 3/2 later-multiplier cohort; this is a mathematical intermediate-state inference, not a directly serialized shield snapshot",
        max_hp_basis_attribute_id,
        max_hp_basis_value: max_hp,
        observed_wire_start_current_shield_total: observed_wire_shield,
        observed_event_order_current_shield_total: observed_event_shield,
        inferred_current_shield_total: *inferred_shield,
        wire_start_to_inferred_shield_delta: inferred_shield.saturating_sub(observed_wire_shield),
        inferred_to_event_order_shield_delta: observed_event_shield
            .map(|value| value.saturating_sub(*inferred_shield)),
        current_shield_input_cap: shield_cap,
        inferred_formula_base_before_later_multiplier: *inferred_formula_base,
        later_multiplier_numerator,
        later_multiplier_denominator,
        exact_recomputed_amount: *recomputed_amount,
        exact_amount_match: *recomputed_amount == amount,
        observed_wire_start_shields: Vec::new(),
        observed_event_order_shields: None,
        candidate_transient_instance_bounds: Vec::new(),
        transient_instance_allocation_constraint: "all retained per-instance current values must sum to the exact inferred aggregate while each matched instance stays within its two packet-observed endpoint values",
        transient_instance_allocation_unique: false,
        shield_provider_decomposition_status: "unresolved: packet proves the transient total but not its per-shield/provider decomposition",
        candidate_provider_entity_uuids: Vec::new(),
        provider_ownership_complete: false,
        resolved_provider_entity_uuid: None,
        resolved_provider_relation: None,
        external_rdps_transfer_required: None,
        provider_attribution_allowed: false,
    })
}

fn resolve_transient_shield_provider_ownership(
    damage_source_entity_uuid: i64,
    bounds: &[ShieldInstanceTransientBounds],
    wire_origins: &[ShieldOriginEvidence],
    event_origins: &[ShieldOriginEvidence],
) -> TransientShieldProviderOwnership {
    let mut all_candidate_providers = BTreeSet::new();
    let mut ownership_complete = !bounds.is_empty();

    for bound in bounds {
        let Some(shield_uuid) = bound.shield_uuid else {
            ownership_complete = false;
            continue;
        };
        let providers = wire_origins
            .iter()
            .chain(event_origins)
            .filter(|origin| origin.shield_uuid == Some(shield_uuid))
            .filter_map(|origin| origin.source_entity_uuid)
            .collect::<BTreeSet<_>>();
        if providers.len() != 1 {
            ownership_complete = false;
        }
        all_candidate_providers.extend(providers);
    }

    let candidate_provider_entity_uuids =
        all_candidate_providers.iter().copied().collect::<Vec<_>>();
    let resolved_provider_entity_uuid = (ownership_complete && all_candidate_providers.len() == 1)
        .then(|| all_candidate_providers.iter().next().copied())
        .flatten();
    let resolved_provider_relation = resolved_provider_entity_uuid.map(|provider| {
        if provider == damage_source_entity_uuid {
            ShieldProviderRelation::SelfProvided
        } else {
            ShieldProviderRelation::ExternallyProvided
        }
    });
    let external_rdps_transfer_required = resolved_provider_relation
        .map(|relation| relation == ShieldProviderRelation::ExternallyProvided);
    let status = match resolved_provider_relation {
        Some(ShieldProviderRelation::SelfProvided) => {
            "resolved owner: per-instance shield allocation remains ambiguous, but every feasible candidate is packet-owned by the damage source; external rDPS transfer is exactly zero"
        }
        Some(ShieldProviderRelation::ExternallyProvided) => {
            "resolved owner: per-instance shield allocation remains ambiguous, but every feasible candidate is packet-owned by one external provider; final damage transfer still requires an exact marginal counterfactual"
        }
        _ => {
            "unresolved owner: packet proves the transient aggregate shield total, but candidate instances do not resolve to one provider"
        }
    };

    TransientShieldProviderOwnership {
        candidate_provider_entity_uuids,
        provider_ownership_complete: ownership_complete,
        resolved_provider_entity_uuid,
        resolved_provider_relation,
        external_rdps_transfer_required,
        status,
    }
}

fn transient_shield_instance_bounds(
    wire_state: Option<&ShieldListSnapshot>,
    event_state: Option<&ShieldListSnapshot>,
    inferred_total: i64,
) -> Vec<ShieldInstanceTransientBounds> {
    let (Some(wire_state), Some(event_state)) = (wire_state, event_state) else {
        return Vec::new();
    };
    let event_by_uuid = event_state
        .shields
        .iter()
        .filter_map(|shield| shield.uuid.map(|uuid| (uuid, shield)))
        .collect::<BTreeMap<_, _>>();
    let matched = wire_state
        .shields
        .iter()
        .filter_map(|wire| {
            let uuid = wire.uuid?;
            let event = event_by_uuid.get(&uuid)?;
            let wire_current = wire.current_value?;
            let event_current = event.current_value?;
            Some((wire, *event, wire_current, event_current))
        })
        .collect::<Vec<_>>();
    if matched.is_empty() {
        return Vec::new();
    }
    let lower_sum = matched.iter().fold(0_i64, |sum, (_, _, wire, event)| {
        sum.saturating_add((*wire).min(*event))
    });
    let upper_sum = matched.iter().fold(0_i64, |sum, (_, _, wire, event)| {
        sum.saturating_add((*wire).max(*event))
    });
    if inferred_total < lower_sum || inferred_total > upper_sum {
        return Vec::new();
    }

    matched
        .into_iter()
        .filter_map(|(wire, event, wire_current, event_current)| {
            let lower = wire_current.min(event_current);
            let upper = wire_current.max(event_current);
            let other_lower = lower_sum.saturating_sub(lower);
            let other_upper = upper_sum.saturating_sub(upper);
            let inferred_min = lower.max(inferred_total.saturating_sub(other_upper));
            let inferred_max = upper.min(inferred_total.saturating_sub(other_lower));
            (inferred_min <= inferred_max).then_some(ShieldInstanceTransientBounds {
                shield_uuid: wire.uuid,
                shield_type: wire.shield_type.or(event.shield_type),
                observed_wire_start_current_value: wire_current,
                observed_event_order_current_value: event_current,
                inferred_current_value_min: inferred_min,
                inferred_current_value_max: inferred_max,
                inferred_delta_from_wire_min: inferred_min.saturating_sub(wire_current),
                inferred_delta_from_wire_max: inferred_max.saturating_sub(wire_current),
            })
        })
        .collect()
}

fn ceil_div_positive(numerator: i128, denominator: i128) -> Option<i128> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    numerator
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)
}

fn floor_mul_ratio(value: i64, numerator: i64, denominator: i64) -> Option<i64> {
    if value < 0 || numerator < 0 || denominator <= 0 {
        return None;
    }
    i64::try_from(
        i128::from(value)
            .checked_mul(i128::from(numerator))?
            .checked_div(i128::from(denominator))?,
    )
    .ok()
}

fn evaluate_bash_pursuit_formula(
    max_hp: i64,
    current_shield_total: i64,
    normal_value: Option<i64>,
    amount: i64,
    source_entity_uuid: i64,
    origins: &[ShieldOriginEvidence],
) -> Option<BashPursuitFormulaEvent> {
    if max_hp <= 0 || current_shield_total < 0 {
        return None;
    }
    let current_shield_input_cap = i64::try_from(
        i128::from(max_hp) * i128::from(BASH_PURSUIT_SHIELD_CAP_BASIS_POINTS) / 10_000,
    )
    .ok()?;
    let capped_current_shield_input = current_shield_total.min(current_shield_input_cap);
    let formula_base_before_later_multipliers = i64::try_from(
        i128::from(max_hp)
            + i128::from(capped_current_shield_input)
                * i128::from(BASH_PURSUIT_SHIELD_DAMAGE_MULTIPLIER),
    )
    .ok()?;
    if formula_base_before_later_multipliers <= 0 {
        return None;
    }

    type ExternalGroup = (i128, bool, BTreeSet<i64>, BTreeSet<i64>);
    let mut groups = BTreeMap::<ShieldProviderKey, ExternalGroup>::new();
    for shield in origins {
        let Some(provider_entity_uuid) = shield.source_entity_uuid else {
            continue;
        };
        if provider_entity_uuid == source_entity_uuid {
            continue;
        }
        let key = ShieldProviderKey {
            relation: ShieldProviderRelation::ExternallyProvided,
            shield_type: shield.shield_type,
            effect_id: shield.effect_id,
            origin_source_type_id: shield.origin_source_type_id,
            origin_source_config_id: shield.origin_source_config_id,
        };
        let group = groups
            .entry(key)
            .or_insert_with(|| (0_i128, true, BTreeSet::<i64>::new(), BTreeSet::<i64>::new()));
        match shield.current_value {
            Some(value) if value >= 0 => group.0 = group.0.saturating_add(i128::from(value)),
            Some(_) | None => group.1 = false,
        }
        group.2.insert(provider_entity_uuid);
        if let Some(shield_uuid) = shield.shield_uuid {
            group.3.insert(shield_uuid);
        }
    }

    let external_provider_counterfactuals = groups
        .into_iter()
        .map(|(key, (provider_value, exact, providers, shield_uuids))| {
            // Missing provider current_value does not make the retained HP-scaled hit unusable.
            // Known shields belonging to everyone else form a conservative packet lower bound.
            // When that lower bound alone reaches the cap, removing this provider cannot change
            // Bash Pursuit even if the provider's own magnitude remains absent on the wire.
            let known_non_provider_current_shield_lower_bound = origins
                .iter()
                .filter_map(|shield| {
                    let shield_uuid = shield.shield_uuid?;
                    let current_value = shield.current_value.filter(|value| *value >= 0)?;
                    (!shield_uuids.contains(&shield_uuid))
                        .then_some((shield_uuid, current_value))
                })
                .fold(BTreeMap::<i64, i64>::new(), |mut values, (uuid, value)| {
                    values
                        .entry(uuid)
                        .and_modify(|observed| *observed = (*observed).min(value))
                        .or_insert(value);
                    values
                })
                .into_values()
                .fold(0_i64, i64::saturating_add);
            let provider_current_shield = exact
                .then(|| i64::try_from(provider_value).ok())
                .flatten()
                .filter(|value| *value <= current_shield_total);
            let current_shield_without_provider =
                provider_current_shield.map(|value| current_shield_total - value);
            let non_provider_shields_alone_saturate_cap =
                known_non_provider_current_shield_lower_bound >= current_shield_input_cap;
            let capped_current_shield_without_provider = current_shield_without_provider
                .map(|value| value.min(current_shield_input_cap))
                .or_else(|| {
                    non_provider_shields_alone_saturate_cap.then_some(current_shield_input_cap)
                });
            let provider_removed_formula_base_delta = capped_current_shield_without_provider
                .and_then(|without| {
                    i64::try_from(
                        i128::from(capped_current_shield_input - without)
                            * i128::from(BASH_PURSUIT_SHIELD_DAMAGE_MULTIPLIER),
                    )
                    .ok()
                });
            ExternalShieldCounterfactualEvent {
                shield_type: key.shield_type,
                effect_id: key.effect_id,
                origin_source_type_id: key.origin_source_type_id,
                origin_source_config_id: key.origin_source_config_id,
                provider_entity_uuids: providers.into_iter().collect(),
                shield_uuids: shield_uuids.into_iter().collect(),
                known_non_provider_current_shield_lower_bound,
                provider_current_shield,
                current_shield_without_provider,
                capped_current_shield_without_provider,
                provider_removed_formula_base_delta,
                zero_due_to_existing_shield_cap: provider_removed_formula_base_delta
                    .map(|delta| {
                        delta == 0
                            && (current_shield_total >= current_shield_input_cap
                                || non_provider_shields_alone_saturate_cap)
                    }),
                zero_proof_basis: (provider_removed_formula_base_delta == Some(0)).then_some(
                    if provider_current_shield.is_some() {
                        "exact provider removal leaves the capped shield input unchanged"
                    } else {
                        "packet-known non-provider shield values alone saturate the shield-input cap; the retained provider shield magnitude is unnecessary for this exact zero marginal"
                    },
                ),
            }
        })
        .collect();

    Some(BashPursuitFormulaEvent {
        max_hp,
        current_shield_total,
        current_shield_input_cap,
        capped_current_shield_input,
        formula_base_before_later_multipliers,
        normal_to_formula_base_basis_points_floor: normal_value.and_then(|normal| {
            ratio_basis_points_floor(normal, formula_base_before_later_multipliers)
        }),
        amount_to_formula_base_basis_points_floor: ratio_basis_points_floor(
            amount,
            formula_base_before_later_multipliers,
        )?,
        inferred_transient_snapshot: None,
        external_provider_counterfactuals,
    })
}

fn ratio_basis_points_floor(numerator: i64, denominator: i64) -> Option<i64> {
    if denominator <= 0 {
        return None;
    }
    i64::try_from(i128::from(numerator) * 10_000_i128 / i128::from(denominator)).ok()
}

fn observe_damage(accumulator: &mut AbilityAccumulator, event: &DamageExample, limit: usize) {
    accumulator.events = accumulator.events.saturating_add(1);
    accumulator.amount_sum = accumulator
        .amount_sum
        .saturating_add(i128::from(event.amount));
    if event.normal_value.is_some() {
        accumulator.events_with_normal_value =
            accumulator.events_with_normal_value.saturating_add(1);
    }
    observe_state_ratios(
        &mut accumulator.event_state_ratios,
        event.amount,
        event.normal_value,
        Some(&event.source_state),
        Some(&event.target_state),
    );
    observe_state_ratios(
        &mut accumulator.wire_start_state_ratios,
        event.amount,
        event.normal_value,
        event.source_state_at_wire_message_start.as_ref(),
        event.target_state_at_wire_message_start.as_ref(),
    );
    observe_state_transitions(
        &mut accumulator.wire_to_event_state_transitions,
        event.amount,
        event.normal_value,
        event.source_state_at_wire_message_start.as_ref(),
        &event.source_state,
        event.target_state_at_wire_message_start.as_ref(),
        &event.target_state,
    );
    observe_source_hp_transition(
        &mut accumulator.source_hp_transition_semantics,
        event.source_state_at_wire_message_start.as_ref(),
        &event.source_state,
    );
    increment_status_signature(
        &mut accumulator.source_status_signatures,
        &event.active_source_statuses,
    );
    increment_status_signature(
        &mut accumulator.target_status_signatures,
        &event.active_target_statuses,
    );
    observe_shield_providers(
        &mut accumulator.shield_provider_observations,
        event.source_entity_uuid,
        event.amount,
        event
            .source_shield_origins_at_wire_message_start
            .as_deref()
            .unwrap_or(&event.source_shield_origins),
    );
    observe_bash_pursuit_formula(accumulator, event);
    observe_judgment_pursuit_formula(accumulator, event);
    if accumulator.examples.len() < limit {
        accumulator.examples.push(event.clone());
    }
}

fn observe_state_transitions(
    accumulators: &mut BTreeMap<&'static str, ScalarTransitionAccumulator>,
    amount: i64,
    normal_value: Option<i64>,
    source_at_wire_start: Option<&StateSnapshot>,
    source_at_event: &StateSnapshot,
    target_at_wire_start: Option<&StateSnapshot>,
    target_at_event: &StateSnapshot,
) {
    if let Some(start) = source_at_wire_start {
        observe_scalar_transition(
            accumulators.entry("source_current_hp").or_default(),
            amount,
            normal_value,
            state_integer(&start.current_hp),
            state_integer(&source_at_event.current_hp),
        );
        observe_scalar_transition(
            accumulators.entry("source_max_hp").or_default(),
            amount,
            normal_value,
            state_integer(&start.max_hp_final),
            state_integer(&source_at_event.max_hp_final),
        );
        observe_scalar_transition(
            accumulators.entry("source_missing_hp").or_default(),
            amount,
            normal_value,
            state_missing_hp(start),
            state_missing_hp(source_at_event),
        );
        observe_scalar_transition(
            accumulators.entry("source_current_shield").or_default(),
            amount,
            normal_value,
            start.current_shield_total,
            source_at_event.current_shield_total,
        );
    }
    if let Some(start) = target_at_wire_start {
        observe_scalar_transition(
            accumulators.entry("target_current_hp").or_default(),
            amount,
            normal_value,
            state_integer(&start.current_hp),
            state_integer(&target_at_event.current_hp),
        );
        observe_scalar_transition(
            accumulators.entry("target_max_hp").or_default(),
            amount,
            normal_value,
            state_integer(&start.max_hp_final),
            state_integer(&target_at_event.max_hp_final),
        );
        observe_scalar_transition(
            accumulators.entry("target_missing_hp").or_default(),
            amount,
            normal_value,
            state_missing_hp(start),
            state_missing_hp(target_at_event),
        );
        observe_scalar_transition(
            accumulators.entry("target_current_shield").or_default(),
            amount,
            normal_value,
            start.current_shield_total,
            target_at_event.current_shield_total,
        );
    }
}

fn observe_scalar_transition(
    accumulator: &mut ScalarTransitionAccumulator,
    amount: i64,
    normal_value: Option<i64>,
    start: Option<i64>,
    end: Option<i64>,
) {
    let (Some(start), Some(end)) = (start, end) else {
        return;
    };
    accumulator.events_with_both_states = accumulator.events_with_both_states.saturating_add(1);
    let change = end.saturating_sub(start);
    let count = accumulator.signed_changes.entry(change).or_default();
    *count = count.saturating_add(1);
    match change.cmp(&0) {
        std::cmp::Ordering::Less => {
            accumulator.decreased_events = accumulator.decreased_events.saturating_add(1);
        }
        std::cmp::Ordering::Equal => {
            accumulator.unchanged_events = accumulator.unchanged_events.saturating_add(1);
        }
        std::cmp::Ordering::Greater => {
            accumulator.increased_events = accumulator.increased_events.saturating_add(1);
        }
    }
    let Some(absolute_change) = change.checked_abs().filter(|value| *value > 0) else {
        return;
    };
    increment_ratio(
        &mut accumulator.amount_to_absolute_change_basis_points,
        amount,
        absolute_change,
    );
    if let Some(normal_value) = normal_value {
        increment_ratio(
            &mut accumulator.normal_to_absolute_change_basis_points,
            normal_value,
            absolute_change,
        );
    }
}

fn observe_source_hp_transition(
    accumulator: &mut SourceHpTransitionAccumulator,
    source_at_wire_start: Option<&StateSnapshot>,
    source_at_event: &StateSnapshot,
) {
    let Some(start) = source_at_wire_start else {
        return;
    };
    let (Some(start_current_hp), Some(end_current_hp), Some(start_max_hp), Some(end_max_hp)) = (
        state_integer(&start.current_hp),
        state_integer(&source_at_event.current_hp),
        state_integer(&start.max_hp_final),
        state_integer(&source_at_event.max_hp_final),
    ) else {
        return;
    };

    let current_hp_change = end_current_hp.saturating_sub(start_current_hp);
    let max_hp_change = end_max_hp.saturating_sub(start_max_hp);
    let start_missing_hp = start_max_hp.saturating_sub(start_current_hp);
    let end_missing_hp = end_max_hp.saturating_sub(end_current_hp);
    let missing_hp_change = end_missing_hp.saturating_sub(start_missing_hp);
    let semantic = match (current_hp_change, max_hp_change) {
        (0, 0) => SourceHpTransitionSemantic::Unchanged,
        (_, 0) => SourceHpTransitionSemantic::CurrentHpChangedMaxHpStable,
        (0, _) => SourceHpTransitionSemantic::CurrentHpStableMaxHpChanged,
        (current, maximum) if current == maximum => {
            SourceHpTransitionSemantic::CurrentAndMaxHpChangedSameDeltaPreservingMissingHp
        }
        _ => SourceHpTransitionSemantic::CurrentAndMaxHpChangedDifferentDelta,
    };

    accumulator.events_with_current_and_max_hp_at_both_timings = accumulator
        .events_with_current_and_max_hp_at_both_timings
        .saturating_add(1);
    let semantic_count = accumulator.semantic_counts.entry(semantic).or_default();
    *semantic_count = semantic_count.saturating_add(1);
    let transition_count = accumulator
        .signed_change_triplets
        .entry((current_hp_change, max_hp_change, missing_hp_change))
        .or_default();
    *transition_count = transition_count.saturating_add(1);
}

fn observe_bash_pursuit_formula(accumulator: &mut AbilityAccumulator, event: &DamageExample) {
    if event.ability_id != BASH_PURSUIT_ABILITY_ID {
        return;
    }
    let Some(formula) = &event.bash_pursuit_formula else {
        let formula = accumulator
            .bash_pursuit_formula
            .get_or_insert_with(Default::default);
        formula.events_without_exact_packet_inputs =
            formula.events_without_exact_packet_inputs.saturating_add(1);
        return;
    };
    let accumulator = accumulator
        .bash_pursuit_formula
        .get_or_insert_with(Default::default);
    accumulator.events_with_exact_packet_inputs = accumulator
        .events_with_exact_packet_inputs
        .saturating_add(1);
    if formula.current_shield_total > formula.current_shield_input_cap {
        accumulator.events_using_shield_cap = accumulator.events_using_shield_cap.saturating_add(1);
    }
    accumulator.formula_base_sum = accumulator
        .formula_base_sum
        .saturating_add(i128::from(formula.formula_base_before_later_multipliers));
    let best_exact_snapshot_formula_base = formula
        .inferred_transient_snapshot
        .as_ref()
        .map(|snapshot| snapshot.inferred_formula_base_before_later_multiplier)
        .unwrap_or(formula.formula_base_before_later_multipliers);
    accumulator.best_exact_snapshot_formula_base_sum = accumulator
        .best_exact_snapshot_formula_base_sum
        .saturating_add(i128::from(best_exact_snapshot_formula_base));
    if let Some(snapshot) = &formula.inferred_transient_snapshot {
        accumulator.events_with_exact_inferred_transient_snapshot = accumulator
            .events_with_exact_inferred_transient_snapshot
            .saturating_add(1);
        if snapshot.resolved_provider_entity_uuid.is_some() {
            accumulator.events_with_resolved_transient_provider_ownership = accumulator
                .events_with_resolved_transient_provider_ownership
                .saturating_add(1);
        } else {
            accumulator.events_with_unresolved_transient_provider_ownership = accumulator
                .events_with_unresolved_transient_provider_ownership
                .saturating_add(1);
        }
        if accumulator.inferred_transient_snapshot_examples.len() < DEFAULT_EXAMPLE_LIMIT {
            accumulator.inferred_transient_snapshot_examples.push(
                InferredTransientBashPursuitExample {
                    rlog: event.rlog.clone(),
                    session_id: event.session_id.clone(),
                    run_ordinal: event.run_ordinal,
                    sequence: event.sequence,
                    observed_micros: event.observed_micros,
                    wire_capture_sequence: event.wire_capture_sequence,
                    source_entity_uuid: event.source_entity_uuid,
                    target_entity_uuid: event.target_entity_uuid,
                    amount: event.amount,
                    snapshot: snapshot.clone(),
                },
            );
        }
    }
    increment_ratio(
        &mut accumulator.amount_to_formula_base_basis_points,
        event.amount,
        formula.formula_base_before_later_multipliers,
    );
    increment_ratio(
        &mut accumulator.amount_to_best_exact_snapshot_formula_base_basis_points,
        event.amount,
        best_exact_snapshot_formula_base,
    );
    if let Some(normal_value) = event.normal_value {
        increment_ratio(
            &mut accumulator.normal_to_formula_base_basis_points,
            normal_value,
            formula.formula_base_before_later_multipliers,
        );
    }
    for counterfactual in &formula.external_provider_counterfactuals {
        let key = ShieldProviderKey {
            relation: ShieldProviderRelation::ExternallyProvided,
            shield_type: counterfactual.shield_type,
            effect_id: counterfactual.effect_id,
            origin_source_type_id: counterfactual.origin_source_type_id,
            origin_source_config_id: counterfactual.origin_source_config_id,
        };
        let report = accumulator
            .external_provider_counterfactuals
            .entry(key)
            .or_default();
        report.damage_events = report.damage_events.saturating_add(1);
        report
            .provider_entity_uuids
            .extend(counterfactual.provider_entity_uuids.iter().copied());
        report
            .recipient_entity_uuids
            .insert(event.source_entity_uuid);
        report
            .shield_uuids
            .extend(counterfactual.shield_uuids.iter().copied());
        if let Some(current) = counterfactual.provider_current_shield {
            report.events_with_exact_current_value =
                report.events_with_exact_current_value.saturating_add(1);
            report.provider_current_shield_sum = report
                .provider_current_shield_sum
                .saturating_add(i128::from(current));
        } else {
            report.events_without_exact_current_value =
                report.events_without_exact_current_value.saturating_add(1);
        }
        if let Some(delta) = counterfactual.provider_removed_formula_base_delta {
            report.provider_removed_formula_base_delta_sum = report
                .provider_removed_formula_base_delta_sum
                .saturating_add(i128::from(delta));
            if delta > 0 {
                report.events_with_positive_formula_base_delta = report
                    .events_with_positive_formula_base_delta
                    .saturating_add(1);
            } else if counterfactual.zero_due_to_existing_shield_cap == Some(true) {
                report.events_with_zero_formula_base_delta_due_to_cap = report
                    .events_with_zero_formula_base_delta_due_to_cap
                    .saturating_add(1);
                if counterfactual.provider_current_shield.is_none() {
                    report.events_with_exact_zero_without_provider_current_value = report
                        .events_with_exact_zero_without_provider_current_value
                        .saturating_add(1);
                }
            }
        }
    }
}

fn observe_judgment_pursuit_formula(accumulator: &mut AbilityAccumulator, event: &DamageExample) {
    if event.ability_id != JUDGMENT_PURSUIT_ABILITY_ID {
        return;
    }
    let formula = accumulator
        .judgment_pursuit_formula
        .get_or_insert_with(Default::default);
    formula.events = formula.events.saturating_add(1);
    let Some(proof) = &event.judgment_pursuit_formula else {
        formula.events_without_exact_integer_solution = formula
            .events_without_exact_integer_solution
            .saturating_add(1);
        return;
    };
    formula.events_with_exact_integer_solution =
        formula.events_with_exact_integer_solution.saturating_add(1);
    if proof.matches_wire_start_max_hp {
        formula.events_matching_wire_start_max_hp =
            formula.events_matching_wire_start_max_hp.saturating_add(1);
    }
    if proof.matches_event_order_max_hp {
        formula.events_matching_event_order_max_hp =
            formula.events_matching_event_order_max_hp.saturating_add(1);
    }
    if proof.matches_wire_start_max_hp && proof.matches_event_order_max_hp {
        formula.events_matching_both_packet_states =
            formula.events_matching_both_packet_states.saturating_add(1);
    }
    formula
        .inferred_calculation_time_max_hp_values
        .insert(proof.inferred_calculation_time_max_hp);
}

fn observe_shield_providers(
    accumulators: &mut BTreeMap<ShieldProviderKey, ShieldProviderAccumulator>,
    damage_source_entity_uuid: i64,
    damage_amount: i64,
    shields: &[ShieldOriginEvidence],
) {
    let mut event_groups =
        BTreeMap::<ShieldProviderKey, (u64, i128, u64, BTreeSet<i64>, BTreeSet<i64>)>::new();

    for shield in shields {
        let relation = match shield.source_entity_uuid {
            Some(provider) if provider == damage_source_entity_uuid => {
                ShieldProviderRelation::SelfProvided
            }
            Some(_) => ShieldProviderRelation::ExternallyProvided,
            None => ShieldProviderRelation::UnresolvedProvider,
        };
        let key = ShieldProviderKey {
            relation,
            shield_type: shield.shield_type,
            effect_id: shield.effect_id,
            origin_source_type_id: shield.origin_source_type_id,
            origin_source_config_id: shield.origin_source_config_id,
        };
        let group = event_groups.entry(key).or_default();
        group.0 = group.0.saturating_add(1);
        if let Some(value) = shield.current_value {
            group.1 = group.1.saturating_add(i128::from(value));
            group.2 = group.2.saturating_add(1);
        }
        if let Some(uuid) = shield.shield_uuid {
            group.3.insert(uuid);
        }
        if let Some(provider) = shield.source_entity_uuid {
            group.4.insert(provider);
        }
    }

    for (key, (instance_count, current_value, value_count, shield_uuids, providers)) in event_groups
    {
        let accumulator = accumulators.entry(key).or_default();
        accumulator.damage_events = accumulator.damage_events.saturating_add(1);
        accumulator.shield_instance_observations = accumulator
            .shield_instance_observations
            .saturating_add(instance_count);
        accumulator.observed_damage_amount_sum = accumulator
            .observed_damage_amount_sum
            .saturating_add(i128::from(damage_amount));
        accumulator.current_value_observations = accumulator
            .current_value_observations
            .saturating_add(value_count);
        accumulator.current_value_sum = accumulator.current_value_sum.saturating_add(current_value);
        if value_count > 0
            && let Ok(current_value) = i64::try_from(current_value)
        {
            accumulator.current_value_min = Some(
                accumulator
                    .current_value_min
                    .map_or(current_value, |value| value.min(current_value)),
            );
            accumulator.current_value_max = Some(
                accumulator
                    .current_value_max
                    .map_or(current_value, |value| value.max(current_value)),
            );
        }
        accumulator.shield_uuids.extend(shield_uuids);
        accumulator.provider_entity_uuids.extend(providers);
        accumulator
            .damage_source_entity_uuids
            .insert(damage_source_entity_uuid);
    }
}

fn observe_state_ratios(
    accumulator: &mut StateRatioAccumulator,
    amount: i64,
    normal_value: Option<i64>,
    source: Option<&StateSnapshot>,
    target: Option<&StateSnapshot>,
) {
    if let Some(source) = source {
        if let Some(current_hp) = state_integer(&source.current_hp).filter(|value| *value > 0) {
            accumulator.events_with_source_current_hp =
                accumulator.events_with_source_current_hp.saturating_add(1);
            increment_ratio(
                &mut accumulator.amount_to_source_current_hp_basis_points,
                amount,
                current_hp,
            );
        }
        if let Some(max_hp) = state_integer(&source.max_hp_final).filter(|value| *value > 0) {
            accumulator.events_with_source_max_hp =
                accumulator.events_with_source_max_hp.saturating_add(1);
            increment_ratio(
                &mut accumulator.amount_to_source_max_hp_basis_points,
                amount,
                max_hp,
            );
            increment_near_integer_multiple(
                &mut accumulator.amount_to_source_max_hp_near_integer_multiples,
                amount,
                max_hp,
            );
            if let Some(normal_value) = normal_value {
                increment_ratio(
                    &mut accumulator.normal_to_source_max_hp_basis_points,
                    normal_value,
                    max_hp,
                );
            }
        }
        if let Some(current_shield) = source.current_shield_total.filter(|value| *value > 0) {
            accumulator.events_with_source_current_shield = accumulator
                .events_with_source_current_shield
                .saturating_add(1);
            increment_ratio(
                &mut accumulator.amount_to_source_current_shield_basis_points,
                amount,
                current_shield,
            );
            if let Some(normal_value) = normal_value {
                increment_ratio(
                    &mut accumulator.normal_to_source_current_shield_basis_points,
                    normal_value,
                    current_shield,
                );
            }
            if let Some(max_hp) = state_integer(&source.max_hp_final).filter(|value| *value > 0) {
                let combined_basis =
                    i128::from(max_hp).saturating_add(i128::from(current_shield).saturating_mul(3));
                if let Ok(combined_basis) = i64::try_from(combined_basis)
                    && combined_basis > 0
                {
                    accumulator.events_with_source_max_hp_plus_three_current_shield = accumulator
                        .events_with_source_max_hp_plus_three_current_shield
                        .saturating_add(1);
                    increment_ratio(
                        &mut accumulator
                            .amount_to_source_max_hp_plus_three_current_shield_basis_points,
                        amount,
                        combined_basis,
                    );
                    if let Some(normal_value) = normal_value {
                        increment_ratio(
                            &mut accumulator
                                .normal_to_source_max_hp_plus_three_current_shield_basis_points,
                            normal_value,
                            combined_basis,
                        );
                    }
                }
            }
        }
        if let Some(missing_hp) = state_missing_hp(source).filter(|value| *value > 0) {
            accumulator.events_with_source_missing_hp =
                accumulator.events_with_source_missing_hp.saturating_add(1);
            increment_ratio(
                &mut accumulator.amount_to_source_missing_hp_basis_points,
                amount,
                missing_hp,
            );
            if let Some(normal_value) = normal_value {
                increment_ratio(
                    &mut accumulator.normal_to_source_missing_hp_basis_points,
                    normal_value,
                    missing_hp,
                );
            }
        }
        if let Some(defense) =
            state_integer(&source.physical_defense_final).filter(|value| *value > 0)
        {
            accumulator.events_with_source_physical_defense = accumulator
                .events_with_source_physical_defense
                .saturating_add(1);
            increment_ratio(
                &mut accumulator.amount_to_source_physical_defense_basis_points,
                amount,
                defense,
            );
            increment_near_integer_multiple(
                &mut accumulator.amount_to_source_physical_defense_near_integer_multiples,
                amount,
                defense,
            );
            if let Some(normal_value) = normal_value {
                increment_ratio(
                    &mut accumulator.normal_to_source_physical_defense_basis_points,
                    normal_value,
                    defense,
                );
            }
        }
    }
    if let Some(target) = target {
        if let Some(current_hp) = state_integer(&target.current_hp).filter(|value| *value > 0) {
            accumulator.events_with_target_current_hp =
                accumulator.events_with_target_current_hp.saturating_add(1);
            increment_ratio(
                &mut accumulator.amount_to_target_current_hp_basis_points,
                amount,
                current_hp,
            );
        }
        if let Some(max_hp) = state_integer(&target.max_hp_final).filter(|value| *value > 0) {
            accumulator.events_with_target_max_hp =
                accumulator.events_with_target_max_hp.saturating_add(1);
            increment_ratio(
                &mut accumulator.amount_to_target_max_hp_basis_points,
                amount,
                max_hp,
            );
            if let Some(normal_value) = normal_value {
                increment_ratio(
                    &mut accumulator.normal_to_target_max_hp_basis_points,
                    normal_value,
                    max_hp,
                );
            }
        }
        if let Some(missing_hp) = state_missing_hp(target).filter(|value| *value > 0) {
            accumulator.events_with_target_missing_hp =
                accumulator.events_with_target_missing_hp.saturating_add(1);
            increment_ratio(
                &mut accumulator.amount_to_target_missing_hp_basis_points,
                amount,
                missing_hp,
            );
            if let Some(normal_value) = normal_value {
                increment_ratio(
                    &mut accumulator.normal_to_target_missing_hp_basis_points,
                    normal_value,
                    missing_hp,
                );
            }
        }
    }
}

fn increment_near_integer_multiple(
    counts: &mut BTreeMap<(i64, i64), u64>,
    amount: i64,
    state_value: i64,
) {
    for multiplier in 1_i64..=10_i64 {
        let residual = i128::from(amount) - i128::from(state_value) * i128::from(multiplier);
        let Ok(residual) = i64::try_from(residual) else {
            continue;
        };
        if residual.abs() <= 16 {
            let count = counts.entry((multiplier, residual)).or_default();
            *count = count.saturating_add(1);
        }
    }
}

fn increment_status_signature(
    counts: &mut BTreeMap<Vec<i64>, u64>,
    statuses: &[ActiveStatusEvidence],
) {
    let signature = statuses
        .iter()
        .map(|status| status.effect_id)
        .collect::<Vec<_>>();
    let count = counts.entry(signature).or_default();
    *count = count.saturating_add(1);
}

fn increment_ratio(counts: &mut BTreeMap<i64, RatioAccumulator>, numerator: i64, denominator: i64) {
    let ratio = (i128::from(numerator) * 10_000_i128) / i128::from(denominator);
    let Ok(ratio) = i64::try_from(ratio) else {
        return;
    };
    let accumulator = counts.entry(ratio).or_default();
    accumulator.count = accumulator.count.saturating_add(1);
    accumulator.numerators.insert(numerator);
    accumulator.denominators.insert(denominator);
}

fn hp_shield_dependency_inventory(
    abilities: &BTreeMap<i64, AbilityAccumulator>,
) -> Result<Vec<HpShieldDependencyCandidateReport>, String> {
    let mut reports = abilities
        .iter()
        .map(|(ability_id, accumulator)| -> Result<_, String> {
            let presentation = combat_action_presentation(*ability_id)?;
            let mut state_observations = Vec::with_capacity(18);
            append_state_observations(
                &mut state_observations,
                StateObservationTiming::EventOrder,
                &accumulator.event_state_ratios,
            );
            append_state_observations(
                &mut state_observations,
                StateObservationTiming::WireMessageStart,
                &accumulator.wire_start_state_ratios,
            );

            let mut fixed_point_candidates = Vec::new();
            append_fixed_point_candidates(
                &mut fixed_point_candidates,
                StateObservationTiming::EventOrder,
                &accumulator.event_state_ratios,
            );
            append_fixed_point_candidates(
                &mut fixed_point_candidates,
                StateObservationTiming::WireMessageStart,
                &accumulator.wire_start_state_ratios,
            );
            classify_fixed_point_candidate_timings(&mut fixed_point_candidates);
            fixed_point_candidates.sort_by(|left, right| {
                fixed_point_strength_rank(right.strength)
                    .cmp(&fixed_point_strength_rank(left.strength))
                    .then_with(|| {
                        right
                            .eligible_for_formula_investigation
                            .cmp(&left.eligible_for_formula_investigation)
                    })
                    .then_with(|| right.count.cmp(&left.count))
                    .then_with(|| left.locus.cmp(right.locus))
                    .then_with(|| left.numerator.cmp(right.numerator))
                    .then_with(|| left.basis_points_floor.cmp(&right.basis_points_floor))
            });

            let exact_formula_proof = if *ability_id == BASH_PURSUIT_ABILITY_ID
                && accumulator
                    .bash_pursuit_formula
                    .as_ref()
                    .is_some_and(|formula| formula.events_with_exact_packet_inputs > 0)
            {
                Some(
                    "MaxHP + 3 * min(current shield total, floor(1.5 * MaxHP)); aggregate formula base is packet-validated, while per-provider transient shield allocation remains unresolved",
                )
            } else if *ability_id == JUDGMENT_PURSUIT_ABILITY_ID
                && accumulator
                    .judgment_pursuit_formula
                    .as_ref()
                    .is_some_and(judgment_pursuit_formula_is_exact)
            {
                Some(
                    "3 * calculation-time source MaxHP - 1; calculation-time MaxHP is inferred exactly from the retained damage component and corroborated by a surrounding packet state because canonical notification order can differ from server calculation order",
                )
            } else {
                None
            };
            let has_cross_state_candidate = fixed_point_candidates.iter().any(|candidate| {
                candidate.strength == FixedPointCandidateStrength::CrossState
                    && candidate.eligible_for_formula_investigation
            });
            let has_repeated_single_state_candidate = fixed_point_candidates.iter().any(|candidate| {
                candidate.strength == FixedPointCandidateStrength::RepeatedSingleState
                    && candidate.eligible_for_formula_investigation
            });
            let has_only_post_hit_target_state_candidates = !fixed_point_candidates.is_empty()
                && fixed_point_candidates
                    .iter()
                    .all(|candidate| candidate.post_hit_consequence_risk);
            let has_state = state_observations
                .iter()
                .any(|observation| observation.events > 0);
            let evidence_status = if exact_formula_proof.is_some() {
                StateDependencyEvidenceStatus::ExactFormulaProvedAtAggregateLevel
            } else if has_cross_state_candidate {
                StateDependencyEvidenceStatus::CrossStateFixedPointCandidate
            } else if has_repeated_single_state_candidate {
                StateDependencyEvidenceStatus::RepeatedSingleStateCandidate
            } else if has_only_post_hit_target_state_candidates {
                StateDependencyEvidenceStatus::EventOrderTargetStateConsequenceObserved
            } else if has_state {
                StateDependencyEvidenceStatus::PacketStateObservedFormulaUnresolved
            } else {
                StateDependencyEvidenceStatus::PacketStateUnavailableInCurrentCaptures
            };
            let interpretation = match evidence_status {
                StateDependencyEvidenceStatus::ExactFormulaProvedAtAggregateLevel => {
                    "aggregate state-scaled formula equality is proved; retain every state and lifecycle input, and keep provider credit disabled until the packet evidence uniquely determines provider contribution"
                }
                StateDependencyEvidenceStatus::CrossStateFixedPointCandidate => {
                    "the same fixed-point ratio repeats across multiple distinct packet state values; this is a high-priority formula candidate, not formula proof"
                }
                StateDependencyEvidenceStatus::RepeatedSingleStateCandidate => {
                    "a fixed-point ratio repeats while the observed state value does not vary enough to prove dependence; preserve and seek a controlled state change"
                }
                StateDependencyEvidenceStatus::EventOrderTargetStateConsequenceObserved => {
                    "a repeated target-current-HP or target-missing-HP relationship exists only after event-order state application; retain it as timing evidence, but do not treat a possible post-hit consequence as a formula input"
                }
                StateDependencyEvidenceStatus::PacketStateObservedFormulaUnresolved => {
                    "HP or shield state is present at the damage event, but current captures do not isolate a stable dependency; preserve the event and state without assuming independence"
                }
                StateDependencyEvidenceStatus::PacketStateUnavailableInCurrentCaptures => {
                    "the ability remains retained, but the current captures do not carry a usable HP or shield snapshot for formula testing"
                }
            };

            Ok(HpShieldDependencyCandidateReport {
                ability_id: *ability_id,
                presentation_kind: presentation.map(|value| value.kind.clone()),
                presentation_resolution: presentation.map(|value| value.resolution.clone()),
                recount_group_id: presentation.and_then(|value| value.recount_group_id),
                damage_events: accumulator.events,
                evidence_status,
                retained_in_canonical_timeline: true,
                runtime_rdps_attribution_enabled: false,
                exact_formula_proof,
                state_observations,
                wire_to_event_state_transitions: state_transition_reports(
                    &accumulator.wire_to_event_state_transitions,
                ),
                source_hp_transition_semantics: source_hp_transition_report(
                    &accumulator.source_hp_transition_semantics,
                ),
                fixed_point_candidates,
                interpretation,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    reports.sort_by(|left, right| {
        state_dependency_status_rank(right.evidence_status)
            .cmp(&state_dependency_status_rank(left.evidence_status))
            .then_with(|| right.damage_events.cmp(&left.damage_events))
            .then_with(|| left.ability_id.cmp(&right.ability_id))
    });
    Ok(reports)
}

fn judgment_pursuit_formula_is_exact(accumulator: &JudgmentPursuitFormulaAccumulator) -> bool {
    accumulator.events >= 2
        && accumulator.events_with_exact_integer_solution == accumulator.events
        && accumulator.events_without_exact_integer_solution == 0
        && accumulator.inferred_calculation_time_max_hp_values.len() >= 2
}

fn hp_shield_dependency_inventory_summary(
    abilities: &[HpShieldDependencyCandidateReport],
) -> HpShieldDependencyInventorySummary {
    let mut summary = HpShieldDependencyInventorySummary {
        abilities: abilities.len(),
        retained_abilities: abilities
            .iter()
            .filter(|ability| ability.retained_in_canonical_timeline)
            .count(),
        runtime_rdps_attribution_enabled: abilities
            .iter()
            .filter(|ability| ability.runtime_rdps_attribution_enabled)
            .count(),
        ..HpShieldDependencyInventorySummary::default()
    };
    for ability in abilities {
        match ability.evidence_status {
            StateDependencyEvidenceStatus::ExactFormulaProvedAtAggregateLevel => {
                summary.exact_formula_proved_at_aggregate_level += 1;
            }
            StateDependencyEvidenceStatus::CrossStateFixedPointCandidate => {
                summary.cross_state_fixed_point_candidates += 1;
            }
            StateDependencyEvidenceStatus::RepeatedSingleStateCandidate => {
                summary.repeated_single_state_candidates += 1;
            }
            StateDependencyEvidenceStatus::EventOrderTargetStateConsequenceObserved => {
                summary.event_order_target_state_consequences_observed += 1;
            }
            StateDependencyEvidenceStatus::PacketStateObservedFormulaUnresolved => {
                summary.packet_state_observed_formula_unresolved += 1;
            }
            StateDependencyEvidenceStatus::PacketStateUnavailableInCurrentCaptures => {
                summary.packet_state_unavailable_in_current_captures += 1;
            }
        }
    }
    summary
}

fn state_dependency_status_rank(status: StateDependencyEvidenceStatus) -> u8 {
    match status {
        StateDependencyEvidenceStatus::ExactFormulaProvedAtAggregateLevel => 5,
        StateDependencyEvidenceStatus::CrossStateFixedPointCandidate => 4,
        StateDependencyEvidenceStatus::RepeatedSingleStateCandidate => 3,
        StateDependencyEvidenceStatus::EventOrderTargetStateConsequenceObserved => 2,
        StateDependencyEvidenceStatus::PacketStateObservedFormulaUnresolved => 1,
        StateDependencyEvidenceStatus::PacketStateUnavailableInCurrentCaptures => 0,
    }
}

fn fixed_point_strength_rank(strength: FixedPointCandidateStrength) -> u8 {
    match strength {
        FixedPointCandidateStrength::CrossState => 1,
        FixedPointCandidateStrength::RepeatedSingleState => 0,
    }
}

fn append_state_observations(
    output: &mut Vec<StateObservationCount>,
    timing: StateObservationTiming,
    ratios: &StateRatioAccumulator,
) {
    output.extend([
        StateObservationCount {
            timing,
            locus: "source_current_hp",
            events: ratios.events_with_source_current_hp,
        },
        StateObservationCount {
            timing,
            locus: "source_max_hp",
            events: ratios.events_with_source_max_hp,
        },
        StateObservationCount {
            timing,
            locus: "source_current_shield",
            events: ratios.events_with_source_current_shield,
        },
        StateObservationCount {
            timing,
            locus: "source_max_hp_plus_three_current_shield",
            events: ratios.events_with_source_max_hp_plus_three_current_shield,
        },
        StateObservationCount {
            timing,
            locus: "source_missing_hp",
            events: ratios.events_with_source_missing_hp,
        },
        StateObservationCount {
            timing,
            locus: "target_current_hp",
            events: ratios.events_with_target_current_hp,
        },
        StateObservationCount {
            timing,
            locus: "target_max_hp",
            events: ratios.events_with_target_max_hp,
        },
        StateObservationCount {
            timing,
            locus: "target_missing_hp",
            events: ratios.events_with_target_missing_hp,
        },
    ]);
}

fn append_fixed_point_candidates(
    output: &mut Vec<FixedPointCandidate>,
    timing: StateObservationTiming,
    ratios: &StateRatioAccumulator,
) {
    let surfaces = [
        (
            "source_current_hp",
            "amount",
            &ratios.amount_to_source_current_hp_basis_points,
        ),
        (
            "source_max_hp",
            "normal_value",
            &ratios.normal_to_source_max_hp_basis_points,
        ),
        (
            "source_max_hp",
            "amount",
            &ratios.amount_to_source_max_hp_basis_points,
        ),
        (
            "source_current_shield",
            "normal_value",
            &ratios.normal_to_source_current_shield_basis_points,
        ),
        (
            "source_current_shield",
            "amount",
            &ratios.amount_to_source_current_shield_basis_points,
        ),
        (
            "source_max_hp_plus_three_current_shield",
            "normal_value",
            &ratios.normal_to_source_max_hp_plus_three_current_shield_basis_points,
        ),
        (
            "source_max_hp_plus_three_current_shield",
            "amount",
            &ratios.amount_to_source_max_hp_plus_three_current_shield_basis_points,
        ),
        (
            "source_missing_hp",
            "normal_value",
            &ratios.normal_to_source_missing_hp_basis_points,
        ),
        (
            "source_missing_hp",
            "amount",
            &ratios.amount_to_source_missing_hp_basis_points,
        ),
        (
            "target_current_hp",
            "amount",
            &ratios.amount_to_target_current_hp_basis_points,
        ),
        (
            "target_max_hp",
            "normal_value",
            &ratios.normal_to_target_max_hp_basis_points,
        ),
        (
            "target_max_hp",
            "amount",
            &ratios.amount_to_target_max_hp_basis_points,
        ),
        (
            "target_missing_hp",
            "normal_value",
            &ratios.normal_to_target_missing_hp_basis_points,
        ),
        (
            "target_missing_hp",
            "amount",
            &ratios.amount_to_target_missing_hp_basis_points,
        ),
    ];
    for (locus, numerator, counts) in surfaces {
        let locus_observation_events = state_observation_events(ratios, locus);
        if locus_observation_events == 0 {
            continue;
        }
        for (basis_points_floor, accumulator) in counts {
            if *basis_points_floor <= 0 {
                continue;
            }
            let coverage_basis_points = accumulator
                .count
                .saturating_mul(10_000)
                .checked_div(locus_observation_events)
                .unwrap_or(0);
            let strength = if accumulator.count >= 2
                && accumulator.numerators.len() >= 2
                && accumulator.denominators.len() >= 2
                && coverage_basis_points >= CROSS_STATE_CANDIDATE_MINIMUM_COVERAGE_BASIS_POINTS
            {
                Some(FixedPointCandidateStrength::CrossState)
            } else if accumulator.count >= 3
                && coverage_basis_points >= SINGLE_STATE_CANDIDATE_MINIMUM_COVERAGE_BASIS_POINTS
            {
                Some(FixedPointCandidateStrength::RepeatedSingleState)
            } else {
                None
            };
            let Some(strength) = strength else {
                continue;
            };
            output.push(FixedPointCandidate {
                timing,
                locus,
                numerator,
                basis_points_floor: *basis_points_floor,
                count: accumulator.count,
                locus_observation_events,
                coverage_basis_points,
                distinct_numerators: accumulator.numerators.len(),
                distinct_denominators: accumulator.denominators.len(),
                strength,
                retained_for_evidence: true,
                matching_other_timing_candidate: false,
                eligible_for_formula_investigation: true,
                post_hit_consequence_risk: false,
                timing_assessment: match timing {
                    StateObservationTiming::EventOrder => {
                        FixedPointTimingAssessment::EventOrderOnly
                    }
                    StateObservationTiming::WireMessageStart => {
                        FixedPointTimingAssessment::WireMessageStartOnly
                    }
                },
            });
        }
    }
}

fn classify_fixed_point_candidate_timings(candidates: &mut [FixedPointCandidate]) {
    let signatures = candidates
        .iter()
        .map(|candidate| {
            (
                candidate.timing,
                candidate.locus,
                candidate.numerator,
                candidate.basis_points_floor,
            )
        })
        .collect::<BTreeSet<_>>();

    for candidate in candidates {
        let other_timing = match candidate.timing {
            StateObservationTiming::EventOrder => StateObservationTiming::WireMessageStart,
            StateObservationTiming::WireMessageStart => StateObservationTiming::EventOrder,
        };
        candidate.matching_other_timing_candidate = signatures.contains(&(
            other_timing,
            candidate.locus,
            candidate.numerator,
            candidate.basis_points_floor,
        ));
        candidate.post_hit_consequence_risk = candidate.timing
            == StateObservationTiming::EventOrder
            && !candidate.matching_other_timing_candidate
            && matches!(candidate.locus, "target_current_hp" | "target_missing_hp");
        candidate.eligible_for_formula_investigation = !candidate.post_hit_consequence_risk;
        candidate.timing_assessment = if candidate.matching_other_timing_candidate {
            FixedPointTimingAssessment::PresentAtBothPacketTimings
        } else if candidate.post_hit_consequence_risk {
            FixedPointTimingAssessment::EventOrderTargetStatePostHitConsequenceRisk
        } else {
            match candidate.timing {
                StateObservationTiming::EventOrder => FixedPointTimingAssessment::EventOrderOnly,
                StateObservationTiming::WireMessageStart => {
                    FixedPointTimingAssessment::WireMessageStartOnly
                }
            }
        };
    }
}

fn state_observation_events(ratios: &StateRatioAccumulator, locus: &str) -> u64 {
    match locus {
        "source_current_hp" => ratios.events_with_source_current_hp,
        "source_max_hp" => ratios.events_with_source_max_hp,
        "source_current_shield" => ratios.events_with_source_current_shield,
        "source_max_hp_plus_three_current_shield" => {
            ratios.events_with_source_max_hp_plus_three_current_shield
        }
        "source_missing_hp" => ratios.events_with_source_missing_hp,
        "target_current_hp" => ratios.events_with_target_current_hp,
        "target_max_hp" => ratios.events_with_target_max_hp,
        "target_missing_hp" => ratios.events_with_target_missing_hp,
        _ => 0,
    }
}

fn ability_reports(abilities: BTreeMap<i64, AbilityAccumulator>) -> Vec<AbilityReport> {
    let mut reports = abilities
        .into_iter()
        .map(|(ability_id, accumulator)| {
            let wire_to_event_state_transitions =
                state_transition_reports(&accumulator.wire_to_event_state_transitions);
            let source_hp_transition_semantics =
                source_hp_transition_report(&accumulator.source_hp_transition_semantics);
            AbilityReport {
                ability_id,
                events: accumulator.events,
                amount_sum: accumulator.amount_sum.to_string(),
                events_with_normal_value: accumulator.events_with_normal_value,
                event_state_ratios: state_ratio_report(accumulator.event_state_ratios),
                wire_start_state_ratios: state_ratio_report(accumulator.wire_start_state_ratios),
                wire_to_event_state_transitions,
                source_hp_transition_semantics,
                source_status_signatures: status_signature_report(
                    accumulator.source_status_signatures,
                ),
                target_status_signatures: status_signature_report(
                    accumulator.target_status_signatures,
                ),
                shield_provider_observations: shield_provider_reports(
                    accumulator.shield_provider_observations,
                ),
                bash_pursuit_formula: accumulator
                    .bash_pursuit_formula
                    .map(bash_pursuit_formula_report),
                judgment_pursuit_formula: accumulator
                    .judgment_pursuit_formula
                    .map(judgment_pursuit_formula_report),
                examples: accumulator.examples,
            }
        })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| {
        right
            .events
            .cmp(&left.events)
            .then_with(|| left.ability_id.cmp(&right.ability_id))
    });
    reports
}

fn effect_ability_inventory(
    effects: &BTreeMap<i64, EffectAccumulator>,
) -> Vec<EffectAbilityInventoryEffect> {
    effects
        .iter()
        .map(|(effect_id, effect)| EffectAbilityInventoryEffect {
            effect_id: *effect_id,
            status_events: effect.status_events,
            source_damage_events_while_active: effect.source_damage_events_while_active,
            direct_source_damage_events_while_active: effect
                .direct_source_damage_events_while_active,
            abilities: effect
                .abilities
                .iter()
                .map(|(ability_id, ability)| EffectAbilityInventoryAbility {
                    ability_id: *ability_id,
                    events: ability.events,
                    amount_sum: ability.amount_sum.to_string(),
                })
                .collect(),
        })
        .collect()
}

fn bash_pursuit_formula_report(
    accumulator: BashPursuitFormulaAccumulator,
) -> BashPursuitFormulaReport {
    let mut external_provider_counterfactuals = accumulator
        .external_provider_counterfactuals
        .into_iter()
        .map(|(key, value)| ExternalShieldCounterfactualReport {
            provider_relation: key.relation,
            shield_type: key.shield_type,
            effect_id: key.effect_id,
            origin_source_type_id: key.origin_source_type_id,
            origin_source_config_id: key.origin_source_config_id,
            damage_events: value.damage_events,
            events_with_exact_current_value: value.events_with_exact_current_value,
            events_without_exact_current_value: value.events_without_exact_current_value,
            events_with_exact_zero_without_provider_current_value: value
                .events_with_exact_zero_without_provider_current_value,
            provider_current_shield_sum: value.provider_current_shield_sum.to_string(),
            provider_removed_formula_base_delta_sum: value
                .provider_removed_formula_base_delta_sum
                .to_string(),
            events_with_positive_formula_base_delta: value
                .events_with_positive_formula_base_delta,
            events_with_zero_formula_base_delta_due_to_cap: value
                .events_with_zero_formula_base_delta_due_to_cap,
            exact_zero_contribution_when_formula_base_delta_is_zero: true,
            positive_final_damage_attribution_policy: "retain the exact provider-removed formula-base delta, but do not convert a positive base delta to final attributed damage until that hit's later target/status multiplier and integer-rounding stage are proven",
            provider_entity_uuids: value.provider_entity_uuids.into_iter().collect(),
            recipient_entity_uuids: value.recipient_entity_uuids.into_iter().collect(),
            shield_uuid_examples: value.shield_uuids.into_iter().take(16).collect(),
        })
        .collect::<Vec<_>>();
    external_provider_counterfactuals.sort_by(|left, right| {
        right
            .damage_events
            .cmp(&left.damage_events)
            .then_with(|| left.effect_id.cmp(&right.effect_id))
            .then_with(|| left.shield_type.cmp(&right.shield_type))
    });
    BashPursuitFormulaReport {
        formula_status: "packet-validated current-build formula base; later target/status multipliers remain separately observed",
        formula: "MaxHP + 3 * min(current shield total, floor(1.5 * MaxHP))",
        packet_input_timing: "source attributes and structured shield list at the beginning of the damage event's exact wire message",
        max_hp_coefficient_basis_points: 10_000,
        current_shield_coefficient_basis_points: 30_000,
        current_shield_input_cap_basis_points_of_max_hp: BASH_PURSUIT_SHIELD_CAP_BASIS_POINTS,
        events_with_exact_packet_inputs: accumulator.events_with_exact_packet_inputs,
        events_without_exact_packet_inputs: accumulator.events_without_exact_packet_inputs,
        events_using_shield_cap: accumulator.events_using_shield_cap,
        formula_base_sum: accumulator.formula_base_sum.to_string(),
        best_exact_snapshot_formula_base_sum: accumulator
            .best_exact_snapshot_formula_base_sum
            .to_string(),
        normal_to_formula_base_basis_points: ratio_counts(
            accumulator.normal_to_formula_base_basis_points,
        ),
        amount_to_formula_base_basis_points: ratio_counts(
            accumulator.amount_to_formula_base_basis_points,
        ),
        amount_to_best_exact_snapshot_formula_base_basis_points: ratio_counts(
            accumulator.amount_to_best_exact_snapshot_formula_base_basis_points,
        ),
        events_with_exact_inferred_transient_snapshot: accumulator
            .events_with_exact_inferred_transient_snapshot,
        events_with_resolved_transient_provider_ownership: accumulator
            .events_with_resolved_transient_provider_ownership,
        events_with_unresolved_transient_provider_ownership: accumulator
            .events_with_unresolved_transient_provider_ownership,
        inferred_transient_snapshot_examples: accumulator.inferred_transient_snapshot_examples,
        external_provider_counterfactuals,
    }
}

fn judgment_pursuit_formula_report(
    accumulator: JudgmentPursuitFormulaAccumulator,
) -> JudgmentPursuitFormulaReport {
    let exact = judgment_pursuit_formula_is_exact(&accumulator);
    JudgmentPursuitFormulaReport {
        formula_status: if exact {
            "packet-validated across multiple calculation-time MaxHP values and target cohorts"
        } else {
            "retained packet evidence; exact equality is not yet complete across all observed events"
        },
        formula: "3 * calculation-time source MaxHP - 1",
        calculation_time_state_policy: "infer MaxHP exactly as (amount + 1) / 3, require integer equality, then require that value to match the wire-start or event-order source MaxHP surrounding the same wire message; never assume canonical notification order is server calculation order",
        events: accumulator.events,
        events_with_exact_integer_solution: accumulator.events_with_exact_integer_solution,
        events_without_exact_integer_solution: accumulator.events_without_exact_integer_solution,
        events_matching_wire_start_max_hp: accumulator.events_matching_wire_start_max_hp,
        events_matching_event_order_max_hp: accumulator.events_matching_event_order_max_hp,
        events_matching_both_packet_states: accumulator.events_matching_both_packet_states,
        distinct_inferred_calculation_time_max_hp_values: accumulator
            .inferred_calculation_time_max_hp_values
            .len(),
        inferred_calculation_time_max_hp_values: accumulator
            .inferred_calculation_time_max_hp_values
            .into_iter()
            .collect(),
        runtime_rdps_attribution_enabled: false,
        provider_attribution_boundary: "the component formula is exact, but external rDPS credit remains disabled until the calculation-time MaxHP aggregate is decomposed into exact self and external provider contributions under the game's fixed-point stages",
    }
}

fn shield_provider_reports(
    observations: BTreeMap<ShieldProviderKey, ShieldProviderAccumulator>,
) -> Vec<ShieldProviderReport> {
    let mut reports = observations
        .into_iter()
        .map(|(key, accumulator)| ShieldProviderReport {
            provider_relation: key.relation,
            shield_type: key.shield_type,
            effect_id: key.effect_id,
            origin_source_type_id: key.origin_source_type_id,
            origin_source_config_id: key.origin_source_config_id,
            damage_events: accumulator.damage_events,
            shield_instance_observations: accumulator.shield_instance_observations,
            observed_damage_amount_sum: accumulator.observed_damage_amount_sum.to_string(),
            current_value_observations: accumulator.current_value_observations,
            current_value_sum: accumulator.current_value_sum.to_string(),
            current_value_per_event_min: accumulator.current_value_min,
            current_value_per_event_max: accumulator.current_value_max,
            distinct_shield_uuids: accumulator.shield_uuids.len(),
            shield_uuid_examples: accumulator.shield_uuids.into_iter().take(16).collect(),
            provider_entity_uuids: accumulator.provider_entity_uuids.into_iter().collect(),
            damage_source_entity_uuids: accumulator
                .damage_source_entity_uuids
                .into_iter()
                .collect(),
        })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| {
        right
            .damage_events
            .cmp(&left.damage_events)
            .then_with(|| left.effect_id.cmp(&right.effect_id))
            .then_with(|| left.shield_type.cmp(&right.shield_type))
    });
    reports
}

fn state_ratio_report(accumulator: StateRatioAccumulator) -> StateRatioReport {
    StateRatioReport {
        events_with_source_current_hp: accumulator.events_with_source_current_hp,
        events_with_source_max_hp: accumulator.events_with_source_max_hp,
        events_with_source_current_shield: accumulator.events_with_source_current_shield,
        events_with_source_max_hp_plus_three_current_shield: accumulator
            .events_with_source_max_hp_plus_three_current_shield,
        events_with_source_missing_hp: accumulator.events_with_source_missing_hp,
        events_with_source_physical_defense: accumulator.events_with_source_physical_defense,
        events_with_target_current_hp: accumulator.events_with_target_current_hp,
        events_with_target_max_hp: accumulator.events_with_target_max_hp,
        events_with_target_missing_hp: accumulator.events_with_target_missing_hp,
        amount_to_source_current_hp_basis_points: ratio_counts(
            accumulator.amount_to_source_current_hp_basis_points,
        ),
        normal_to_source_max_hp_basis_points: ratio_counts(
            accumulator.normal_to_source_max_hp_basis_points,
        ),
        amount_to_source_max_hp_basis_points: ratio_counts(
            accumulator.amount_to_source_max_hp_basis_points,
        ),
        normal_to_source_current_shield_basis_points: ratio_counts(
            accumulator.normal_to_source_current_shield_basis_points,
        ),
        amount_to_source_current_shield_basis_points: ratio_counts(
            accumulator.amount_to_source_current_shield_basis_points,
        ),
        normal_to_source_max_hp_plus_three_current_shield_basis_points: ratio_counts(
            accumulator.normal_to_source_max_hp_plus_three_current_shield_basis_points,
        ),
        amount_to_source_max_hp_plus_three_current_shield_basis_points: ratio_counts(
            accumulator.amount_to_source_max_hp_plus_three_current_shield_basis_points,
        ),
        normal_to_source_missing_hp_basis_points: ratio_counts(
            accumulator.normal_to_source_missing_hp_basis_points,
        ),
        amount_to_source_missing_hp_basis_points: ratio_counts(
            accumulator.amount_to_source_missing_hp_basis_points,
        ),
        normal_to_source_physical_defense_basis_points: ratio_counts(
            accumulator.normal_to_source_physical_defense_basis_points,
        ),
        amount_to_source_physical_defense_basis_points: ratio_counts(
            accumulator.amount_to_source_physical_defense_basis_points,
        ),
        amount_to_target_current_hp_basis_points: ratio_counts(
            accumulator.amount_to_target_current_hp_basis_points,
        ),
        normal_to_target_max_hp_basis_points: ratio_counts(
            accumulator.normal_to_target_max_hp_basis_points,
        ),
        amount_to_target_max_hp_basis_points: ratio_counts(
            accumulator.amount_to_target_max_hp_basis_points,
        ),
        normal_to_target_missing_hp_basis_points: ratio_counts(
            accumulator.normal_to_target_missing_hp_basis_points,
        ),
        amount_to_target_missing_hp_basis_points: ratio_counts(
            accumulator.amount_to_target_missing_hp_basis_points,
        ),
        amount_to_source_max_hp_near_integer_multiples: near_integer_multiple_counts(
            accumulator.amount_to_source_max_hp_near_integer_multiples,
        ),
        amount_to_source_physical_defense_near_integer_multiples: near_integer_multiple_counts(
            accumulator.amount_to_source_physical_defense_near_integer_multiples,
        ),
    }
}

fn state_transition_reports(
    accumulators: &BTreeMap<&'static str, ScalarTransitionAccumulator>,
) -> Vec<StateTransitionReport> {
    accumulators
        .iter()
        .filter(|(_, accumulator)| accumulator.events_with_both_states > 0)
        .map(|(locus, accumulator)| {
            let mut signed_change_examples = accumulator
                .signed_changes
                .iter()
                .map(|(signed_change, count)| StateTransitionCount {
                    signed_change: *signed_change,
                    count: *count,
                })
                .collect::<Vec<_>>();
            signed_change_examples.sort_by(|left, right| {
                right
                    .count
                    .cmp(&left.count)
                    .then_with(|| left.signed_change.cmp(&right.signed_change))
            });
            signed_change_examples.truncate(32);
            let post_event_consequence_risk = matches!(
                *locus,
                "target_current_hp" | "target_missing_hp" | "target_current_shield"
            );
            StateTransitionReport {
                locus,
                events_with_both_states: accumulator.events_with_both_states,
                unchanged_events: accumulator.unchanged_events,
                increased_events: accumulator.increased_events,
                decreased_events: accumulator.decreased_events,
                signed_change_examples,
                amount_to_absolute_change_basis_points: ratio_counts(
                    accumulator
                        .amount_to_absolute_change_basis_points
                        .clone(),
                ),
                normal_to_absolute_change_basis_points: ratio_counts(
                    accumulator
                        .normal_to_absolute_change_basis_points
                        .clone(),
                ),
                post_event_consequence_risk,
                interpretation: if post_event_consequence_risk {
                    "wire-start to event-order target change is retained, but may be the damage or shield-loss consequence itself and is never treated as a formula input without independent packet evidence"
                } else {
                    "wire-start to event-order source or Max-HP change is retained as an intra-message transition; it can identify an HP cost, gain, or scaling stage but does not by itself establish server calculation order"
                },
            }
        })
        .collect()
}

fn source_hp_transition_report(
    accumulator: &SourceHpTransitionAccumulator,
) -> SourceHpTransitionReport {
    let semantic_counts = accumulator
        .semantic_counts
        .iter()
        .map(|(semantic, count)| SourceHpTransitionSemanticCount {
            semantic: *semantic,
            count: *count,
        })
        .collect();
    let mut signed_change_examples = accumulator
        .signed_change_triplets
        .iter()
        .map(
            |((current_hp_change, max_hp_change, missing_hp_change), count)| {
                SourceHpTransitionCount {
                    current_hp_change: *current_hp_change,
                    max_hp_change: *max_hp_change,
                    missing_hp_change: *missing_hp_change,
                    count: *count,
                }
            },
        )
        .collect::<Vec<_>>();
    signed_change_examples.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.current_hp_change.cmp(&right.current_hp_change))
            .then_with(|| left.max_hp_change.cmp(&right.max_hp_change))
            .then_with(|| left.missing_hp_change.cmp(&right.missing_hp_change))
    });
    signed_change_examples.truncate(32);

    SourceHpTransitionReport {
        events_with_current_and_max_hp_at_both_timings: accumulator
            .events_with_current_and_max_hp_at_both_timings,
        semantic_counts,
        signed_change_examples,
        hp_dependent_events_retained: true,
        interpretation: "all source CurrentHP and MaxHP transitions remain first-class packet evidence: HP-only changes may be costs, healing, or missing-HP inputs; MaxHP-only changes may change the scaling surface; equal CurrentHP and MaxHP deltas preserve missing HP and identify a MaxHP-surface transition rather than an HP cost; none of these labels removes or suppresses an event",
    }
}

fn near_integer_multiple_counts(
    counts: BTreeMap<(i64, i64), u64>,
) -> Vec<NearIntegerMultipleCount> {
    let mut values = counts
        .into_iter()
        .map(|((multiplier, residual), count)| NearIntegerMultipleCount {
            multiplier,
            residual,
            count,
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.multiplier.cmp(&right.multiplier))
            .then_with(|| left.residual.cmp(&right.residual))
    });
    values
}

fn status_signature_report(counts: BTreeMap<Vec<i64>, u64>) -> StatusSignatureReport {
    let unique_signatures = counts.len();
    let observations = counts.values().copied().sum();
    let mut values = counts
        .into_iter()
        .map(|(effect_ids, count)| StatusSignatureCount { effect_ids, count })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.effect_ids.cmp(&right.effect_ids))
    });
    values.truncate(STATUS_SIGNATURE_REPORT_LIMIT);
    StatusSignatureReport {
        unique_signatures,
        observations,
        top_signatures: values,
    }
}

fn ratio_counts(counts: BTreeMap<i64, RatioAccumulator>) -> Vec<RatioCount> {
    let mut values = counts
        .into_iter()
        .map(|(basis_points_floor, accumulator)| RatioCount {
            basis_points_floor,
            count: accumulator.count,
            distinct_numerators: accumulator.numerators.len(),
            distinct_denominators: accumulator.denominators.len(),
            numerator_examples: accumulator.numerators.into_iter().take(8).collect(),
            denominator_examples: accumulator.denominators.into_iter().take(8).collect(),
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.basis_points_floor.cmp(&right.basis_points_floor))
    });
    values.truncate(64);
    values
}

fn preserve_attribute(attribute: &rlogs_events::EntityAttribute) -> StateScalar {
    let decoded = attribute.decoded.clone().or_else(|| {
        decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value)
    });
    let integer_varint = match decoded {
        Some(EntityAttributeValue::Integer(value)) => Some(value),
        Some(EntityAttributeValue::Text(_)) | Some(EntityAttributeValue::Position { .. }) => None,
        None => decode_varint(&attribute.raw_value).and_then(|value| i64::try_from(value).ok()),
    };
    let float32_little_endian = (matches!(
        attribute.attribute_id,
        CURRENT_ENERGY_ATTRIBUTE_ID
            | MAX_ENERGY_ATTRIBUTE_ID
            | MAX_ENERGY_TOTAL_ATTRIBUTE_ID
            | MAX_ENERGY_ADD_ATTRIBUTE_ID
            | MAX_ENERGY_EXTRA_ADD_ATTRIBUTE_ID
            | MAX_ENERGY_PERCENT_ATTRIBUTE_ID
            | MAX_ENERGY_EXTRA_PERCENT_ATTRIBUTE_ID
    ) && attribute.raw_value.len() == 4)
        .then(|| {
            f32::from_le_bytes([
                attribute.raw_value[0],
                attribute.raw_value[1],
                attribute.raw_value[2],
                attribute.raw_value[3],
            ])
        });
    let decoded_shield_list = (attribute.attribute_id == SHIELD_LIST_ATTRIBUTE_ID)
        .then(|| decode_shield_list(&attribute.raw_value).ok())
        .flatten();
    StateScalar {
        attribute_id: attribute.attribute_id,
        raw_length: attribute.raw_value.len(),
        raw_sha256: format!("sha256:{:x}", Sha256::digest(&attribute.raw_value)),
        raw_hex: (attribute.raw_value.len() <= MAX_INLINE_RAW_BYTES)
            .then(|| hex_bytes(&attribute.raw_value)),
        integer_varint,
        float32_little_endian,
        decoded_shield_list,
    }
}

fn state_snapshot(
    values: Option<&BTreeMap<i32, StateScalar>>,
    retain_all_packet_attributes: bool,
) -> StateSnapshot {
    let value = |id| values.and_then(|entries| entries.get(&id)).cloned();
    let family = |range: std::ops::RangeInclusive<i32>| {
        values
            .map(|entries| {
                range
                    .filter_map(|id| entries.get(&id).cloned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let current_shields = values
        .and_then(|entries| entries.get(&SHIELD_LIST_ATTRIBUTE_ID))
        .and_then(|value| value.decoded_shield_list.clone());
    let current_shield_total = current_shields
        .as_ref()
        .and_then(ShieldListSnapshot::current_value_total);
    StateSnapshot {
        current_hp: value(CURRENT_HP_ATTRIBUTE_ID),
        max_hp_final: value(MAX_HP_ATTRIBUTE_ID),
        max_hp_total: value(MAX_HP_TOTAL_ATTRIBUTE_ID),
        max_hp_add: value(MAX_HP_ADD_ATTRIBUTE_ID),
        max_hp_extra_add: value(MAX_HP_EXTRA_ADD_ATTRIBUTE_ID),
        max_hp_percent: value(MAX_HP_PERCENT_ATTRIBUTE_ID),
        max_hp_extra_percent: value(MAX_HP_EXTRA_PERCENT_ATTRIBUTE_ID),
        current_shields,
        current_shield_total,
        physical_defense_final: value(PHYSICAL_DEFENSE_ATTRIBUTE_ID),
        physical_defense_total: value(PHYSICAL_DEFENSE_TOTAL_ATTRIBUTE_ID),
        physical_defense_add: value(PHYSICAL_DEFENSE_ADD_ATTRIBUTE_ID),
        physical_defense_extra_add: value(PHYSICAL_DEFENSE_EXTRA_ADD_ATTRIBUTE_ID),
        physical_defense_percent: value(PHYSICAL_DEFENSE_PERCENT_ATTRIBUTE_ID),
        physical_defense_extra_percent: value(PHYSICAL_DEFENSE_EXTRA_PERCENT_ATTRIBUTE_ID),
        current_energy: value(CURRENT_ENERGY_ATTRIBUTE_ID),
        max_energy_final: value(MAX_ENERGY_ATTRIBUTE_ID),
        max_energy_total: value(MAX_ENERGY_TOTAL_ATTRIBUTE_ID),
        max_energy_add: value(MAX_ENERGY_ADD_ATTRIBUTE_ID),
        max_energy_extra_add: value(MAX_ENERGY_EXTRA_ADD_ATTRIBUTE_ID),
        max_energy_percent: value(MAX_ENERGY_PERCENT_ATTRIBUTE_ID),
        max_energy_extra_percent: value(MAX_ENERGY_EXTRA_PERCENT_ATTRIBUTE_ID),
        shield_add_percent_family: family(SHIELD_ADD_PERCENT_ATTRIBUTE_IDS),
        shield_gain_percent_family: family(SHIELD_GAIN_PERCENT_ATTRIBUTE_IDS),
        shield_damage_percent_family: family(SHIELD_DAMAGE_PERCENT_ATTRIBUTE_IDS),
        shield_damage_reduction_percent_family: family(
            SHIELD_DAMAGE_REDUCTION_PERCENT_ATTRIBUTE_IDS,
        ),
        outgoing_damage_formula_attributes: values
            .map(|entries| {
                entries
                    .values()
                    .filter(|value| {
                        OUTGOING_DAMAGE_ATTRIBUTE_RANGES
                            .iter()
                            .any(|range| range.contains(&value.attribute_id))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        all_packet_attributes: if retain_all_packet_attributes {
            values
                .map(|entries| entries.values().cloned().collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        },
    }
}

fn status_provider_attribute_snapshots(
    run_ordinal: u32,
    source_statuses: Option<&[ActiveStatusEvidence]>,
    target_statuses: Option<&[ActiveStatusEvidence]>,
    attributes_at_wire_message_start: &HashMap<(u32, i64), BTreeMap<i32, StateScalar>>,
    current_attributes: &HashMap<(u32, i64), BTreeMap<i32, StateScalar>>,
) -> Vec<StatusProviderAttributeSnapshot> {
    source_statuses
        .unwrap_or_default()
        .iter()
        .chain(target_statuses.unwrap_or_default())
        .filter_map(|status| status.source_entity_uuid)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|provider_entity_uuid| {
            let key = (run_ordinal, provider_entity_uuid);
            let values = attributes_at_wire_message_start
                .get(&key)
                .or_else(|| current_attributes.get(&key));
            StatusProviderAttributeSnapshot {
                provider_entity_uuid,
                state_at_wire_message_start: values
                    .map(|values| state_snapshot(Some(values), true)),
            }
        })
        .collect()
}

impl FormulaCohortAccumulator {
    fn push(
        &mut self,
        example: &DamageExample,
        provider_snapshots: &[StatusProviderAttributeSnapshot],
        scene_id: Option<i32>,
        source_actor: Option<&ActorSnapshot>,
        direct_source_actor: Option<&ActorSnapshot>,
        target_actor: Option<&ActorSnapshot>,
        direct_source_state_at_wire_message_start: Option<&StateSnapshot>,
        source_position_at_wire_message_start: Option<CompactFormulaPositionObservation>,
        direct_source_position_at_wire_message_start: Option<CompactFormulaPositionObservation>,
        target_position_at_wire_message_start: Option<CompactFormulaPositionObservation>,
    ) {
        let source_attribute_state_id =
            self.intern_attribute_state(example.source_state_at_wire_message_start.as_ref());
        let direct_source_attribute_state_id = direct_source_state_at_wire_message_start
            .map(|state| self.intern_attribute_state(Some(state)));
        let target_attribute_state_id =
            self.intern_attribute_state(example.target_state_at_wire_message_start.as_ref());
        let source_status_state_id =
            self.intern_status_state(example.source_statuses_at_wire_message_start.as_deref());
        let target_status_state_id =
            self.intern_status_state(example.target_statuses_at_wire_message_start.as_deref());
        let status_provider_attribute_states = provider_snapshots
            .iter()
            .map(|provider| CompactFormulaProviderAttributeState {
                provider_entity_uuid: provider.provider_entity_uuid,
                attribute_state_id: provider
                    .state_at_wire_message_start
                    .as_ref()
                    .map(|state| self.intern_attribute_state(Some(state))),
            })
            .collect();
        self.samples.push(FormulaCohortSample {
            rlog: example.rlog.clone(),
            session_id: example.session_id.clone(),
            run_ordinal: example.run_ordinal,
            sequence: example.sequence,
            observed_micros: example.observed_micros,
            wire_capture_sequence: example.wire_capture_sequence,
            scene_id,
            source_entity_uuid: example.source_entity_uuid,
            direct_source_entity_uuid: example.direct_source_entity_uuid,
            target_entity_uuid: example.target_entity_uuid,
            source_actor_identity: compact_formula_actor_identity(source_actor),
            direct_source_actor_identity: compact_formula_actor_identity(direct_source_actor),
            target_actor_identity: compact_formula_actor_identity(target_actor),
            source_position_at_wire_message_start,
            direct_source_position_at_wire_message_start,
            target_position_at_wire_message_start,
            ability_id: example.ability_id,
            passive_uuid: example.passive_uuid,
            hit_event_id: example.hit_event_id,
            amount: example.amount,
            actual_amount: example.actual_amount,
            normal_value: example.normal_value,
            lucky_value: example.lucky_value,
            hp_loss: example.hp_loss,
            shield_loss: example.shield_loss,
            damage_source: example.damage_source,
            damage_type: example.damage_type,
            critical: example.critical,
            lucky: example.lucky,
            packet: example.packet.clone(),
            source_attribute_state_id,
            direct_source_attribute_state_id,
            target_attribute_state_id,
            source_status_state_id,
            target_status_state_id,
            status_provider_attribute_states,
        });
    }

    fn intern_attribute_state(&mut self, state: Option<&StateSnapshot>) -> u32 {
        let compact = state
            .into_iter()
            .flat_map(|state| state.all_packet_attributes.iter())
            .filter_map(|attribute| {
                (attribute.attribute_id == 10
                    || (10_000..=39_999).contains(&attribute.attribute_id))
                .then_some(CompactFormulaAttribute {
                    attribute_id: attribute.attribute_id,
                    value: attribute.integer_varint?,
                })
            })
            .collect::<Vec<_>>();
        if let Some(id) = self.attribute_state_ids.get(&compact) {
            return *id;
        }
        let id = u32::try_from(self.attribute_states.len()).unwrap_or(u32::MAX);
        self.attribute_states.push(compact.clone());
        self.attribute_state_ids.insert(compact, id);
        id
    }

    fn intern_status_state(&mut self, state: Option<&[ActiveStatusEvidence]>) -> u32 {
        let mut compact = state
            .unwrap_or_default()
            .iter()
            .map(|status| CompactFormulaStatus {
                effect_id: status.effect_id,
                source_entity_uuid: status.source_entity_uuid,
                stacks: status.stacks,
                level: status.level,
                origin_source_type_id: status.origin_source_type_id,
                origin_source_config_id: status.origin_source_config_id,
            })
            .collect::<Vec<_>>();
        compact.sort_unstable();
        compact.dedup();
        if let Some(id) = self.status_state_ids.get(&compact) {
            return *id;
        }
        let id = u32::try_from(self.status_states.len()).unwrap_or(u32::MAX);
        self.status_states.push(compact.clone());
        self.status_state_ids.insert(compact, id);
        id
    }
}

fn position_at_wire_message_start(
    positions_at_wire_message_start: &HashMap<
        (u32, i64),
        Option<CompactFormulaPositionObservation>,
    >,
    positions: &HashMap<(u32, i64), CompactFormulaPositionObservation>,
    key: (u32, i64),
) -> Option<CompactFormulaPositionObservation> {
    positions_at_wire_message_start
        .get(&key)
        .cloned()
        .unwrap_or_else(|| positions.get(&key).cloned())
}

fn compact_formula_actor_identity(
    actor: Option<&ActorSnapshot>,
) -> Option<CompactFormulaActorIdentity> {
    let actor = actor?;
    Some(CompactFormulaActorIdentity {
        entity_type_id: actor.entity_type_id?,
        monster_id: actor.monster_id,
        character_id: actor.character_id.clone(),
        class_id: actor.class_id,
        specialization_id: actor.specialization_id,
        level: actor.level,
    })
}

fn formula_proof_bundle(
    cohort: &FormulaCohortAccumulator,
    inputs: Vec<String>,
    game_build: String,
    args: &Arguments,
    gap_window_filter: Option<&FormulaGapWindowFilter>,
) -> Result<FormulaProofBundle, Box<dyn std::error::Error>> {
    let season_vectors = season_vector_counts(cohort);
    let season_target_input_proof = season_target_input_proof(cohort);
    let input_determinism = formula_input_determinism_report(cohort)?;
    let message_scope_determinism = formula_message_scope_report(cohort)?;
    let formula_surface = formula_surface_inventory_report(cohort);
    let coefficient_pair_proof = formula_coefficient_pair_report(cohort)?;
    let post_coefficient_stage = formula_post_coefficient_stage_report(cohort);
    let candidates = [
        (
            "physical_attack",
            FormulaAttributeLocus::Source,
            PHYSICAL_ATTACK_ATTRIBUTE_ID,
        ),
        (
            "physical_defense",
            FormulaAttributeLocus::Target,
            PHYSICAL_DEFENSE_ATTRIBUTE_ID,
        ),
        (
            "source_season_strength",
            FormulaAttributeLocus::Source,
            SEASON_STRENGTH_ATTRIBUTE_ID,
        ),
        (
            "source_target_season_input",
            FormulaAttributeLocus::Source,
            SEASON_TARGET_INPUT_ATTRIBUTE_ID,
        ),
        (
            "target_season_strength",
            FormulaAttributeLocus::Target,
            SEASON_STRENGTH_ATTRIBUTE_ID,
        ),
        (
            "target_target_season_input",
            FormulaAttributeLocus::Target,
            SEASON_TARGET_INPUT_ATTRIBUTE_ID,
        ),
        (
            "critical_chance",
            FormulaAttributeLocus::Source,
            CRITICAL_CHANCE_ATTRIBUTE_ID,
        ),
        (
            "lucky_chance",
            FormulaAttributeLocus::Source,
            LUCKY_CHANCE_ATTRIBUTE_ID,
        ),
        (
            "external_damage",
            FormulaAttributeLocus::Source,
            EXTERNAL_DAMAGE_ATTRIBUTE_ID,
        ),
        ("haste", FormulaAttributeLocus::Source, HASTE_ATTRIBUTE_ID),
        (
            "mastery",
            FormulaAttributeLocus::Source,
            MASTERY_ATTRIBUTE_ID,
        ),
        (
            "versatility",
            FormulaAttributeLocus::Source,
            VERSATILITY_ATTRIBUTE_ID,
        ),
        (
            "derived_light_damage",
            FormulaAttributeLocus::Source,
            DERIVED_LIGHT_DAMAGE_ATTRIBUTE_ID,
        ),
    ]
    .into_iter()
    .map(|(name, locus, attribute_id)| formula_candidate_report(cohort, name, locus, attribute_id))
    .collect::<Result<Vec<_>, _>>()?;
    let candidate_bundles = vec![formula_candidate_bundle_report(
        cohort,
        "fatal_spiral_generic_element_damage",
        FormulaAttributeLocus::Source,
        GENERIC_ELEMENT_DAMAGE_ATTRIBUTE_ID,
        GENERIC_ELEMENT_DAMAGE_ATTRIBUTE_FAMILY.collect(),
        vec![FATAL_SPIRAL_TEAM_STATUS_EFFECT_ID],
    )?];
    Ok(FormulaProofBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-state-scaling-damage-proof",
        game_build,
        policy: FormulaProofPolicy {
            state_timing: "wire-message start, before any attribute or status mutation carried by the same decoded wire message",
            strict_scope: "candidate attribute removed; every other retained source, target, and status-provider attribute, semantic status identity, hit flag, and packet formula input must match exactly",
            diagnostic_scope: "same control key except target current HP 11310 is also removed; retained only to discover comparisons because target current HP can be a real formula input and therefore this scope never authorizes rDPS",
            output_fields_removed_from_control_key: "packet normal_value, lucky_value, hit-part damage_value, SkillEffect.uuid, SkillEffect.total_damage, and the AoiSyncDelta group index are occurrence or outcome evidence rather than formula inputs; SkillEffect component index/count are also excluded because the decoder derives them locally from the zero-based damage-array position and array length rather than packet formula fields",
            promotion_rule: "only strict controlled equality plus an exact reversible external provider lifecycle and conserved counterfactual may be promoted into runtime rDPS; diagnostic comparisons and missing packet fields remain unresolved evidence",
        },
        selection: formula_selection_receipt(args),
        gap_window_filter: gap_window_filter.map(|filter| filter.metadata.clone()),
        inputs,
        sample_count: cohort.samples.len(),
        season_vectors,
        season_target_input_proof,
        input_determinism,
        message_scope_determinism,
        formula_surface,
        coefficient_pair_proof,
        post_coefficient_stage,
        candidates,
        candidate_bundles,
    })
}

fn formula_surface_inventory_report(
    cohort: &FormulaCohortAccumulator,
) -> FormulaSurfaceInventoryReport {
    let mut groups = BTreeMap::<FormulaSurfaceGroupKey, FormulaSurfaceGroupAccumulator>::new();
    let mut source_formula_attributes = BTreeMap::<i32, FormulaSurfaceAttributeAccumulator>::new();
    let mut target_formula_attributes = BTreeMap::<i32, FormulaSurfaceAttributeAccumulator>::new();
    let mut samples_with_normal_value = 0_u64;
    let mut samples_with_lucky_value = 0_u64;
    let mut samples_flagged_critical = 0_u64;
    let mut samples_flagged_lucky = 0_u64;
    let mut samples_with_source_physical_attack = 0_u64;
    let mut samples_with_source_magical_attack = 0_u64;
    let mut samples_with_target_physical_defense = 0_u64;
    let mut samples_with_target_magical_defense = 0_u64;

    for sample in &cohort.samples {
        samples_with_normal_value += u64::from(sample.normal_value.is_some());
        samples_with_lucky_value += u64::from(sample.lucky_value.is_some());
        samples_flagged_critical += u64::from(sample.critical == Some(true));
        samples_flagged_lucky += u64::from(sample.lucky == Some(true));

        let source_state = formula_attribute_state(cohort, sample, FormulaAttributeLocus::Source);
        let target_state = formula_attribute_state(cohort, sample, FormulaAttributeLocus::Target);
        samples_with_source_physical_attack +=
            u64::from(compact_attribute(source_state, PHYSICAL_ATTACK_ATTRIBUTE_ID).is_some());
        samples_with_source_magical_attack +=
            u64::from(compact_attribute(source_state, MAGICAL_ATTACK_ATTRIBUTE_ID).is_some());
        samples_with_target_physical_defense +=
            u64::from(compact_attribute(target_state, PHYSICAL_DEFENSE_ATTRIBUTE_ID).is_some());
        samples_with_target_magical_defense +=
            u64::from(compact_attribute(target_state, MAGICAL_DEFENSE_ATTRIBUTE_ID).is_some());

        let key = FormulaSurfaceGroupKey {
            ability_id: sample.ability_id,
            passive_uuid: sample.passive_uuid,
            hit_event_id: sample.hit_event_id,
            damage_source: sample.damage_source,
            damage_type: sample.damage_type,
            critical: sample.critical,
            lucky: sample.lucky,
        };
        let group = groups.entry(key).or_default();
        group.samples += 1;
        group.normal_value_samples += u64::from(sample.normal_value.is_some());
        group.lucky_value_samples += u64::from(sample.lucky_value.is_some());
        group.actual_amount_samples += u64::from(sample.actual_amount.is_some());
        group.hp_loss_samples += u64::from(sample.hp_loss.is_some());
        group.shield_loss_samples += u64::from(sample.shield_loss.is_some());
        if group.examples.len() < DEFAULT_EXAMPLE_LIMIT {
            group.examples.push(FormulaSurfaceExample {
                rlog: sample.rlog.clone(),
                session_id: sample.session_id.clone(),
                run_ordinal: sample.run_ordinal,
                sequence: sample.sequence,
                wire_capture_sequence: sample.wire_capture_sequence,
                observed_micros: sample.observed_micros,
                source_entity_uuid: sample.source_entity_uuid,
                direct_source_entity_uuid: sample.direct_source_entity_uuid,
                target_entity_uuid: sample.target_entity_uuid,
                amount: sample.amount,
                normal_value: sample.normal_value,
                lucky_value: sample.lucky_value,
            });
        }

        for attribute in source_state
            .iter()
            .filter(|attribute| is_source_formula_surface_attribute(attribute.attribute_id))
        {
            observe_formula_surface_attribute(
                &mut source_formula_attributes,
                attribute.attribute_id,
                attribute.value,
            );
            observe_formula_surface_attribute(
                &mut group.source_attributes,
                attribute.attribute_id,
                attribute.value,
            );
        }
        for attribute in target_state
            .iter()
            .filter(|attribute| is_target_formula_surface_attribute(attribute.attribute_id))
        {
            observe_formula_surface_attribute(
                &mut target_formula_attributes,
                attribute.attribute_id,
                attribute.value,
            );
            observe_formula_surface_attribute(
                &mut group.target_attributes,
                attribute.attribute_id,
                attribute.value,
            );
        }
    }

    FormulaSurfaceInventoryReport {
        scope: "all packet-observed ability/passive/hit/source/type/critical/lucky groups; formula-relevant attributes retain complete distinct-value sets",
        group_count: groups.len(),
        sample_count: cohort.samples.len() as u64,
        samples_with_normal_value,
        samples_with_lucky_value,
        samples_flagged_critical,
        samples_flagged_lucky,
        samples_with_source_physical_attack,
        samples_with_source_magical_attack,
        samples_with_target_physical_defense,
        samples_with_target_magical_defense,
        source_formula_attributes: formula_surface_attribute_reports(source_formula_attributes),
        target_formula_attributes: formula_surface_attribute_reports(target_formula_attributes),
        groups: groups
            .into_iter()
            .map(|(key, group)| FormulaSurfaceGroupReport {
                key,
                samples: group.samples,
                normal_value_samples: group.normal_value_samples,
                lucky_value_samples: group.lucky_value_samples,
                actual_amount_samples: group.actual_amount_samples,
                hp_loss_samples: group.hp_loss_samples,
                shield_loss_samples: group.shield_loss_samples,
                source_attributes: formula_surface_attribute_reports(group.source_attributes),
                target_attributes: formula_surface_attribute_reports(group.target_attributes),
                examples: group.examples,
            })
            .collect(),
    }
}

fn observe_formula_surface_attribute(
    attributes: &mut BTreeMap<i32, FormulaSurfaceAttributeAccumulator>,
    attribute_id: i32,
    value: i64,
) {
    let attribute = attributes.entry(attribute_id).or_default();
    attribute.samples += 1;
    attribute.distinct_values.insert(value);
}

fn formula_surface_attribute_reports(
    attributes: BTreeMap<i32, FormulaSurfaceAttributeAccumulator>,
) -> Vec<FormulaSurfaceAttributeReport> {
    attributes
        .into_iter()
        .map(|(attribute_id, attribute)| FormulaSurfaceAttributeReport {
            attribute_id,
            samples: attribute.samples,
            distinct_values: attribute.distinct_values.into_iter().collect(),
        })
        .collect()
}

fn is_source_formula_surface_attribute(attribute_id: i32) -> bool {
    matches!(
        attribute_id,
        PHYSICAL_ATTACK_ATTRIBUTE_ID
            | MAGICAL_ATTACK_ATTRIBUTE_ID
            | PHYSICAL_IGNORE_DEFENSE_ATTRIBUTE_ID
            | MAGICAL_IGNORE_DEFENSE_ATTRIBUTE_ID
            | PHYSICAL_IGNORE_DEFENSE_PERCENT_ATTRIBUTE_ID
            | MAGICAL_IGNORE_DEFENSE_PERCENT_ATTRIBUTE_ID
            | REFINED_PHYSICAL_ATTACK_ATTRIBUTE_ID
            | REFINED_MAGICAL_ATTACK_ATTRIBUTE_ID
            | SEASON_STRENGTH_ATTRIBUTE_ID
            | MASTERY_ATTRIBUTE_ID
            | VERSATILITY_ATTRIBUTE_ID
    ) || attribute_id == ELEMENT_ATTACK_ATTRIBUTE_ID
        || (11510..=11999).contains(&attribute_id)
        || (12500..=13299).contains(&attribute_id)
}

fn is_target_formula_surface_attribute(attribute_id: i32) -> bool {
    (PHYSICAL_DEFENSE_ATTRIBUTE_ID..=MAGICAL_IGNORE_DEFENSE_PERCENT_ATTRIBUTE_ID)
        .contains(&attribute_id)
        || attribute_id == SEASON_STRENGTH_ATTRIBUTE_ID
        || (11500..=11999).contains(&attribute_id)
        || (12500..=13299).contains(&attribute_id)
}

fn formula_input_determinism_report(
    cohort: &FormulaCohortAccumulator,
) -> Result<FormulaInputDeterminismReport, Box<dyn std::error::Error>> {
    let mut groups = BTreeMap::<FormulaExactInputKey, BTreeMap<FormulaOutcome, u64>>::new();
    for sample in &cohort.samples {
        let key = FormulaExactInputKey {
            rlog: sample.rlog.clone(),
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
            packet_inputs: formula_packet_inputs(&sample.packet)?,
            source_attribute_state_id: sample.source_attribute_state_id,
            target_attribute_state_id: sample.target_attribute_state_id,
            source_status_state_id: sample.source_status_state_id,
            target_status_state_id: sample.target_status_state_id,
        };
        *groups
            .entry(key)
            .or_default()
            .entry(formula_outcome(sample))
            .or_default() += 1;
    }

    let input_groups = groups.len() as u64;
    let mut repeated_input_groups = 0_u64;
    let mut repeated_input_samples = 0_u64;
    let mut invariant_repeated_groups = 0_u64;
    let mut divergent_repeated_groups = 0_u64;
    let mut divergent_repeated_samples = 0_u64;
    let mut examples = Vec::new();
    for (key, outcomes) in groups {
        let samples = outcomes.values().copied().sum::<u64>();
        if samples < 2 {
            continue;
        }
        repeated_input_groups += 1;
        repeated_input_samples = repeated_input_samples.saturating_add(samples);
        if outcomes.len() == 1 {
            invariant_repeated_groups += 1;
            continue;
        }
        divergent_repeated_groups += 1;
        divergent_repeated_samples = divergent_repeated_samples.saturating_add(samples);
        examples.push(FormulaInputDeterminismExample {
            rlog: key.rlog,
            session_id: key.session_id,
            run_ordinal: key.run_ordinal,
            source_entity_uuid: key.source_entity_uuid,
            direct_source_entity_uuid: key.direct_source_entity_uuid,
            target_entity_uuid: key.target_entity_uuid,
            ability_id: key.ability_id,
            hit_event_id: key.hit_event_id,
            source_attribute_state_id: key.source_attribute_state_id,
            target_attribute_state_id: key.target_attribute_state_id,
            source_status_state_id: key.source_status_state_id,
            target_status_state_id: key.target_status_state_id,
            samples,
            outcomes: outcomes
                .into_iter()
                .map(|(outcome, samples)| FormulaOutcomeCount { outcome, samples })
                .collect(),
        });
    }
    examples.sort_by(|left, right| {
        right
            .samples
            .cmp(&left.samples)
            .then_with(|| left.ability_id.cmp(&right.ability_id))
            .then_with(|| left.source_entity_uuid.cmp(&right.source_entity_uuid))
            .then_with(|| left.target_entity_uuid.cmp(&right.target_entity_uuid))
    });
    examples.truncate(FORMULA_PROOF_EXAMPLE_LIMIT);

    Ok(FormulaInputDeterminismReport {
        proof_authority: true,
        scope: "same rlog, session, run, source, direct source, target, ability, hit event, flags, complete retained packet inputs, complete retained wire-start attributes, and complete retained semantic status state; time and output values are not inputs",
        input_groups,
        repeated_input_groups,
        repeated_input_samples,
        invariant_repeated_groups,
        divergent_repeated_groups,
        divergent_repeated_samples,
        examples,
    })
}

fn formula_message_scope_report(
    cohort: &FormulaCohortAccumulator,
) -> Result<FormulaMessageScopeReport, Box<dyn std::error::Error>> {
    let mut messages = BTreeMap::<FormulaMessageKey, FormulaMessageAccumulator>::new();
    for sample in &cohort.samples {
        if sample.ability_id != COEFFICIENT_PAIR_ABILITY_ID {
            continue;
        }
        let Some(wire_capture_sequence) = sample.wire_capture_sequence else {
            continue;
        };
        let control = FormulaMessageControlKey {
            rlog: sample.rlog.clone(),
            session_id: sample.session_id.clone(),
            run_ordinal: sample.run_ordinal,
            source_entity_uuid: sample.source_entity_uuid,
            direct_source_entity_uuid: sample.direct_source_entity_uuid,
            ability_id: sample.ability_id,
            passive_uuid: sample.passive_uuid,
            hit_event_id: sample.hit_event_id,
            damage_source: sample.damage_source,
            damage_type: sample.damage_type,
            critical: sample.critical,
            lucky: sample.lucky,
            packet_formula_inputs: formula_message_scope_packet_inputs(&sample.packet)?,
            source_attribute_state_id: sample.source_attribute_state_id,
            source_status_state_id: sample.source_status_state_id,
        };
        let message = messages
            .entry(FormulaMessageKey {
                control,
                wire_capture_sequence,
            })
            .or_default();
        message.observed_micros.insert(sample.observed_micros);
        message
            .target_entity_uuids
            .insert(sample.target_entity_uuid);
        message.target_state_tuples.insert((
            sample.target_entity_uuid,
            sample.target_attribute_state_id,
            sample.target_status_state_id,
        ));
        message
            .target_attribute_state_ids
            .insert(sample.target_attribute_state_id);
        message
            .target_status_state_ids
            .insert(sample.target_status_state_id);
        *message.outcomes.entry(formula_outcome(sample)).or_default() += 1;
    }

    let wire_groups = messages.len() as u64;
    let mut multi_target_wire_groups = 0_u64;
    let mut invariant_multi_target_wire_groups = 0_u64;
    let mut divergent_multi_target_wire_groups = 0_u64;
    let mut controlled =
        BTreeMap::<FormulaMessageControlKey, Vec<(u64, FormulaMessageAccumulator)>>::new();
    for (key, message) in messages {
        if message.target_entity_uuids.len() < 2 {
            continue;
        }
        multi_target_wire_groups += 1;
        if message.outcomes.len() == 1 {
            invariant_multi_target_wire_groups += 1;
            controlled
                .entry(key.control)
                .or_default()
                .push((key.wire_capture_sequence, message));
        } else {
            divergent_multi_target_wire_groups += 1;
        }
    }

    let cross_wire_control_groups = controlled
        .values()
        .filter(|messages| messages.len() >= 2)
        .count() as u64;
    let shared_scalar = formula_message_shared_scalar_report(&controlled);
    let mut divergent_cross_wire_control_groups = 0_u64;
    let mut invariant_wires_in_divergent_control_groups = 0_u64;
    let mut target_samples_in_divergent_control_groups = 0_u64;
    let mut examples = Vec::new();
    for (control, mut wires) in controlled {
        if wires.len() < 2 {
            continue;
        }
        wires.sort_by_key(|(wire_capture_sequence, _)| *wire_capture_sequence);
        let distinct_outcomes = wires
            .iter()
            .filter_map(|(_, message)| message.outcomes.keys().next().cloned())
            .collect::<BTreeSet<_>>();
        if distinct_outcomes.len() < 2 {
            continue;
        }
        divergent_cross_wire_control_groups += 1;
        invariant_wires_in_divergent_control_groups =
            invariant_wires_in_divergent_control_groups.saturating_add(wires.len() as u64);
        target_samples_in_divergent_control_groups = target_samples_in_divergent_control_groups
            .saturating_add(
                wires
                    .iter()
                    .map(|(_, message)| message.outcomes.values().copied().sum::<u64>())
                    .sum::<u64>(),
            );
        examples.push(FormulaMessageScopeExample {
            rlog: control.rlog,
            session_id: control.session_id,
            run_ordinal: control.run_ordinal,
            source_entity_uuid: control.source_entity_uuid,
            direct_source_entity_uuid: control.direct_source_entity_uuid,
            ability_id: control.ability_id,
            hit_event_id: control.hit_event_id,
            source_attribute_state_id: control.source_attribute_state_id,
            source_status_state_id: control.source_status_state_id,
            wires: wires
                .into_iter()
                .map(
                    |(wire_capture_sequence, message)| FormulaMessageWireExample {
                        wire_capture_sequence,
                        observed_micros: message.observed_micros.into_iter().collect(),
                        target_entity_count: message.target_entity_uuids.len() as u64,
                        target_attribute_state_count: message.target_attribute_state_ids.len()
                            as u64,
                        target_status_state_count: message.target_status_state_ids.len() as u64,
                        target_samples: message.outcomes.values().copied().sum(),
                        outcomes: message
                            .outcomes
                            .into_iter()
                            .map(|(outcome, samples)| FormulaOutcomeCount { outcome, samples })
                            .collect(),
                    },
                )
                .collect(),
        });
    }
    examples.sort_by(|left, right| {
        right
            .wires
            .len()
            .cmp(&left.wires.len())
            .then_with(|| left.hit_event_id.cmp(&right.hit_event_id))
            .then_with(|| left.source_entity_uuid.cmp(&right.source_entity_uuid))
    });
    examples.truncate(FORMULA_PROOF_EXAMPLE_LIMIT);

    Ok(FormulaMessageScopeReport {
        proof_authority: false,
        scope: "ability 2352 only; same rlog, session, run, source, direct source, hit event, flags, packet formula fields excluding output and spatial coordinates, and complete source wire-start attributes/statuses. Each retained wire group must contain at least two distinct targets and exactly one outcome; target identities and complete target state IDs remain counted rather than assumed equivalent. Divergent outcomes across two or more such wire groups identify a missing wire/message-scoped calculation input, but do not identify its arithmetic or authorize rDPS.",
        ability_id: COEFFICIENT_PAIR_ABILITY_ID,
        wire_groups,
        multi_target_wire_groups,
        invariant_multi_target_wire_groups,
        divergent_multi_target_wire_groups,
        cross_wire_control_groups,
        divergent_cross_wire_control_groups,
        invariant_wires_in_divergent_control_groups,
        target_samples_in_divergent_control_groups,
        shared_scalar,
        examples,
    })
}

fn formula_message_shared_scalar_report(
    controlled: &BTreeMap<FormulaMessageControlKey, Vec<(u64, FormulaMessageAccumulator)>>,
) -> FormulaMessageSharedScalarReport {
    let mut wire_pairs = BTreeMap::<
        FormulaMessageSharedScalarPairKey,
        Vec<FormulaMessageSharedScalarSignature>,
    >::new();
    for (control, wires) in controlled {
        if wires.len() < 2 {
            continue;
        }
        let context = FormulaMessageSharedScalarContext {
            rlog: control.rlog.clone(),
            session_id: control.session_id.clone(),
            run_ordinal: control.run_ordinal,
            source_entity_uuid: control.source_entity_uuid,
            direct_source_entity_uuid: control.direct_source_entity_uuid,
            ability_id: control.ability_id,
            passive_uuid: control.passive_uuid,
            hit_event_id: control.hit_event_id,
            damage_source: control.damage_source,
            damage_type: control.damage_type,
            source_attribute_state_id: control.source_attribute_state_id,
            source_status_state_id: control.source_status_state_id,
        };
        for from_index in 0..wires.len() {
            for to_index in (from_index + 1)..wires.len() {
                let (from_wire_capture_sequence, from_message) = &wires[from_index];
                let (to_wire_capture_sequence, to_message) = &wires[to_index];
                let Some(from_output) = formula_message_reference_output(from_message) else {
                    continue;
                };
                let Some(to_output) = formula_message_reference_output(to_message) else {
                    continue;
                };
                if from_output <= 0 || to_output <= 0 {
                    continue;
                }
                let exact_target_state_overlap = from_message
                    .target_state_tuples
                    .intersection(&to_message.target_state_tuples)
                    .count() as u64;
                wire_pairs
                    .entry(FormulaMessageSharedScalarPairKey {
                        context: context.clone(),
                        from_wire_capture_sequence: *from_wire_capture_sequence,
                        to_wire_capture_sequence: *to_wire_capture_sequence,
                    })
                    .or_default()
                    .push(FormulaMessageSharedScalarSignature {
                        control_signature: format!(
                            "critical={:?};lucky={:?};packet={}",
                            control.critical, control.lucky, control.packet_formula_inputs
                        ),
                        from_output,
                        to_output,
                        exact_target_state_overlap,
                    });
            }
        }
    }

    let wire_pair_candidates = wire_pairs.len() as u64;
    let mut multi_signature_wire_pairs = 0_u64;
    let mut exact_target_state_overlap_wire_pairs = 0_u64;
    let mut identity_ratio_wire_pairs = 0_u64;
    let mut changed_ratio_wire_pairs = 0_u64;
    let mut floor_interval_consistent_wire_pairs = 0_u64;
    let mut floor_interval_inconsistent_wire_pairs = 0_u64;
    let mut exact_target_state_floor_interval_consistent_wire_pairs = 0_u64;
    let mut exact_target_state_floor_interval_inconsistent_wire_pairs = 0_u64;
    let mut maximum_signature_support = 0_u64;
    let mut maximum_exact_target_state_signature_support = 0_u64;
    let mut examples = Vec::new();
    for (pair, signatures) in wire_pairs {
        if signatures.len() < 2 {
            continue;
        }
        multi_signature_wire_pairs += 1;
        maximum_signature_support = maximum_signature_support.max(signatures.len() as u64);

        let mut common_lower = 0.0_f64;
        let mut common_upper = f64::INFINITY;
        let mut exact_target_state_common_lower = 0.0_f64;
        let mut exact_target_state_common_upper = f64::INFINITY;
        let mut exact_target_state_signature_support = 0_u64;
        let mut ratio_min = i64::MAX;
        let mut ratio_max = i64::MIN;
        let mut signature_examples = Vec::new();
        for signature in signatures {
            let observed_ratio = signature.to_output as f64 / signature.from_output as f64;
            let floor_lower =
                signature.to_output as f64 / signature.from_output.saturating_add(1) as f64;
            let floor_upper =
                signature.to_output.saturating_add(1) as f64 / signature.from_output as f64;
            common_lower = common_lower.max(floor_lower);
            common_upper = common_upper.min(floor_upper);
            if signature.exact_target_state_overlap > 0 {
                exact_target_state_signature_support += 1;
                exact_target_state_common_lower = exact_target_state_common_lower.max(floor_lower);
                exact_target_state_common_upper = exact_target_state_common_upper.min(floor_upper);
            }
            let observed_ratio_parts_per_million = ratio_parts_per_million(observed_ratio);
            ratio_min = ratio_min.min(observed_ratio_parts_per_million);
            ratio_max = ratio_max.max(observed_ratio_parts_per_million);
            signature_examples.push(FormulaMessageSharedScalarSignatureExample {
                control_signature: signature.control_signature,
                from_output: signature.from_output,
                to_output: signature.to_output,
                exact_target_state_overlap: signature.exact_target_state_overlap,
                observed_ratio_parts_per_million,
                floor_ratio_lower_parts_per_million: ratio_parts_per_million(floor_lower),
                floor_ratio_upper_parts_per_million: ratio_parts_per_million(floor_upper),
            });
        }
        let floor_interval_consistent = common_lower <= common_upper;
        let exact_target_state_floor_interval_consistent = (exact_target_state_signature_support
            >= 2)
            .then_some(exact_target_state_common_lower <= exact_target_state_common_upper);
        if exact_target_state_signature_support >= 2 {
            exact_target_state_overlap_wire_pairs += 1;
            maximum_exact_target_state_signature_support =
                maximum_exact_target_state_signature_support
                    .max(exact_target_state_signature_support);
            if exact_target_state_floor_interval_consistent == Some(true) {
                exact_target_state_floor_interval_consistent_wire_pairs += 1;
            } else {
                exact_target_state_floor_interval_inconsistent_wire_pairs += 1;
            }
        }
        if ratio_min == 1_000_000 && ratio_max == 1_000_000 {
            identity_ratio_wire_pairs += 1;
        } else {
            changed_ratio_wire_pairs += 1;
        }
        if floor_interval_consistent {
            floor_interval_consistent_wire_pairs += 1;
        } else {
            floor_interval_inconsistent_wire_pairs += 1;
        }
        let signature_support = signature_examples.len() as u64;
        signature_examples.sort_by(|left, right| {
            right
                .from_output
                .cmp(&left.from_output)
                .then_with(|| right.to_output.cmp(&left.to_output))
                .then_with(|| left.control_signature.cmp(&right.control_signature))
        });
        signature_examples.truncate(8);
        examples.push(FormulaMessageSharedScalarExample {
            rlog: pair.context.rlog,
            session_id: pair.context.session_id,
            run_ordinal: pair.context.run_ordinal,
            source_entity_uuid: pair.context.source_entity_uuid,
            direct_source_entity_uuid: pair.context.direct_source_entity_uuid,
            ability_id: pair.context.ability_id,
            hit_event_id: pair.context.hit_event_id,
            source_attribute_state_id: pair.context.source_attribute_state_id,
            source_status_state_id: pair.context.source_status_state_id,
            from_wire_capture_sequence: pair.from_wire_capture_sequence,
            to_wire_capture_sequence: pair.to_wire_capture_sequence,
            signature_support,
            exact_target_state_signature_support,
            floor_interval_consistent,
            exact_target_state_floor_interval_consistent,
            observed_ratio_parts_per_million_min: ratio_min,
            observed_ratio_parts_per_million_max: ratio_max,
            observed_ratio_parts_per_million_spread: ratio_max.saturating_sub(ratio_min),
            common_floor_ratio_lower_parts_per_million: ratio_parts_per_million(common_lower),
            common_floor_ratio_upper_parts_per_million: ratio_parts_per_million(common_upper),
            signatures: signature_examples,
        });
    }
    examples.sort_by(|left, right| {
        right
            .exact_target_state_signature_support
            .cmp(&left.exact_target_state_signature_support)
            .then_with(|| {
                shared_scalar_ratio_deviation(right).cmp(&shared_scalar_ratio_deviation(left))
            })
            .then_with(|| right.signature_support.cmp(&left.signature_support))
            .then_with(|| {
                left.observed_ratio_parts_per_million_spread
                    .cmp(&right.observed_ratio_parts_per_million_spread)
            })
            .then_with(|| {
                left.from_wire_capture_sequence
                    .cmp(&right.from_wire_capture_sequence)
            })
    });
    examples.truncate(FORMULA_PROOF_EXAMPLE_LIMIT);

    FormulaMessageSharedScalarReport {
        proof_authority: false,
        scope: "ability 2352 only; pairs two wire messages inside the same complete retained source-side context. Every contributing packet formula signature must be invariant across at least two target entities in each wire. The relaxed counters remain target-set-confounded. Exact-target-state counters require at least two independent signatures where the same target entity, target attribute state, and target status state occur in both wires. A common interval is the intersection of conservative floor-ratio bounds y/(x+1) through (y+1)/x. Even exact-target-state consistency proves only compatibility with one shared scalar; it does not identify the scalar, its distribution, or its formula stage and does not authorize rDPS.",
        wire_pair_candidates,
        multi_signature_wire_pairs,
        exact_target_state_overlap_wire_pairs,
        identity_ratio_wire_pairs,
        changed_ratio_wire_pairs,
        floor_interval_consistent_wire_pairs,
        floor_interval_inconsistent_wire_pairs,
        exact_target_state_floor_interval_consistent_wire_pairs,
        exact_target_state_floor_interval_inconsistent_wire_pairs,
        maximum_signature_support,
        maximum_exact_target_state_signature_support,
        examples,
    }
}

fn shared_scalar_ratio_deviation(example: &FormulaMessageSharedScalarExample) -> i64 {
    example
        .observed_ratio_parts_per_million_min
        .saturating_sub(1_000_000)
        .abs()
        .max(
            example
                .observed_ratio_parts_per_million_max
                .saturating_sub(1_000_000)
                .abs(),
        )
}

fn formula_message_reference_output(message: &FormulaMessageAccumulator) -> Option<i64> {
    if message.outcomes.len() != 1 {
        return None;
    }
    let outcome = message.outcomes.keys().next()?;
    Some(outcome.normal.unwrap_or(outcome.amount))
}

fn ratio_parts_per_million(value: f64) -> i64 {
    if !value.is_finite() {
        return i64::MAX;
    }
    (value * 1_000_000.0).round() as i64
}

fn formula_coefficient_pair_report(
    cohort: &FormulaCohortAccumulator,
) -> Result<FormulaCoefficientPairReport, Box<dyn std::error::Error>> {
    let mut groups = BTreeMap::<
        FormulaCoefficientPairKey,
        (Vec<&FormulaCohortSample>, Vec<&FormulaCohortSample>),
    >::new();
    for sample in &cohort.samples {
        if sample.ability_id != COEFFICIENT_PAIR_ABILITY_ID {
            continue;
        }
        let Some(hit_event_id) = sample.hit_event_id else {
            continue;
        };
        if hit_event_id != COEFFICIENT_PAIR_HIGH_EVENT_ID
            && hit_event_id != COEFFICIENT_PAIR_LOW_EVENT_ID
        {
            continue;
        }
        let key = FormulaCoefficientPairKey {
            rlog: sample.rlog.clone(),
            session_id: sample.session_id.clone(),
            run_ordinal: sample.run_ordinal,
            wire_capture_sequence: sample.wire_capture_sequence,
            observed_micros: sample.observed_micros,
            source_entity_uuid: sample.source_entity_uuid,
            direct_source_entity_uuid: sample.direct_source_entity_uuid,
            target_entity_uuid: sample.target_entity_uuid,
            passive_uuid: sample.passive_uuid,
            damage_source: sample.damage_source,
            damage_type: sample.damage_type,
            critical: sample.critical,
            lucky: sample.lucky,
            property: sample.packet.property,
            damage_mode: sample.packet.damage_mode,
            source_attribute_state_id: sample.source_attribute_state_id,
            target_attribute_state_id: sample.target_attribute_state_id,
            source_status_state_id: sample.source_status_state_id,
            target_status_state_id: sample.target_status_state_id,
        };
        let entry = groups.entry(key).or_default();
        if hit_event_id == COEFFICIENT_PAIR_HIGH_EVENT_ID {
            entry.0.push(sample);
        } else {
            entry.1.push(sample);
        }
    }

    let mut candidate_groups = 0_u64;
    let mut candidate_comparisons = 0_u64;
    let mut evaluated_nearest_sequence_comparisons = 0_u64;
    let mut exact_proportional_comparisons = 0_u64;
    let near_proportional_residual_exclusive = COEFFICIENT_PAIR_HIGH_BASIS_POINTS;
    let mut near_proportional_comparisons = 0_u64;
    let mut packet_inputs_equal_comparisons = 0_u64;
    let mut residuals = BTreeMap::<i64, u64>::new();
    let mut packet_input_pairs = BTreeMap::<(String, String), BTreeMap<i64, u64>>::new();
    let mut examples = Vec::new();
    for (key, (high_events, low_events)) in groups {
        if high_events.is_empty() || low_events.is_empty() {
            continue;
        }
        candidate_groups += 1;
        candidate_comparisons = candidate_comparisons.saturating_add(
            u64::try_from(high_events.len().saturating_mul(low_events.len())).unwrap_or(u64::MAX),
        );
        for high in high_events {
            let Some(low) = low_events
                .iter()
                .min_by_key(|low| high.sequence.abs_diff(low.sequence))
            else {
                continue;
            };
            evaluated_nearest_sequence_comparisons += 1;
            let residual = coefficient_cross_product_residual(high.amount, low.amount);
            if residual == 0 {
                exact_proportional_comparisons += 1;
            }
            if residual.saturating_abs() < near_proportional_residual_exclusive {
                near_proportional_comparisons += 1;
            }
            *residuals.entry(residual).or_default() += 1;
            let high_packet_inputs = formula_packet_inputs(&high.packet)?;
            let low_packet_inputs = formula_packet_inputs(&low.packet)?;
            if high_packet_inputs == low_packet_inputs {
                packet_inputs_equal_comparisons += 1;
            }
            *packet_input_pairs
                .entry((high_packet_inputs.clone(), low_packet_inputs.clone()))
                .or_default()
                .entry(residual)
                .or_default() += 1;
            examples.push(FormulaCoefficientPairExample {
                rlog: key.rlog.clone(),
                session_id: key.session_id.clone(),
                run_ordinal: key.run_ordinal,
                wire_capture_sequence: key.wire_capture_sequence,
                observed_micros: key.observed_micros,
                source_entity_uuid: key.source_entity_uuid,
                target_entity_uuid: key.target_entity_uuid,
                source_attribute_state_id: key.source_attribute_state_id,
                target_attribute_state_id: key.target_attribute_state_id,
                source_status_state_id: key.source_status_state_id,
                target_status_state_id: key.target_status_state_id,
                high_sequence: high.sequence,
                low_sequence: low.sequence,
                sequence_gap: high.sequence.abs_diff(low.sequence),
                high_amount: high.amount,
                low_amount: low.amount,
                high_normal_value: high.normal_value,
                low_normal_value: low.normal_value,
                cross_product_residual: residual,
                ratio_basis_points: (low.amount != 0)
                    .then(|| high.amount.saturating_mul(10_000) / low.amount),
                high_event_packet_inputs: high_packet_inputs,
                low_event_packet_inputs: low_packet_inputs,
            });
        }
    }
    let residual_min = residuals.keys().next().copied();
    let residual_max = residuals.keys().next_back().copied();
    let packet_input_pair_variants = packet_input_pairs.len() as u64;
    let mut packet_input_pairs = packet_input_pairs
        .into_iter()
        .map(
            |((high_event_packet_inputs, low_event_packet_inputs), residuals)| {
                let comparisons = residuals.values().copied().sum();
                FormulaPacketInputPairCount {
                    high_event_packet_inputs,
                    low_event_packet_inputs,
                    comparisons,
                    residuals: residuals
                        .into_iter()
                        .map(|(cross_product_residual, comparisons)| {
                            FormulaCoefficientResidualCount {
                                cross_product_residual,
                                comparisons,
                            }
                        })
                        .collect(),
                }
            },
        )
        .collect::<Vec<_>>();
    packet_input_pairs.sort_by_key(|entry| std::cmp::Reverse(entry.comparisons));
    packet_input_pairs.truncate(FORMULA_PROOF_EXAMPLE_LIMIT);
    examples.sort_by(|left, right| {
        left.cross_product_residual
            .saturating_abs()
            .cmp(&right.cross_product_residual.saturating_abs())
            .then_with(|| left.sequence_gap.cmp(&right.sequence_gap))
            .then_with(|| left.high_sequence.cmp(&right.high_sequence))
    });
    examples.truncate(FORMULA_PROOF_EXAMPLE_LIMIT);

    Ok(FormulaCoefficientPairReport {
        formula_stage_authority: evaluated_nearest_sequence_comparisons > 0
            && exact_proportional_comparisons == evaluated_nearest_sequence_comparisons
            && packet_inputs_equal_comparisons == evaluated_nearest_sequence_comparisons,
        ability_id: COEFFICIENT_PAIR_ABILITY_ID,
        high_event_id: COEFFICIENT_PAIR_HIGH_EVENT_ID,
        low_event_id: COEFFICIENT_PAIR_LOW_EVENT_ID,
        high_coefficient_basis_points: COEFFICIENT_PAIR_HIGH_BASIS_POINTS,
        low_coefficient_basis_points: COEFFICIENT_PAIR_LOW_BASIS_POINTS,
        matching_scope: "same wire capture, capture time, source, direct source, target, complete wire-start attribute and status state, crit/lucky state, damage source/type, property, and damage mode; each high event is evaluated against the nearest-sequence low event while every possible comparison remains counted; event-specific packet inputs are retained as paired evidence rather than assumed equal",
        candidate_groups,
        candidate_comparisons,
        evaluated_nearest_sequence_comparisons,
        exact_proportional_comparisons,
        near_proportional_residual_exclusive,
        near_proportional_comparisons,
        packet_inputs_equal_comparisons,
        residual_min,
        residual_max,
        residuals: residuals
            .into_iter()
            .map(
                |(cross_product_residual, comparisons)| FormulaCoefficientResidualCount {
                    cross_product_residual,
                    comparisons,
                },
            )
            .collect(),
        packet_input_pair_variants,
        packet_input_pairs,
        examples,
    })
}

fn formula_post_coefficient_stage_report(
    cohort: &FormulaCohortAccumulator,
) -> FormulaPostCoefficientStageReport {
    let applicable = cohort
        .samples
        .iter()
        .any(|sample| sample.ability_id == COEFFICIENT_PAIR_ABILITY_ID);
    let mut groups = BTreeMap::<
        FormulaCoefficientPairKey,
        (Vec<&FormulaCohortSample>, Vec<&FormulaCohortSample>),
    >::new();
    let mut samples_with_source_attack = 0_u64;
    let mut samples_with_positive_coefficient_body_and_normal_output = 0_u64;
    let mut individual_integer_factor_interval_compatible_samples = 0_u64;
    let mut individual_integer_factor_interval_incompatible_samples = 0_u64;

    for sample in &cohort.samples {
        if sample.ability_id != COEFFICIENT_PAIR_ABILITY_ID {
            continue;
        }
        let Some(hit_event_id) = sample.hit_event_id else {
            continue;
        };
        let coefficient_basis_points = match hit_event_id {
            COEFFICIENT_PAIR_HIGH_EVENT_ID => COEFFICIENT_PAIR_HIGH_BASIS_POINTS,
            COEFFICIENT_PAIR_LOW_EVENT_ID => COEFFICIENT_PAIR_LOW_BASIS_POINTS,
            _ => continue,
        };
        let source_state = formula_attribute_state(cohort, sample, FormulaAttributeLocus::Source);
        let Some(source_attack) = compact_attribute(source_state, PHYSICAL_ATTACK_ATTRIBUTE_ID)
        else {
            continue;
        };
        samples_with_source_attack += 1;
        let Some(body) = coefficient_body(source_attack, coefficient_basis_points) else {
            continue;
        };
        let Some(normal_value) = sample.normal_value.filter(|value| *value >= 0) else {
            continue;
        };
        samples_with_positive_coefficient_body_and_normal_output += 1;
        if fixed_point_factor_interval(normal_value, body).is_some() {
            individual_integer_factor_interval_compatible_samples += 1;
        } else {
            individual_integer_factor_interval_incompatible_samples += 1;
        }

        let key = FormulaCoefficientPairKey {
            rlog: sample.rlog.clone(),
            session_id: sample.session_id.clone(),
            run_ordinal: sample.run_ordinal,
            wire_capture_sequence: sample.wire_capture_sequence,
            observed_micros: sample.observed_micros,
            source_entity_uuid: sample.source_entity_uuid,
            direct_source_entity_uuid: sample.direct_source_entity_uuid,
            target_entity_uuid: sample.target_entity_uuid,
            passive_uuid: sample.passive_uuid,
            damage_source: sample.damage_source,
            damage_type: sample.damage_type,
            critical: sample.critical,
            lucky: sample.lucky,
            property: sample.packet.property,
            damage_mode: sample.packet.damage_mode,
            source_attribute_state_id: sample.source_attribute_state_id,
            target_attribute_state_id: sample.target_attribute_state_id,
            source_status_state_id: sample.source_status_state_id,
            target_status_state_id: sample.target_status_state_id,
        };
        let entry = groups.entry(key).or_default();
        if hit_event_id == COEFFICIENT_PAIR_HIGH_EVENT_ID {
            entry.0.push(sample);
        } else {
            entry.1.push(sample);
        }
    }

    let mut paired_groups_with_source_attack = 0_u64;
    let mut paired_groups_with_positive_bodies_and_normal_outputs = 0_u64;
    let mut paired_integer_factor_interval_consistent_groups = 0_u64;
    let mut paired_integer_factor_interval_inconsistent_groups = 0_u64;
    let mut paired_exact_integer_factor_groups = 0_u64;
    let mut factor_intervals = BTreeMap::<(i64, i64), u64>::new();
    let mut examples = Vec::new();

    for (key, (high_events, low_events)) in groups {
        if high_events.is_empty() || low_events.is_empty() {
            continue;
        }
        paired_groups_with_source_attack += 1;
        let Some((high, low)) = high_events
            .iter()
            .flat_map(|high| low_events.iter().map(move |low| (*high, *low)))
            .min_by_key(|(high, low)| high.sequence.abs_diff(low.sequence))
        else {
            continue;
        };
        let source_state = formula_attribute_state(cohort, high, FormulaAttributeLocus::Source);
        let target_state = formula_attribute_state(cohort, high, FormulaAttributeLocus::Target);
        let Some(source_attack) = compact_attribute(source_state, PHYSICAL_ATTACK_ATTRIBUTE_ID)
        else {
            continue;
        };
        let Some(high_body) = coefficient_body(source_attack, COEFFICIENT_PAIR_HIGH_BASIS_POINTS)
        else {
            continue;
        };
        let Some(low_body) = coefficient_body(source_attack, COEFFICIENT_PAIR_LOW_BASIS_POINTS)
        else {
            continue;
        };
        let (Some(high_normal), Some(low_normal)) = (high.normal_value, low.normal_value) else {
            continue;
        };
        if high_normal < 0 || low_normal < 0 {
            continue;
        }
        paired_groups_with_positive_bodies_and_normal_outputs += 1;
        let high_interval = fixed_point_factor_interval(high_normal, high_body);
        let low_interval = fixed_point_factor_interval(low_normal, low_body);
        let shared_interval = high_interval.zip(low_interval).and_then(|(high, low)| {
            let shared_minimum = high.0.max(low.0);
            let shared_maximum = high.1.min(low.1);
            (shared_minimum <= shared_maximum).then_some((shared_minimum, shared_maximum))
        });
        if let Some(interval) = shared_interval {
            paired_integer_factor_interval_consistent_groups += 1;
            if interval.0 == interval.1 {
                paired_exact_integer_factor_groups += 1;
            }
            *factor_intervals.entry(interval).or_default() += 1;
        } else {
            paired_integer_factor_interval_inconsistent_groups += 1;
        }

        examples.push(FormulaPostCoefficientStageExample {
            rlog: key.rlog.clone(),
            session_id: key.session_id.clone(),
            run_ordinal: key.run_ordinal,
            wire_capture_sequence: key.wire_capture_sequence,
            source_entity_uuid: key.source_entity_uuid,
            target_entity_uuid: key.target_entity_uuid,
            source_attribute_state_id: key.source_attribute_state_id,
            target_attribute_state_id: key.target_attribute_state_id,
            source_status_state_id: key.source_status_state_id,
            target_status_state_id: key.target_status_state_id,
            source_attack,
            source_mastery_basis_points: compact_attribute(source_state, MASTERY_ATTRIBUTE_ID),
            source_versatility_basis_points: compact_attribute(
                source_state,
                VERSATILITY_ATTRIBUTE_ID,
            ),
            source_light_damage_basis_points: compact_attribute(
                source_state,
                DERIVED_LIGHT_DAMAGE_ATTRIBUTE_ID,
            ),
            source_season_strength: compact_attribute(source_state, SEASON_STRENGTH_ATTRIBUTE_ID),
            target_physical_defense: compact_attribute(target_state, PHYSICAL_DEFENSE_ATTRIBUTE_ID),
            target_season_strength: compact_attribute(target_state, SEASON_STRENGTH_ATTRIBUTE_ID),
            critical: key.critical,
            lucky: key.lucky,
            high_sequence: high.sequence,
            low_sequence: low.sequence,
            high_coefficient_body: high_body,
            low_coefficient_body: low_body,
            high_normal_value: high_normal,
            low_normal_value: low_normal,
            high_factor_interval_minimum_basis_points: high_interval.map(|value| value.0),
            high_factor_interval_maximum_basis_points: high_interval.map(|value| value.1),
            low_factor_interval_minimum_basis_points: low_interval.map(|value| value.0),
            low_factor_interval_maximum_basis_points: low_interval.map(|value| value.1),
            shared_factor_interval_minimum_basis_points: shared_interval.map(|value| value.0),
            shared_factor_interval_maximum_basis_points: shared_interval.map(|value| value.1),
        });
    }

    examples.sort_by(|left, right| {
        left.shared_factor_interval_minimum_basis_points
            .is_some()
            .cmp(&right.shared_factor_interval_minimum_basis_points.is_some())
            .then_with(|| left.source_attack.cmp(&right.source_attack))
            .then_with(|| left.high_sequence.cmp(&right.high_sequence))
    });
    examples.truncate(FORMULA_PROOF_EXAMPLE_LIMIT);

    FormulaPostCoefficientStageReport {
        proof_authority: false,
        applicable,
        not_applicable_reason: (!applicable).then_some(
            "the selected cohort contains no ability 2352 samples; zero counters below are not evidence that source Attack is absent",
        ),
        scope: "current-build ability 2352 events 1 and 3 only. The current game table proves coefficient bodies floor(Attack * 50000 / 10000) and floor(Attack * 8000 / 10000). This report asks whether both observed packet normal values for the same wire, source, target, complete wire-start attribute/status state, and flags are compatible with one additional integer basis-point floor stage. Compatibility isolates a post-coefficient factor interval; it does not identify its constituent defense, season, mastery, versatility, elemental, critical, lucky, random, or hidden inputs and never authorizes rDPS by itself.",
        fixed_point_denominator: 10_000,
        ability_id: COEFFICIENT_PAIR_ABILITY_ID,
        high_event_id: COEFFICIENT_PAIR_HIGH_EVENT_ID,
        low_event_id: COEFFICIENT_PAIR_LOW_EVENT_ID,
        high_coefficient_basis_points: COEFFICIENT_PAIR_HIGH_BASIS_POINTS,
        low_coefficient_basis_points: COEFFICIENT_PAIR_LOW_BASIS_POINTS,
        samples_with_source_attack,
        samples_with_positive_coefficient_body_and_normal_output,
        individual_integer_factor_interval_compatible_samples,
        individual_integer_factor_interval_incompatible_samples,
        paired_groups_with_source_attack,
        paired_groups_with_positive_bodies_and_normal_outputs,
        paired_integer_factor_interval_consistent_groups,
        paired_integer_factor_interval_inconsistent_groups,
        paired_exact_integer_factor_groups,
        factor_intervals: factor_intervals
            .into_iter()
            .map(
                |((minimum_factor_basis_points, maximum_factor_basis_points), groups)| {
                    FormulaPostCoefficientFactorIntervalCount {
                        minimum_factor_basis_points,
                        maximum_factor_basis_points,
                        groups,
                    }
                },
            )
            .collect(),
        examples,
    }
}

fn coefficient_body(source_attack: i64, coefficient_basis_points: i64) -> Option<i64> {
    if source_attack <= 0 || coefficient_basis_points <= 0 {
        return None;
    }
    let body =
        i128::from(source_attack).checked_mul(i128::from(coefficient_basis_points))? / 10_000;
    (body > 0 && body <= i128::from(i64::MAX)).then_some(body as i64)
}

fn fixed_point_factor_interval(output: i64, body: i64) -> Option<(i64, i64)> {
    if output < 0 || body <= 0 {
        return None;
    }
    let output = i128::from(output);
    let body = i128::from(body);
    let denominator = 10_000_i128;
    let lower_numerator = output.checked_mul(denominator)?;
    let lower = lower_numerator
        .checked_add(body.checked_sub(1)?)?
        .checked_div(body)?;
    let upper = output
        .checked_add(1)?
        .checked_mul(denominator)?
        .checked_sub(1)?
        .checked_div(body)?;
    (lower <= upper && lower >= 0 && upper <= i128::from(i64::MAX))
        .then_some((lower as i64, upper as i64))
}

fn formula_outcome(sample: &FormulaCohortSample) -> FormulaOutcome {
    FormulaOutcome {
        normal: sample.normal_value,
        lucky: sample.lucky_value,
        amount: sample.amount,
        actual: sample.actual_amount,
        hp_loss: sample.hp_loss,
        shield_loss: sample.shield_loss,
    }
}

fn coefficient_cross_product_residual(high_amount: i64, low_amount: i64) -> i64 {
    high_amount
        .saturating_mul(COEFFICIENT_PAIR_LOW_BASIS_POINTS)
        .saturating_sub(low_amount.saturating_mul(COEFFICIENT_PAIR_HIGH_BASIS_POINTS))
}

fn season_vector_counts(cohort: &FormulaCohortAccumulator) -> Vec<SeasonVectorCount> {
    let mut counts = BTreeMap::<SeasonVector, u64>::new();
    for sample in &cohort.samples {
        let source = cohort
            .attribute_states
            .get(sample.source_attribute_state_id as usize)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let target = cohort
            .attribute_states
            .get(sample.target_attribute_state_id as usize)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let source_season_strength = compact_attribute(source, SEASON_STRENGTH_ATTRIBUTE_ID);
        let target_season_strength = compact_attribute(target, SEASON_STRENGTH_ATTRIBUTE_ID);
        let vector = SeasonVector {
            source_season_strength,
            source_target_season_input: compact_attribute(source, SEASON_TARGET_INPUT_ATTRIBUTE_ID),
            target_season_strength,
            target_target_season_input: compact_attribute(target, SEASON_TARGET_INPUT_ATTRIBUTE_ID),
            source_minus_target_season_strength: source_season_strength
                .zip(target_season_strength)
                .map(|(source, target)| source.saturating_sub(target)),
        };
        *counts.entry(vector).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(vector, samples)| SeasonVectorCount { vector, samples })
        .collect()
}

fn season_target_input_proof(cohort: &FormulaCohortAccumulator) -> SeasonTargetInputProof {
    let mut samples_with_source_target_input = 0_u64;
    let mut samples_with_target_season_strength = 0_u64;
    let mut comparable_samples = 0_u64;
    let mut exact_match_samples = 0_u64;
    let mut mismatch_samples = 0_u64;
    let mut source_target_input_without_target_strength_samples = 0_u64;
    let mut target_strength_without_source_target_input_samples = 0_u64;
    let mut mismatch_counts = BTreeMap::<(i64, i64), u64>::new();

    for sample in &cohort.samples {
        let source = formula_attribute_state(cohort, sample, FormulaAttributeLocus::Source);
        let target = formula_attribute_state(cohort, sample, FormulaAttributeLocus::Target);
        let source_target_input = compact_attribute(source, SEASON_TARGET_INPUT_ATTRIBUTE_ID);
        let target_season_strength = compact_attribute(target, SEASON_STRENGTH_ATTRIBUTE_ID);

        samples_with_source_target_input += u64::from(source_target_input.is_some());
        samples_with_target_season_strength += u64::from(target_season_strength.is_some());
        match (source_target_input, target_season_strength) {
            (Some(source_value), Some(target_value)) => {
                comparable_samples += 1;
                if source_value == target_value {
                    exact_match_samples += 1;
                } else {
                    mismatch_samples += 1;
                    *mismatch_counts
                        .entry((source_value, target_value))
                        .or_default() += 1;
                }
            }
            (Some(_), None) => source_target_input_without_target_strength_samples += 1,
            (None, Some(_)) => target_strength_without_source_target_input_samples += 1,
            (None, None) => {}
        }
    }

    SeasonTargetInputProof {
        proof_authority: comparable_samples > 0 && mismatch_samples == 0,
        source_attribute_id: SEASON_TARGET_INPUT_ATTRIBUTE_ID,
        target_attribute_id: SEASON_STRENGTH_ATTRIBUTE_ID,
        samples: cohort.samples.len() as u64,
        samples_with_source_target_input,
        samples_with_target_season_strength,
        comparable_samples,
        exact_match_samples,
        mismatch_samples,
        source_target_input_without_target_strength_samples,
        target_strength_without_source_target_input_samples,
        distinct_mismatches: mismatch_counts
            .into_iter()
            .map(|((source_target_input, target_season_strength), samples)| {
                SeasonTargetInputMismatchCount {
                    source_target_input,
                    target_season_strength,
                    samples,
                }
            })
            .collect(),
        conclusion: "within comparable wire-start damage samples, source attribute 11450 is tested as a mirrored target-season input and not inferred from its static table label",
        damage_formula_boundary: "exact equality proves the packet input identity only; it does not yet prove the season multiplier, cap, row selection, or rounding order",
    }
}

fn formula_candidate_report(
    cohort: &FormulaCohortAccumulator,
    name: &'static str,
    locus: FormulaAttributeLocus,
    attribute_id: i32,
) -> Result<FormulaCandidateReport, Box<dyn std::error::Error>> {
    let mut values = BTreeSet::new();
    let mut samples_with_attribute = 0_u64;
    for sample in &cohort.samples {
        let state = formula_attribute_state(cohort, sample, locus);
        if let Some(value) = compact_attribute(state, attribute_id) {
            samples_with_attribute += 1;
            values.insert(value);
        }
    }
    let strict_all_observed_state = controlled_scope_report(cohort, locus, attribute_id, false)?;
    let mut target_current_hp_excluded_diagnostic =
        controlled_scope_report(cohort, locus, attribute_id, true)?;
    target_current_hp_excluded_diagnostic.proof_authority = false;
    Ok(FormulaCandidateReport {
        name,
        locus: match locus {
            FormulaAttributeLocus::Source => "source",
            FormulaAttributeLocus::Target => "target",
        },
        attribute_id,
        samples_with_attribute,
        minimum_value: values.first().copied(),
        maximum_value: values.last().copied(),
        distinct_values: values.into_iter().collect(),
        strict_all_observed_state,
        target_current_hp_excluded_diagnostic,
    })
}

fn formula_candidate_bundle_report(
    cohort: &FormulaCohortAccumulator,
    name: &'static str,
    locus: FormulaAttributeLocus,
    primary_attribute_id: i32,
    removed_attribute_ids: Vec<i32>,
    removed_source_status_effect_ids: Vec<i64>,
) -> Result<FormulaCandidateBundleReport, Box<dyn std::error::Error>> {
    let mut values = BTreeSet::new();
    let mut samples_with_primary_attribute = 0_u64;
    for sample in &cohort.samples {
        let state = formula_attribute_state(cohort, sample, locus);
        if let Some(value) = compact_attribute(state, primary_attribute_id) {
            samples_with_primary_attribute += 1;
            values.insert(value);
        }
    }
    let strict_all_observed_state = controlled_scope_report_bundle(
        cohort,
        locus,
        primary_attribute_id,
        &removed_attribute_ids,
        &removed_source_status_effect_ids,
        false,
        false,
        false,
    )?;
    let mut target_current_hp_excluded_diagnostic = controlled_scope_report_bundle(
        cohort,
        locus,
        primary_attribute_id,
        &removed_attribute_ids,
        &removed_source_status_effect_ids,
        true,
        false,
        false,
    )?;
    target_current_hp_excluded_diagnostic.proof_authority = false;
    let mut position_excluded_diagnostic = controlled_scope_report_bundle(
        cohort,
        locus,
        primary_attribute_id,
        &removed_attribute_ids,
        &removed_source_status_effect_ids,
        false,
        true,
        false,
    )?;
    position_excluded_diagnostic.proof_authority = false;
    let mut position_and_target_current_hp_excluded_diagnostic = controlled_scope_report_bundle(
        cohort,
        locus,
        primary_attribute_id,
        &removed_attribute_ids,
        &removed_source_status_effect_ids,
        true,
        true,
        false,
    )?;
    position_and_target_current_hp_excluded_diagnostic.proof_authority = false;
    let mut position_hp_and_non_candidate_statuses_excluded_diagnostic =
        controlled_scope_report_bundle(
            cohort,
            locus,
            primary_attribute_id,
            &removed_attribute_ids,
            &removed_source_status_effect_ids,
            true,
            true,
            true,
        )?;
    position_hp_and_non_candidate_statuses_excluded_diagnostic.proof_authority = false;
    let near_pair_diagnostics = formula_bundle_near_pair_diagnostics(
        cohort,
        locus,
        primary_attribute_id,
        &removed_attribute_ids,
        &removed_source_status_effect_ids,
    )?;
    let basis_point_multiplier_check =
        basis_point_multiplier_check(&strict_all_observed_state.transitions);
    Ok(FormulaCandidateBundleReport {
        name,
        locus: match locus {
            FormulaAttributeLocus::Source => "source",
            FormulaAttributeLocus::Target => "target",
        },
        primary_attribute_id,
        removed_attribute_ids,
        removed_source_status_effect_ids,
        samples_with_primary_attribute,
        distinct_primary_values: values.into_iter().collect(),
        strict_all_observed_state,
        target_current_hp_excluded_diagnostic,
        position_excluded_diagnostic,
        position_and_target_current_hp_excluded_diagnostic,
        position_hp_and_non_candidate_statuses_excluded_diagnostic,
        near_pair_diagnostics,
        basis_point_multiplier_check,
    })
}

fn formula_bundle_near_pair_diagnostics(
    cohort: &FormulaCohortAccumulator,
    locus: FormulaAttributeLocus,
    primary_attribute_id: i32,
    removed_attribute_ids: &[i32],
    removed_source_status_effect_ids: &[i64],
) -> Result<FormulaBundleNearPairDiagnostics, Box<dyn std::error::Error>> {
    let removed_attribute_ids = removed_attribute_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let removed_source_status_effect_ids = removed_source_status_effect_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut groups = BTreeMap::<FormulaControlKey, BTreeMap<i64, Vec<&FormulaCohortSample>>>::new();
    for sample in &cohort.samples {
        let candidate_state = formula_attribute_state(cohort, sample, locus);
        let Some(candidate_value) = compact_attribute(candidate_state, primary_attribute_id) else {
            continue;
        };
        let source_attributes = cohort
            .attribute_states
            .get(sample.source_attribute_state_id as usize)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|attribute| {
                !(matches!(locus, FormulaAttributeLocus::Source)
                    && removed_attribute_ids.contains(&attribute.attribute_id))
            })
            .collect();
        let target_attributes = cohort
            .attribute_states
            .get(sample.target_attribute_state_id as usize)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|attribute| {
                !(matches!(locus, FormulaAttributeLocus::Target)
                    && removed_attribute_ids.contains(&attribute.attribute_id))
                    && attribute.attribute_id != CURRENT_HP_ATTRIBUTE_ID
            })
            .collect();
        let key = FormulaControlKey {
            ability_id: sample.ability_id,
            passive_uuid: sample.passive_uuid,
            hit_event_id: sample.hit_event_id,
            damage_source: sample.damage_source,
            damage_type: sample.damage_type,
            critical: sample.critical,
            lucky: sample.lucky,
            packet_inputs: formula_message_scope_packet_inputs(&sample.packet)?,
            source_attributes,
            target_attributes,
            source_statuses: Vec::new(),
            target_statuses: Vec::new(),
            status_provider_attributes: sample.status_provider_attribute_states.clone(),
        };
        groups
            .entry(key)
            .or_default()
            .entry(candidate_value)
            .or_default()
            .push(sample);
    }

    let mut controlled_groups = 0_u64;
    let mut comparisons = 0_u64;
    let mut signatures = BTreeMap::<FormulaStatusDifferenceSignature, u64>::new();
    let mut examples = Vec::new();
    for values in groups.into_values().filter(|values| values.len() >= 2) {
        controlled_groups = controlled_groups.saturating_add(1);
        let values = values.into_iter().collect::<Vec<_>>();
        for left_index in 0..values.len() {
            for right_index in (left_index + 1)..values.len() {
                let (candidate_from, samples_from) = &values[left_index];
                let (candidate_to, samples_to) = &values[right_index];
                for from in samples_from {
                    for to in samples_to {
                        comparisons = comparisons.saturating_add(1);
                        let differences = formula_status_differences(
                            cohort,
                            from,
                            to,
                            &removed_source_status_effect_ids,
                        );
                        *signatures.entry(differences.clone()).or_default() += 1;
                        if examples.len() < FORMULA_PROOF_EXAMPLE_LIMIT {
                            examples.push(FormulaBundleNearPairExample {
                                candidate_from: *candidate_from,
                                candidate_to: *candidate_to,
                                from: formula_near_pair_sample(cohort, from),
                                to: formula_near_pair_sample(cohort, to),
                                status_differences: differences,
                            });
                        }
                    }
                }
            }
        }
    }
    let mut status_difference_signatures = signatures
        .into_iter()
        .map(
            |(signature, comparisons)| FormulaStatusDifferenceSignatureCount {
                signature,
                comparisons,
            },
        )
        .collect::<Vec<_>>();
    status_difference_signatures.sort_by(|left, right| {
        right
            .comparisons
            .cmp(&left.comparisons)
            .then_with(|| left.signature.cmp(&right.signature))
    });
    status_difference_signatures.truncate(FORMULA_PROOF_EXAMPLE_LIMIT);
    Ok(FormulaBundleNearPairDiagnostics {
        proof_authority: false,
        scope: "same ability, hit identity, hit flags, non-candidate attributes, and message-scope packet inputs after excluding position and target current HP; all remaining source and target status differences are retained explicitly below",
        controlled_groups,
        comparisons,
        status_difference_signatures,
        examples,
    })
}

fn formula_status_differences(
    cohort: &FormulaCohortAccumulator,
    from: &FormulaCohortSample,
    to: &FormulaCohortSample,
    removed_source_status_effect_ids: &BTreeSet<i64>,
) -> FormulaStatusDifferenceSignature {
    let source_from = formula_status_set(
        cohort,
        from.source_status_state_id,
        removed_source_status_effect_ids,
    );
    let source_to = formula_status_set(
        cohort,
        to.source_status_state_id,
        removed_source_status_effect_ids,
    );
    let target_from = formula_status_set(cohort, from.target_status_state_id, &BTreeSet::new());
    let target_to = formula_status_set(cohort, to.target_status_state_id, &BTreeSet::new());
    FormulaStatusDifferenceSignature {
        source_only_from: source_from.difference(&source_to).copied().collect(),
        source_only_to: source_to.difference(&source_from).copied().collect(),
        target_only_from: target_from.difference(&target_to).copied().collect(),
        target_only_to: target_to.difference(&target_from).copied().collect(),
    }
}

fn formula_status_set(
    cohort: &FormulaCohortAccumulator,
    state_id: u32,
    removed_effect_ids: &BTreeSet<i64>,
) -> BTreeSet<CompactFormulaStatus> {
    cohort
        .status_states
        .get(state_id as usize)
        .into_iter()
        .flatten()
        .filter(|status| !removed_effect_ids.contains(&status.effect_id))
        .copied()
        .collect()
}

fn formula_near_pair_sample(
    cohort: &FormulaCohortAccumulator,
    sample: &FormulaCohortSample,
) -> FormulaNearPairSample {
    FormulaNearPairSample {
        rlog: sample.rlog.clone(),
        session_id: sample.session_id.clone(),
        run_ordinal: sample.run_ordinal,
        sequence: sample.sequence,
        observed_micros: sample.observed_micros,
        wire_capture_sequence: sample.wire_capture_sequence,
        source_entity_uuid: sample.source_entity_uuid,
        direct_source_entity_uuid: sample.direct_source_entity_uuid,
        target_entity_uuid: sample.target_entity_uuid,
        ability_id: sample.ability_id,
        passive_uuid: sample.passive_uuid,
        hit_event_id: sample.hit_event_id,
        damage_source: sample.damage_source,
        damage_type: sample.damage_type,
        damage_property: sample.packet.property,
        target_current_hp: cohort
            .attribute_states
            .get(sample.target_attribute_state_id as usize)
            .and_then(|state| compact_attribute(state, CURRENT_HP_ATTRIBUTE_ID)),
        source_attributes: cohort
            .attribute_states
            .get(sample.source_attribute_state_id as usize)
            .cloned()
            .unwrap_or_default(),
        target_attributes: cohort
            .attribute_states
            .get(sample.target_attribute_state_id as usize)
            .cloned()
            .unwrap_or_default(),
        outcome: FormulaOutcome {
            normal: sample.normal_value,
            lucky: sample.lucky_value,
            amount: sample.amount,
            actual: sample.actual_amount,
            hp_loss: sample.hp_loss,
            shield_loss: sample.shield_loss,
        },
    }
}

fn controlled_scope_report_bundle(
    cohort: &FormulaCohortAccumulator,
    locus: FormulaAttributeLocus,
    primary_attribute_id: i32,
    removed_attribute_ids: &[i32],
    removed_source_status_effect_ids: &[i64],
    exclude_target_current_hp: bool,
    exclude_position: bool,
    exclude_non_candidate_statuses: bool,
) -> Result<FormulaControlledScopeReport, Box<dyn std::error::Error>> {
    let removed_attribute_ids = removed_attribute_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let removed_source_status_effect_ids = removed_source_status_effect_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut groups =
        BTreeMap::<FormulaControlKey, BTreeMap<i64, BTreeMap<FormulaOutcome, u64>>>::new();
    for sample in &cohort.samples {
        let candidate_state = formula_attribute_state(cohort, sample, locus);
        let Some(candidate_value) = compact_attribute(candidate_state, primary_attribute_id) else {
            continue;
        };
        let source_attributes = cohort
            .attribute_states
            .get(sample.source_attribute_state_id as usize)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|attribute| {
                !(matches!(locus, FormulaAttributeLocus::Source)
                    && removed_attribute_ids.contains(&attribute.attribute_id))
            })
            .collect();
        let target_attributes = cohort
            .attribute_states
            .get(sample.target_attribute_state_id as usize)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|attribute| {
                !(matches!(locus, FormulaAttributeLocus::Target)
                    && removed_attribute_ids.contains(&attribute.attribute_id))
                    && !(exclude_target_current_hp
                        && attribute.attribute_id == CURRENT_HP_ATTRIBUTE_ID)
            })
            .collect();
        let source_statuses = if exclude_non_candidate_statuses {
            Vec::new()
        } else {
            cohort
                .status_states
                .get(sample.source_status_state_id as usize)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|status| !removed_source_status_effect_ids.contains(&status.effect_id))
                .collect()
        };
        let key = FormulaControlKey {
            ability_id: sample.ability_id,
            passive_uuid: sample.passive_uuid,
            hit_event_id: sample.hit_event_id,
            damage_source: sample.damage_source,
            damage_type: sample.damage_type,
            critical: sample.critical,
            lucky: sample.lucky,
            packet_inputs: if exclude_position {
                formula_message_scope_packet_inputs(&sample.packet)?
            } else {
                formula_packet_inputs(&sample.packet)?
            },
            source_attributes,
            target_attributes,
            source_statuses,
            target_statuses: if exclude_non_candidate_statuses {
                Vec::new()
            } else {
                cohort
                    .status_states
                    .get(sample.target_status_state_id as usize)
                    .cloned()
                    .unwrap_or_default()
            },
            status_provider_attributes: sample.status_provider_attribute_states.clone(),
        };
        let outcome = FormulaOutcome {
            normal: sample.normal_value,
            lucky: sample.lucky_value,
            amount: sample.amount,
            actual: sample.actual_amount,
            hp_loss: sample.hp_loss,
            shield_loss: sample.shield_loss,
        };
        *groups
            .entry(key)
            .or_default()
            .entry(candidate_value)
            .or_default()
            .entry(outcome)
            .or_default() += 1;
    }
    controlled_scope_report_from_groups(groups, !exclude_target_current_hp)
}

fn basis_point_multiplier_check(
    transitions: &[FormulaValueTransitionCount],
) -> FormulaBasisPointMultiplierCheck {
    let mut evaluated_normal_comparisons = 0_u64;
    let mut exact_normal_comparisons = 0_u64;
    let mut evaluated_amount_comparisons = 0_u64;
    let mut exact_amount_comparisons = 0_u64;
    let mut normal_residuals = BTreeMap::<i128, u64>::new();
    let mut amount_residuals = BTreeMap::<i128, u64>::new();
    for transition in transitions {
        let from_factor =
            i128::from(10_000_i64.saturating_add(transition.transition.candidate_from));
        let to_factor = i128::from(10_000_i64.saturating_add(transition.transition.candidate_to));
        if from_factor <= 0 || to_factor <= 0 {
            continue;
        }
        if let (Some(from), Some(to)) = (
            transition.transition.normal_from,
            transition.transition.normal_to,
        ) {
            evaluated_normal_comparisons =
                evaluated_normal_comparisons.saturating_add(transition.comparisons);
            let residual = i128::from(to) * from_factor - i128::from(from) * to_factor;
            exact_normal_comparisons = exact_normal_comparisons
                .saturating_add(u64::from(residual == 0).saturating_mul(transition.comparisons));
            *normal_residuals.entry(residual).or_default() += transition.comparisons;
        }
        evaluated_amount_comparisons =
            evaluated_amount_comparisons.saturating_add(transition.comparisons);
        let residual = i128::from(transition.transition.amount_to) * from_factor
            - i128::from(transition.transition.amount_from) * to_factor;
        exact_amount_comparisons = exact_amount_comparisons
            .saturating_add(u64::from(residual == 0).saturating_mul(transition.comparisons));
        *amount_residuals.entry(residual).or_default() += transition.comparisons;
    }
    FormulaBasisPointMultiplierCheck {
        proof_authority: evaluated_normal_comparisons > 0
            && exact_normal_comparisons == evaluated_normal_comparisons,
        hypothesis: "with every non-candidate wire-start attribute, status, hit flag, and packet formula input fixed, normal output is multiplied by (10000 + generic element damage) / 10000",
        evaluated_normal_comparisons,
        exact_normal_comparisons,
        evaluated_amount_comparisons,
        exact_amount_comparisons,
        normal_cross_product_residuals: residual_counts(normal_residuals),
        amount_cross_product_residuals: residual_counts(amount_residuals),
    }
}

fn residual_counts(counts: BTreeMap<i128, u64>) -> Vec<FormulaResidualCount> {
    let mut values = counts
        .into_iter()
        .map(|(residual, comparisons)| FormulaResidualCount {
            residual,
            comparisons,
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .comparisons
            .cmp(&left.comparisons)
            .then_with(|| left.residual.abs().cmp(&right.residual.abs()))
    });
    values.truncate(FORMULA_PROOF_EXAMPLE_LIMIT);
    values
}

fn controlled_scope_report(
    cohort: &FormulaCohortAccumulator,
    locus: FormulaAttributeLocus,
    attribute_id: i32,
    exclude_target_current_hp: bool,
) -> Result<FormulaControlledScopeReport, Box<dyn std::error::Error>> {
    let mut groups =
        BTreeMap::<FormulaControlKey, BTreeMap<i64, BTreeMap<FormulaOutcome, u64>>>::new();
    for sample in &cohort.samples {
        let candidate_state = formula_attribute_state(cohort, sample, locus);
        let Some(candidate_value) = compact_attribute(candidate_state, attribute_id) else {
            continue;
        };
        let source_attributes = cohort
            .attribute_states
            .get(sample.source_attribute_state_id as usize)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|attribute| {
                !(matches!(locus, FormulaAttributeLocus::Source)
                    && attribute.attribute_id == attribute_id)
            })
            .collect();
        let target_attributes = cohort
            .attribute_states
            .get(sample.target_attribute_state_id as usize)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|attribute| {
                !(matches!(locus, FormulaAttributeLocus::Target)
                    && attribute.attribute_id == attribute_id)
                    && !(exclude_target_current_hp
                        && attribute.attribute_id == CURRENT_HP_ATTRIBUTE_ID)
            })
            .collect();
        let key = FormulaControlKey {
            ability_id: sample.ability_id,
            passive_uuid: sample.passive_uuid,
            hit_event_id: sample.hit_event_id,
            damage_source: sample.damage_source,
            damage_type: sample.damage_type,
            critical: sample.critical,
            lucky: sample.lucky,
            packet_inputs: formula_packet_inputs(&sample.packet)?,
            source_attributes,
            target_attributes,
            source_statuses: cohort
                .status_states
                .get(sample.source_status_state_id as usize)
                .cloned()
                .unwrap_or_default(),
            target_statuses: cohort
                .status_states
                .get(sample.target_status_state_id as usize)
                .cloned()
                .unwrap_or_default(),
            status_provider_attributes: sample.status_provider_attribute_states.clone(),
        };
        let outcome = FormulaOutcome {
            normal: sample.normal_value,
            lucky: sample.lucky_value,
            amount: sample.amount,
            actual: sample.actual_amount,
            hp_loss: sample.hp_loss,
            shield_loss: sample.shield_loss,
        };
        *groups
            .entry(key)
            .or_default()
            .entry(candidate_value)
            .or_default()
            .entry(outcome)
            .or_default() += 1;
    }
    controlled_scope_report_from_groups(groups, !exclude_target_current_hp)
}

fn controlled_scope_report_from_groups(
    groups: BTreeMap<FormulaControlKey, BTreeMap<i64, BTreeMap<FormulaOutcome, u64>>>,
    proof_authority: bool,
) -> Result<FormulaControlledScopeReport, Box<dyn std::error::Error>> {
    let mut controlled_groups = 0_u64;
    let mut controlled_samples = 0_u64;
    let mut transitions = BTreeMap::<FormulaValueTransition, u64>::new();
    for values in groups.into_values().filter(|values| values.len() >= 2) {
        controlled_groups += 1;
        controlled_samples += values
            .values()
            .flat_map(BTreeMap::values)
            .copied()
            .sum::<u64>();
        let values = values.into_iter().collect::<Vec<_>>();
        for left_index in 0..values.len() {
            for right_index in (left_index + 1)..values.len() {
                let (candidate_from, outcomes_from) = &values[left_index];
                let (candidate_to, outcomes_to) = &values[right_index];
                for (from, from_count) in outcomes_from {
                    for (to, to_count) in outcomes_to {
                        let transition = FormulaValueTransition {
                            candidate_from: *candidate_from,
                            candidate_to: *candidate_to,
                            normal_from: from.normal,
                            normal_to: to.normal,
                            lucky_from: from.lucky,
                            lucky_to: to.lucky,
                            amount_from: from.amount,
                            amount_to: to.amount,
                            actual_from: from.actual,
                            actual_to: to.actual,
                            hp_loss_from: from.hp_loss,
                            hp_loss_to: to.hp_loss,
                            shield_loss_from: from.shield_loss,
                            shield_loss_to: to.shield_loss,
                        };
                        *transitions.entry(transition).or_default() +=
                            from_count.saturating_mul(*to_count);
                    }
                }
            }
        }
    }
    Ok(FormulaControlledScopeReport {
        // An otherwise authoritative scope cannot prove a formula without an
        // observed transition. Keep empty searches explicitly non-authoritative.
        proof_authority: proof_authority && controlled_groups > 0,
        controlled_groups,
        controlled_samples,
        transitions: transitions
            .into_iter()
            .map(|(transition, comparisons)| FormulaValueTransitionCount {
                transition,
                comparisons,
            })
            .collect(),
    })
}

fn formula_attribute_state<'a>(
    cohort: &'a FormulaCohortAccumulator,
    sample: &FormulaCohortSample,
    locus: FormulaAttributeLocus,
) -> &'a [CompactFormulaAttribute] {
    let id = match locus {
        FormulaAttributeLocus::Source => sample.source_attribute_state_id,
        FormulaAttributeLocus::Target => sample.target_attribute_state_id,
    };
    cohort
        .attribute_states
        .get(id as usize)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn compact_attribute(state: &[CompactFormulaAttribute], attribute_id: i32) -> Option<i64> {
    state
        .iter()
        .find(|attribute| attribute.attribute_id == attribute_id)
        .map(|attribute| attribute.value)
}

fn formula_packet_inputs(packet: &DamagePacketDetail) -> Result<String, serde_json::Error> {
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
    serde_json::to_string(&packet)
}

fn formula_message_scope_packet_inputs(
    packet: &DamagePacketDetail,
) -> Result<String, serde_json::Error> {
    let mut packet = packet.clone();
    packet.dead = None;
    packet.normal_value = None;
    packet.lucky_value = None;
    packet.skill_effect_uuid = None;
    packet.skill_effect_total_damage = None;
    packet.skill_effect_group_index = None;
    packet.skill_effect_component_index = None;
    packet.skill_effect_component_count = None;
    packet.position = None;
    for hit_part in &mut packet.hit_parts {
        hit_part.position = None;
        hit_part.damage_value = None;
    }
    serde_json::to_string(&packet)
}

fn state_integer(value: &Option<StateScalar>) -> Option<i64> {
    value.as_ref().and_then(|value| value.integer_varint)
}

fn state_missing_hp(state: &StateSnapshot) -> Option<i64> {
    let max_hp = state_integer(&state.max_hp_final)?;
    let current_hp = state_integer(&state.current_hp)?;
    Some(max_hp.saturating_sub(current_hp))
}

fn shield_origin_evidence(
    state: &StateSnapshot,
    origins: Option<&BTreeMap<i64, ShieldOriginRecord>>,
) -> Vec<ShieldOriginEvidence> {
    state
        .current_shields
        .as_ref()
        .map(|list| {
            list.shields
                .iter()
                .map(|shield| {
                    let origin = shield
                        .uuid
                        .and_then(|uuid| origins.and_then(|entries| entries.get(&uuid)));
                    ShieldOriginEvidence {
                        shield_uuid: shield.uuid,
                        shield_type: shield.shield_type,
                        current_value: shield.current_value,
                        effect_id: origin.map(|origin| origin.effect_id),
                        source_entity_uuid: origin.and_then(|origin| origin.source_entity_uuid),
                        origin_source_type_id: origin
                            .and_then(|origin| origin.origin_source_type_id),
                        origin_source_config_id: origin
                            .and_then(|origin| origin.origin_source_config_id),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
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

fn actor_key(session_id: &str, run_ordinal: u32, entity_uuid: i64) -> String {
    format!("{session_id}:{run_ordinal}:{entity_uuid}")
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn parse_arguments<I>(mut arguments: I) -> Result<Arguments, String>
where
    I: Iterator<Item = OsString>,
{
    let mut rlogs = Vec::new();
    let mut source_entities = BTreeSet::new();
    let mut effects = BTreeSet::new();
    let mut abilities = BTreeSet::new();
    let mut sequences = BTreeSet::new();
    let mut all_abilities = false;
    let mut output = None;
    let mut proof_only = false;
    let mut inventory_output = None;
    let mut effect_ability_inventory_output = None;
    let mut example_limit = DEFAULT_EXAMPLE_LIMIT;
    let mut formula_cohort_output = None;
    let mut formula_proof_output = None;
    let mut formula_sample_limit = 100_000_usize;
    let mut formula_target_effects = BTreeSet::new();
    let mut formula_source_effects = BTreeSet::new();
    let mut formula_gap_window_audit = None;
    let mut formula_transition_seeds = None;
    let mut formula_transition_window_micros = 2_000_000_u64;
    while let Some(flag) = arguments.next() {
        match flag.to_string_lossy().as_ref() {
            "--rlog" => rlogs.push(next_path(&mut arguments, "--rlog")?),
            "--source-entity" => {
                source_entities.insert(next_i64(&mut arguments, "--source-entity")?);
            }
            "--effect" => {
                effects.insert(next_i64(&mut arguments, "--effect")?);
            }
            "--ability" => {
                abilities.insert(next_i64(&mut arguments, "--ability")?);
            }
            "--sequence" => {
                sequences.insert(next_u64(&mut arguments, "--sequence")?);
            }
            "--all-abilities" => all_abilities = true,
            "--proof-only" => proof_only = true,
            "--output" => output = Some(next_path(&mut arguments, "--output")?),
            "--inventory-output" => {
                inventory_output = Some(next_path(&mut arguments, "--inventory-output")?);
            }
            "--effect-ability-inventory-output" => {
                effect_ability_inventory_output = Some(next_path(
                    &mut arguments,
                    "--effect-ability-inventory-output",
                )?);
            }
            "--formula-cohort-output" => {
                formula_cohort_output = Some(next_path(&mut arguments, "--formula-cohort-output")?);
            }
            "--formula-proof-output" => {
                formula_proof_output = Some(next_path(&mut arguments, "--formula-proof-output")?);
            }
            "--example-limit" => {
                example_limit = next_usize(&mut arguments, "--example-limit")?;
            }
            "--formula-sample-limit" => {
                formula_sample_limit = next_usize(&mut arguments, "--formula-sample-limit")?;
            }
            "--formula-target-effect" => {
                formula_target_effects.insert(next_i64(&mut arguments, "--formula-target-effect")?);
            }
            "--formula-source-effect" => {
                formula_source_effects.insert(next_i64(&mut arguments, "--formula-source-effect")?);
            }
            "--formula-gap-window-audit" => {
                formula_gap_window_audit =
                    Some(next_path(&mut arguments, "--formula-gap-window-audit")?);
            }
            "--formula-transition-seeds" => {
                formula_transition_seeds =
                    Some(next_path(&mut arguments, "--formula-transition-seeds")?);
            }
            "--formula-transition-window-micros" => {
                formula_transition_window_micros =
                    next_u64(&mut arguments, "--formula-transition-window-micros")?;
            }
            _ => return Err(usage()),
        }
    }
    if !formula_target_effects.is_empty() && !formula_source_effects.is_empty() {
        return Err(
            "--formula-target-effect and --formula-source-effect cannot be combined; source-side and target-side joins are independent"
                .to_owned(),
        );
    }
    let formula_effect_locus = if formula_source_effects.is_empty() {
        FormulaEffectLocus::Target
    } else {
        formula_target_effects = formula_source_effects;
        FormulaEffectLocus::Source
    };
    if rlogs.is_empty()
        || (source_entities.is_empty()
            && effects.is_empty()
            && abilities.is_empty()
            && sequences.is_empty()
            && formula_target_effects.is_empty()
            && !all_abilities)
    {
        return Err(usage());
    }
    if proof_only
        && formula_cohort_output.is_none()
        && formula_proof_output.is_none()
        && effect_ability_inventory_output.is_none()
    {
        return Err(
            "--proof-only requires --formula-cohort-output, --formula-proof-output, or --effect-ability-inventory-output".to_owned(),
        );
    }
    if !proof_only && output.is_none() {
        return Err(usage());
    }
    if formula_transition_seeds.is_some()
        && formula_cohort_output.is_none()
        && formula_proof_output.is_none()
    {
        return Err(
            "--formula-transition-seeds requires --formula-cohort-output or --formula-proof-output"
                .to_owned(),
        );
    }
    if !formula_target_effects.is_empty()
        && formula_cohort_output.is_none()
        && formula_proof_output.is_none()
    {
        return Err(
            "--formula-target-effect requires --formula-cohort-output or --formula-proof-output"
                .to_owned(),
        );
    }
    if formula_gap_window_audit.is_some() && formula_target_effects.len() != 1 {
        return Err(
            "--formula-gap-window-audit requires exactly one --formula-target-effect or --formula-source-effect".to_owned(),
        );
    }
    Ok(Arguments {
        rlogs,
        source_entities,
        effects,
        abilities,
        sequences,
        all_abilities,
        output,
        proof_only,
        inventory_output,
        effect_ability_inventory_output,
        example_limit,
        formula_cohort_output,
        formula_proof_output,
        formula_sample_limit,
        formula_target_effects,
        formula_effect_locus,
        formula_gap_window_audit,
        formula_transition_seeds,
        formula_transition_window_micros,
    })
}

fn next_path<I>(arguments: &mut I, flag: &str) -> Result<PathBuf, String>
where
    I: Iterator<Item = OsString>,
{
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing path after {flag}\n{}", usage()))
}

fn next_i64<I>(arguments: &mut I, flag: &str) -> Result<i64, String>
where
    I: Iterator<Item = OsString>,
{
    let value = arguments
        .next()
        .ok_or_else(|| format!("missing value after {flag}"))?;
    value
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("{flag} requires an integer"))
}

fn next_usize<I>(arguments: &mut I, flag: &str) -> Result<usize, String>
where
    I: Iterator<Item = OsString>,
{
    let value = arguments
        .next()
        .ok_or_else(|| format!("missing value after {flag}"))?;
    value
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
}

fn next_u64<I>(arguments: &mut I, flag: &str) -> Result<u64, String>
where
    I: Iterator<Item = OsString>,
{
    let value = arguments
        .next()
        .ok_or_else(|| format!("missing value after {flag}"))?;
    value
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-state-scaling-damage-proof --rlog <current-decoder.rlog> [--rlog ...] [--all-abilities | --ability <packet-ability-id> ...] [--sequence <canonical-sequence> ...] [--source-entity <talent-or-factor-id> ...] [--effect <runtime-buff-id> ...] [--output <proof.json> | --proof-only] [--inventory-output <compact-inventory.json>] [--effect-ability-inventory-output <compact-effect-abilities.json>] [--formula-cohort-output <compact-wire-start-samples.json>] [--formula-proof-output <strict-controlled-pair-report.json>] [--formula-target-effect <exact-target-effect-id> ... | --formula-source-effect <exact-source-effect-id> ...] [--formula-gap-window-audit <exact-build-gap-windows.json>] [--formula-transition-seeds <complete-single-term-transition-seeds.json>] [--formula-transition-window-micros <count>] [--formula-sample-limit <count>] [--example-limit <count>]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_transition_seed_filter(window_micros: u64) -> FormulaTransitionSeedFilter {
        FormulaTransitionSeedFilter {
            metadata: FormulaTransitionSeedFilterMetadata {
                source: "seeds.json".to_owned(),
                source_sha256: "00".repeat(32),
                window_micros_before_and_after: window_micros,
                retained_transition_seeds: 1,
                selected_effect_ids: vec![2_207_252],
                selected_attribute_ids: vec![11_030],
                selection_policy: "test",
                formula_authority: false,
            },
            seeds_by_recipient: BTreeMap::from([(("session".to_owned(), 2, 17), vec![1_000])]),
        }
    }

    #[test]
    fn transition_seed_filter_requires_same_session_run_recipient_and_window() {
        let filter = test_transition_seed_filter(100);
        assert!(filter.matches("session", 2, 17, 900));
        assert!(filter.matches("session", 2, 17, 1_100));
        assert!(!filter.matches("other", 2, 17, 1_000));
        assert!(!filter.matches("session", 3, 17, 1_000));
        assert!(!filter.matches("session", 2, 18, 1_000));
        assert!(!filter.matches("session", 2, 17, 1_101));
    }

    #[test]
    fn transition_seed_filter_requires_formula_output() {
        let error = parse_arguments(
            [
                "--rlog",
                "one.rlog",
                "--all-abilities",
                "--proof-only",
                "--formula-transition-seeds",
                "seeds.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect_err("a transition filter without formula output must fail closed");
        assert!(error.contains("requires --formula-cohort-output"));
    }

    #[test]
    fn formula_target_effect_is_an_exact_formula_selection_without_an_ability_filter() {
        let args = parse_arguments(
            [
                "--rlog",
                "one.rlog",
                "--formula-target-effect",
                "2110092",
                "--proof-only",
                "--formula-proof-output",
                "proof.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("target-effect formula selection should parse");

        assert_eq!(
            args.formula_target_effects,
            [2_110_092].into_iter().collect()
        );
        assert!(args.abilities.is_empty());
        let receipt = formula_selection_receipt(&args);
        assert_eq!(receipt.selected_effect_ids, vec![2_110_092]);
        assert!(receipt.source_effect_ids.is_empty());
        assert_eq!(receipt.target_effect_ids, vec![2_110_092]);
        assert_eq!(receipt.effect_locus, FormulaEffectLocus::Target);
        assert!(!receipt.formula_authority);
    }

    #[test]
    fn formula_source_effect_selects_the_damage_actor_without_requiring_remote_casts() {
        let args = parse_arguments(
            [
                "--rlog",
                "one.rlog",
                "--formula-source-effect",
                "2110125",
                "--proof-only",
                "--formula-proof-output",
                "proof.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("source-effect formula selection should parse");

        assert_eq!(
            args.formula_target_effects,
            [2_110_125].into_iter().collect()
        );
        assert_eq!(args.formula_effect_locus, FormulaEffectLocus::Source);
        let receipt = formula_selection_receipt(&args);
        assert_eq!(receipt.selected_effect_ids, vec![2_110_125]);
        assert_eq!(receipt.source_effect_ids, vec![2_110_125]);
        assert!(receipt.target_effect_ids.is_empty());
        assert_eq!(receipt.effect_locus, FormulaEffectLocus::Source);
        assert!(
            receipt
                .target_effect_scope
                .contains("canonical damage actor")
        );
    }

    #[test]
    fn formula_source_and_target_effect_selectors_cannot_be_mixed() {
        let error = parse_arguments(
            [
                "--rlog",
                "one.rlog",
                "--formula-source-effect",
                "2110125",
                "--formula-target-effect",
                "55228",
                "--proof-only",
                "--formula-proof-output",
                "proof.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect_err("independent endpoint joins must not be conflated");
        assert!(error.contains("source-side and target-side joins are independent"));
    }

    #[test]
    fn formula_target_effect_requires_formula_output() {
        let error = parse_arguments(
            [
                "--rlog",
                "one.rlog",
                "--formula-target-effect",
                "2110092",
                "--output",
                "general.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect_err("a target-effect selector without formula output must fail closed");

        assert!(error.contains("--formula-target-effect requires"));
    }

    #[test]
    fn formula_gap_window_audit_requires_one_exact_endpoint_effect() {
        let error = parse_arguments(
            [
                "--rlog",
                "one.rlog",
                "--all-abilities",
                "--proof-only",
                "--formula-proof-output",
                "proof.json",
                "--formula-gap-window-audit",
                "gaps.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect_err("a gap audit without one target effect must fail closed");

        assert!(
            error.contains(
                "requires exactly one --formula-target-effect or --formula-source-effect"
            )
        );
    }

    #[test]
    fn gap_window_filter_uses_strict_canonical_event_boundaries() {
        let mut filter = FormulaGapWindowFilter {
            metadata: FormulaGapWindowFilterMetadata {
                source: "gaps.json".to_owned(),
                source_sha256: "00".repeat(32),
                effect_id: 2_110_092,
                effect_locus: FormulaEffectLocus::Target,
                complete_gap_bounded_lifecycles: 1,
                audited_damage_events_while_active: 1,
                matched_damage_events: 0,
                matched_window_damage_memberships: 0,
                selection_policy: "test",
                formula_authority: false,
            },
            sessions_by_file: BTreeMap::from([(
                "one.rlog".to_owned(),
                FormulaGapWindowSessionFilter {
                    session_id: "session".to_owned(),
                    sealed_content_sha256: "11".repeat(32),
                    windows_by_target: BTreeMap::from([(
                        42,
                        vec![FormulaGapWindowAuditWindow {
                            target_entity_uuid: 42,
                            applied_envelope_sequence: 10,
                            applied_observed_micros: 1_000,
                            terminal_envelope_sequence: 20,
                            terminal_observed_micros: 2_000,
                            damage_events_while_active: 1,
                            gap_bounded: true,
                            controlled_counterfactual_pair_proven: false,
                            formula_authority: false,
                            effect_endpoint_damage_role: Some("damage_target".to_owned()),
                        }],
                    )]),
                },
            )]),
            matched_damage_events: 0,
            matched_window_damage_memberships: 0,
        };

        assert!(!filter.matches(Path::new("one.rlog"), "session", 42, 10, 1_000));
        assert!(filter.matches(Path::new("one.rlog"), "session", 42, 11, 1_000));
        assert!(!filter.matches(Path::new("one.rlog"), "session", 42, 20, 2_000));
        filter.finish().expect("one audited damage row matched");
        assert_eq!(filter.metadata.matched_damage_events, 1);
    }

    #[test]
    fn target_effect_selection_treats_expired_status_as_inactive() {
        let selected = [2_110_092].into_iter().collect();
        let active = [(
            ActiveStatusKey {
                effect_id: 2_110_092,
                instance_id: 1,
                source_entity_uuid: 42,
            },
            ActiveStatusMetadata {
                expires_at_observed_micros: Some(2_000),
                ..ActiveStatusMetadata::default()
            },
        )]
        .into_iter()
        .collect();

        assert!(has_active_selected_effect(Some(&active), &selected, 2_000));
        assert!(!has_active_selected_effect(Some(&active), &selected, 2_001));
        assert!(!has_active_selected_effect(None, &selected, 1_000));
    }

    #[test]
    fn compact_effect_ability_inventory_keeps_exact_ids_and_counts() {
        let mut effects = BTreeMap::new();
        let mut effect = EffectAccumulator {
            status_events: 31,
            source_damage_events_while_active: 7,
            direct_source_damage_events_while_active: 2,
            ..EffectAccumulator::default()
        };
        effect.abilities.insert(
            2207,
            AbilityAccumulator {
                events: 5,
                amount_sum: 12_345,
                ..AbilityAccumulator::default()
            },
        );
        effects.insert(2_207_252, effect);

        let inventory = effect_ability_inventory(&effects);
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].effect_id, 2_207_252);
        assert_eq!(inventory[0].status_events, 31);
        assert_eq!(inventory[0].abilities.len(), 1);
        assert_eq!(inventory[0].abilities[0].ability_id, 2207);
        assert_eq!(inventory[0].abilities[0].events, 5);
        assert_eq!(inventory[0].abilities[0].amount_sum, "12345");
    }

    #[test]
    fn proof_only_accepts_compact_effect_ability_inventory_output() {
        let args = parse_arguments(
            [
                "--rlog",
                "one.rlog",
                "--effect",
                "2207252",
                "--proof-only",
                "--effect-ability-inventory-output",
                "inventory.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("compact effect inventory is a valid proof-only output");

        assert!(args.proof_only);
        assert_eq!(
            args.effect_ability_inventory_output,
            Some(PathBuf::from("inventory.json"))
        );
    }

    #[test]
    fn formula_proof_rejects_damage_only_replay_inputs() {
        let error = ensure_formula_status_lifecycle(0)
            .expect_err("damage-only replay must not be accepted as complete formula evidence");
        assert!(error.to_string().contains("no status lifecycle events"));
        ensure_formula_status_lifecycle(1).expect("one retained lifecycle event is sufficient");
    }

    #[test]
    fn formula_packet_inputs_drop_outputs_and_decoder_generated_container_ordinals() {
        let packet = DamagePacketDetail {
            attacker_uuid: Some(17),
            top_summoner_uuid: Some(23),
            owner_id: Some(29),
            dead: Some(true),
            normal_value: Some(1_234),
            lucky_value: Some(5_678),
            skill_effect_uuid: Some(90),
            skill_effect_total_damage: Some(6_912),
            skill_effect_group_index: Some(11),
            skill_effect_component_index: Some(2),
            skill_effect_component_count: Some(7),
            hit_parts: vec![rlogs_events::DamageHitPart {
                part_id: Some(3),
                damage_value: Some(1_234),
                ..rlogs_events::DamageHitPart::default()
            }],
            ..DamagePacketDetail::default()
        };

        let retained: DamagePacketDetail = serde_json::from_str(
            &formula_packet_inputs(&packet).expect("serialize formula packet inputs"),
        )
        .expect("deserialize formula packet inputs");

        assert_eq!(retained.normal_value, None);
        assert_eq!(retained.dead, None);
        assert_eq!(retained.attacker_uuid, Some(17));
        assert_eq!(retained.top_summoner_uuid, Some(23));
        assert_eq!(retained.owner_id, Some(29));
        assert_eq!(retained.lucky_value, None);
        assert_eq!(retained.skill_effect_uuid, None);
        assert_eq!(retained.skill_effect_total_damage, None);
        assert_eq!(retained.skill_effect_group_index, None);
        assert_eq!(retained.hit_parts[0].damage_value, None);
        assert_eq!(retained.hit_parts[0].part_id, Some(3));
        assert_eq!(retained.skill_effect_component_index, None);
        assert_eq!(retained.skill_effect_component_count, None);
    }

    #[test]
    fn ratio_uses_ten_thousand_fixed_point_units() {
        let mut counts = BTreeMap::new();
        increment_ratio(&mut counts, 600, 10_000);
        let ratio = counts.get(&600).expect("ratio");
        assert_eq!(ratio.count, 1);
        assert_eq!(ratio.numerators, [600].into_iter().collect());
        assert_eq!(ratio.denominators, [10_000].into_iter().collect());
    }

    #[test]
    fn ratio_evidence_counts_distinct_state_values() {
        let mut counts = BTreeMap::new();
        increment_ratio(&mut counts, 300, 100);
        increment_ratio(&mut counts, 600, 200);
        let report = ratio_counts(counts);
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].basis_points_floor, 30_000);
        assert_eq!(report[0].count, 2);
        assert_eq!(report[0].distinct_numerators, 2);
        assert_eq!(report[0].distinct_denominators, 2);
        assert_eq!(report[0].numerator_examples, vec![300, 600]);
        assert_eq!(report[0].denominator_examples, vec![100, 200]);
    }

    #[test]
    fn hp_dependency_inventory_retains_every_observed_ability() {
        let mut unresolved = AbilityAccumulator {
            events: 1,
            ..AbilityAccumulator::default()
        };
        unresolved.event_state_ratios.events_with_source_max_hp = 1;
        increment_ratio(
            &mut unresolved
                .event_state_ratios
                .amount_to_source_max_hp_basis_points,
            37,
            100,
        );
        let abilities = [(123, unresolved)].into_iter().collect();
        let inventory = hp_shield_dependency_inventory(&abilities).unwrap();

        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].ability_id, 123);
        assert!(inventory[0].retained_in_canonical_timeline);
        assert!(!inventory[0].runtime_rdps_attribution_enabled);
        assert_eq!(
            inventory[0].evidence_status,
            StateDependencyEvidenceStatus::PacketStateObservedFormulaUnresolved
        );
    }

    #[test]
    fn hp_dependency_inventory_marks_cross_state_ratio_as_candidate_not_proof() {
        let mut candidate = AbilityAccumulator {
            events: 2,
            ..AbilityAccumulator::default()
        };
        candidate.event_state_ratios.events_with_source_max_hp = 2;
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_source_max_hp_basis_points,
            500,
            1_000,
        );
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_source_max_hp_basis_points,
            750,
            1_500,
        );
        let abilities = [(456, candidate)].into_iter().collect();
        let inventory = hp_shield_dependency_inventory(&abilities).unwrap();

        assert_eq!(
            inventory[0].evidence_status,
            StateDependencyEvidenceStatus::CrossStateFixedPointCandidate
        );
        assert_eq!(inventory[0].fixed_point_candidates.len(), 1);
        assert_eq!(
            inventory[0].fixed_point_candidates[0].strength,
            FixedPointCandidateStrength::CrossState
        );
        assert!(inventory[0].exact_formula_proof.is_none());
        assert!(!inventory[0].runtime_rdps_attribution_enabled);
    }

    #[test]
    fn hp_dependency_inventory_retains_event_only_target_missing_hp_as_consequence_risk() {
        let mut candidate = AbilityAccumulator {
            events: 2,
            ..AbilityAccumulator::default()
        };
        candidate.event_state_ratios.events_with_target_missing_hp = 2;
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_target_missing_hp_basis_points,
            500,
            500,
        );
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_target_missing_hp_basis_points,
            750,
            750,
        );
        let abilities = [(791, candidate)].into_iter().collect();
        let inventory = hp_shield_dependency_inventory(&abilities).unwrap();

        assert_eq!(
            inventory[0].evidence_status,
            StateDependencyEvidenceStatus::EventOrderTargetStateConsequenceObserved
        );
        assert_eq!(inventory[0].fixed_point_candidates.len(), 1);
        assert!(inventory[0].fixed_point_candidates[0].retained_for_evidence);
        assert!(inventory[0].fixed_point_candidates[0].post_hit_consequence_risk);
        assert!(!inventory[0].fixed_point_candidates[0].eligible_for_formula_investigation);
        assert_eq!(
            inventory[0].fixed_point_candidates[0].timing_assessment,
            FixedPointTimingAssessment::EventOrderTargetStatePostHitConsequenceRisk
        );
        assert!(inventory[0].retained_in_canonical_timeline);
    }

    #[test]
    fn hp_dependency_inventory_promotes_target_hp_ratio_when_packet_timings_match() {
        let mut candidate = AbilityAccumulator {
            events: 2,
            ..AbilityAccumulator::default()
        };
        for ratios in [
            &mut candidate.event_state_ratios,
            &mut candidate.wire_start_state_ratios,
        ] {
            ratios.events_with_target_missing_hp = 2;
            increment_ratio(
                &mut ratios.amount_to_target_missing_hp_basis_points,
                500,
                1_000,
            );
            increment_ratio(
                &mut ratios.amount_to_target_missing_hp_basis_points,
                750,
                1_500,
            );
        }
        let abilities = [(792, candidate)].into_iter().collect();
        let inventory = hp_shield_dependency_inventory(&abilities).unwrap();

        assert_eq!(
            inventory[0].evidence_status,
            StateDependencyEvidenceStatus::CrossStateFixedPointCandidate
        );
        assert_eq!(inventory[0].fixed_point_candidates.len(), 2);
        assert!(inventory[0].fixed_point_candidates.iter().all(|candidate| {
            candidate.matching_other_timing_candidate
                && candidate.eligible_for_formula_investigation
                && !candidate.post_hit_consequence_risk
                && candidate.timing_assessment
                    == FixedPointTimingAssessment::PresentAtBothPacketTimings
        }));
    }

    #[test]
    fn hp_dependency_inventory_does_not_promote_low_coverage_ratio_noise() {
        let mut candidate = AbilityAccumulator {
            events: 20,
            ..AbilityAccumulator::default()
        };
        candidate.event_state_ratios.events_with_target_max_hp = 20;
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_target_max_hp_basis_points,
            500,
            1_000,
        );
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_target_max_hp_basis_points,
            750,
            1_500,
        );
        let abilities = [(789, candidate)].into_iter().collect();
        let inventory = hp_shield_dependency_inventory(&abilities).unwrap();

        assert!(inventory[0].fixed_point_candidates.is_empty());
        assert_eq!(
            inventory[0].evidence_status,
            StateDependencyEvidenceStatus::PacketStateObservedFormulaUnresolved
        );
        assert!(inventory[0].retained_in_canonical_timeline);
    }

    #[test]
    fn hp_dependency_inventory_never_promotes_zero_damage_ratio() {
        let mut candidate = AbilityAccumulator {
            events: 4,
            ..AbilityAccumulator::default()
        };
        candidate.event_state_ratios.events_with_source_max_hp = 4;
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_source_max_hp_basis_points,
            0,
            1_000,
        );
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_source_max_hp_basis_points,
            0,
            1_500,
        );
        increment_ratio(
            &mut candidate
                .event_state_ratios
                .amount_to_source_max_hp_basis_points,
            0,
            2_000,
        );
        let abilities = [(790, candidate)].into_iter().collect();
        let inventory = hp_shield_dependency_inventory(&abilities).unwrap();

        assert!(inventory[0].fixed_point_candidates.is_empty());
        assert!(inventory[0].retained_in_canonical_timeline);
    }

    #[test]
    fn hp_dependency_inventory_summary_accounts_for_every_ability() {
        let abilities = [
            HpShieldDependencyCandidateReport {
                ability_id: 1,
                presentation_kind: None,
                presentation_resolution: None,
                recount_group_id: None,
                damage_events: 2,
                evidence_status:
                    StateDependencyEvidenceStatus::PacketStateObservedFormulaUnresolved,
                retained_in_canonical_timeline: true,
                runtime_rdps_attribution_enabled: false,
                exact_formula_proof: None,
                state_observations: Vec::new(),
                wire_to_event_state_transitions: Vec::new(),
                source_hp_transition_semantics: source_hp_transition_report(
                    &SourceHpTransitionAccumulator::default(),
                ),
                fixed_point_candidates: Vec::new(),
                interpretation: "test",
            },
            HpShieldDependencyCandidateReport {
                ability_id: 2,
                presentation_kind: None,
                presentation_resolution: None,
                recount_group_id: None,
                damage_events: 1,
                evidence_status: StateDependencyEvidenceStatus::CrossStateFixedPointCandidate,
                retained_in_canonical_timeline: true,
                runtime_rdps_attribution_enabled: false,
                exact_formula_proof: None,
                state_observations: Vec::new(),
                wire_to_event_state_transitions: Vec::new(),
                source_hp_transition_semantics: source_hp_transition_report(
                    &SourceHpTransitionAccumulator::default(),
                ),
                fixed_point_candidates: Vec::new(),
                interpretation: "test",
            },
        ];
        let summary = hp_shield_dependency_inventory_summary(&abilities);

        assert_eq!(summary.abilities, 2);
        assert_eq!(summary.retained_abilities, 2);
        assert_eq!(summary.cross_state_fixed_point_candidates, 1);
        assert_eq!(summary.packet_state_observed_formula_unresolved, 1);
        assert_eq!(summary.runtime_rdps_attribution_enabled, 0);
    }

    #[test]
    fn selected_effects_are_deduplicated_across_owner_and_direct_source() {
        let mut statuses = HashMap::new();
        statuses.insert(
            (1, 10),
            [(
                ActiveStatusKey {
                    effect_id: 2206270,
                    instance_id: 1,
                    source_entity_uuid: 10,
                },
                ActiveStatusMetadata::default(),
            )]
            .into_iter()
            .collect(),
        );
        statuses.insert(
            (1, 11),
            [(
                ActiveStatusKey {
                    effect_id: 2206270,
                    instance_id: 2,
                    source_entity_uuid: 10,
                },
                ActiveStatusMetadata::default(),
            )]
            .into_iter()
            .collect(),
        );
        let selected_effects = [2206270].into_iter().collect();
        assert_eq!(
            active_selected_effects(&statuses, 1, 10, Some(11), &selected_effects, 0),
            [2206270]
        );
    }

    #[test]
    fn consumed_status_only_closes_when_remaining_stack_count_is_zero() {
        let key = ActiveStatusKey {
            effect_id: 2_404_261,
            instance_id: 77,
            source_entity_uuid: 42,
        };
        let mut active = BTreeMap::new();
        update_active_status(
            &mut active,
            key,
            ActiveStatusMetadata {
                stacks: Some(3),
                level: Some(1),
                duration_millis: Some(10_000),
                origin_source_type_id: Some(1),
                origin_source_config_id: Some(2_404_260),
                last_observed_micros: 1_000_000,
                expires_at_observed_micros: None,
            },
            StatusState::Applied,
        );
        update_active_status(
            &mut active,
            key,
            ActiveStatusMetadata {
                stacks: Some(2),
                last_observed_micros: 2_000_000,
                ..ActiveStatusMetadata::default()
            },
            StatusState::Consumed,
        );
        let retained = active.get(&key).expect("two stacks remain active");
        assert_eq!(retained.stacks, Some(2));
        assert_eq!(retained.origin_source_config_id, Some(2_404_260));

        update_active_status(
            &mut active,
            key,
            ActiveStatusMetadata {
                stacks: Some(0),
                last_observed_micros: 3_000_000,
                ..ActiveStatusMetadata::default()
            },
            StatusState::Consumed,
        );
        assert!(active.is_empty());
    }

    #[test]
    fn consumed_pulse_refreshes_observed_window_without_becoming_permanent() {
        let key = ActiveStatusKey {
            effect_id: 2_110_143,
            instance_id: 709,
            source_entity_uuid: 42,
        };
        let mut active = BTreeMap::new();
        update_active_status(
            &mut active,
            key,
            ActiveStatusMetadata {
                stacks: Some(1),
                duration_millis: Some(1_000),
                last_observed_micros: 1_000_000,
                ..ActiveStatusMetadata::default()
            },
            StatusState::Applied,
        );
        update_active_status(
            &mut active,
            key,
            ActiveStatusMetadata {
                stacks: Some(1),
                duration_millis: Some(1_000),
                last_observed_micros: 1_500_000,
                ..ActiveStatusMetadata::default()
            },
            StatusState::Consumed,
        );

        assert_eq!(active_statuses_from_set(Some(&active), 2_400_000).len(), 1);
        assert!(active_statuses_from_set(Some(&active), 2_500_001).is_empty());
        assert_eq!(
            active.len(),
            1,
            "expiry filters evidence without deleting history"
        );
    }

    #[test]
    fn near_integer_multiple_retains_small_rounding_residuals() {
        let mut counts = BTreeMap::new();
        increment_near_integer_multiple(&mut counts, 2_481_311, 827_104);
        assert_eq!(counts.get(&(3, -1)), Some(&1));
        assert_eq!(counts.len(), 1);
    }

    #[test]
    fn missing_hp_and_physical_defense_are_formula_inputs() {
        let scalar = |attribute_id, value| StateScalar {
            attribute_id,
            raw_length: 0,
            raw_sha256: "sha256:test".to_owned(),
            raw_hex: Some(String::new()),
            integer_varint: Some(value),
            float32_little_endian: None,
            decoded_shield_list: None,
        };
        let state = StateSnapshot {
            current_hp: Some(scalar(CURRENT_HP_ATTRIBUTE_ID, 600)),
            max_hp_final: Some(scalar(MAX_HP_ATTRIBUTE_ID, 1_000)),
            physical_defense_final: Some(scalar(PHYSICAL_DEFENSE_ATTRIBUTE_ID, 250)),
            all_packet_attributes: vec![scalar(474, 10)],
            ..StateSnapshot::default()
        };
        assert_eq!(state_missing_hp(&state), Some(400));
        assert_eq!(state.all_packet_attributes[0].attribute_id, 474);
    }

    #[test]
    fn wire_to_event_hp_changes_are_retained_without_becoming_formula_proof() {
        let scalar = |attribute_id, value| StateScalar {
            attribute_id,
            raw_length: 0,
            raw_sha256: "sha256:test".to_owned(),
            raw_hex: Some(String::new()),
            integer_varint: Some(value),
            float32_little_endian: None,
            decoded_shield_list: None,
        };
        let source_start = StateSnapshot {
            current_hp: Some(scalar(CURRENT_HP_ATTRIBUTE_ID, 1_000)),
            max_hp_final: Some(scalar(MAX_HP_ATTRIBUTE_ID, 1_000)),
            ..StateSnapshot::default()
        };
        let source_end = StateSnapshot {
            current_hp: Some(scalar(CURRENT_HP_ATTRIBUTE_ID, 900)),
            max_hp_final: Some(scalar(MAX_HP_ATTRIBUTE_ID, 1_000)),
            ..StateSnapshot::default()
        };
        let target_start = StateSnapshot {
            current_hp: Some(scalar(CURRENT_HP_ATTRIBUTE_ID, 1_000)),
            max_hp_final: Some(scalar(MAX_HP_ATTRIBUTE_ID, 1_000)),
            ..StateSnapshot::default()
        };
        let target_end = StateSnapshot {
            current_hp: Some(scalar(CURRENT_HP_ATTRIBUTE_ID, 500)),
            max_hp_final: Some(scalar(MAX_HP_ATTRIBUTE_ID, 1_000)),
            ..StateSnapshot::default()
        };
        let mut transitions = BTreeMap::new();
        observe_state_transitions(
            &mut transitions,
            500,
            Some(500),
            Some(&source_start),
            &source_end,
            Some(&target_start),
            &target_end,
        );
        let reports = state_transition_reports(&transitions);
        let source = reports
            .iter()
            .find(|report| report.locus == "source_current_hp")
            .unwrap();
        let target = reports
            .iter()
            .find(|report| report.locus == "target_current_hp")
            .unwrap();

        assert_eq!(source.decreased_events, 1);
        assert_eq!(source.signed_change_examples[0].signed_change, -100);
        assert_eq!(
            source.amount_to_absolute_change_basis_points[0].basis_points_floor,
            50_000
        );
        assert!(!source.post_event_consequence_risk);
        assert_eq!(target.decreased_events, 1);
        assert_eq!(target.signed_change_examples[0].signed_change, -500);
        assert_eq!(
            target.amount_to_absolute_change_basis_points[0].basis_points_floor,
            10_000
        );
        assert!(target.post_event_consequence_risk);
    }

    #[test]
    fn joint_source_hp_semantics_preserve_hp_costs_and_max_hp_surface_changes() {
        let scalar = |attribute_id, value| StateScalar {
            attribute_id,
            raw_length: 0,
            raw_sha256: "sha256:test".to_owned(),
            raw_hex: Some(String::new()),
            integer_varint: Some(value),
            float32_little_endian: None,
            decoded_shield_list: None,
        };
        let snapshot = |current_hp, max_hp| StateSnapshot {
            current_hp: Some(scalar(CURRENT_HP_ATTRIBUTE_ID, current_hp)),
            max_hp_final: Some(scalar(MAX_HP_ATTRIBUTE_ID, max_hp)),
            ..StateSnapshot::default()
        };
        let surface_start = snapshot(878_721, 878_723);
        let surface_end = snapshot(852_940, 852_942);
        let hp_cost_start = snapshot(600_000, 900_000);
        let hp_cost_end = snapshot(575_000, 900_000);
        let mut accumulator = SourceHpTransitionAccumulator::default();

        observe_source_hp_transition(&mut accumulator, Some(&surface_start), &surface_end);
        observe_source_hp_transition(&mut accumulator, Some(&hp_cost_start), &hp_cost_end);
        let report = source_hp_transition_report(&accumulator);

        assert!(report.hp_dependent_events_retained);
        assert_eq!(report.events_with_current_and_max_hp_at_both_timings, 2);
        assert!(report.semantic_counts.iter().any(|entry| {
            entry.semantic
                == SourceHpTransitionSemantic::CurrentAndMaxHpChangedSameDeltaPreservingMissingHp
                && entry.count == 1
        }));
        assert!(report.semantic_counts.iter().any(|entry| {
            entry.semantic == SourceHpTransitionSemantic::CurrentHpChangedMaxHpStable
                && entry.count == 1
        }));
        assert!(report.signed_change_examples.iter().any(|entry| {
            entry.current_hp_change == -25_781
                && entry.max_hp_change == -25_781
                && entry.missing_hp_change == 0
        }));
        assert!(report.signed_change_examples.iter().any(|entry| {
            entry.current_hp_change == -25_000
                && entry.max_hp_change == 0
                && entry.missing_hp_change == 25_000
        }));
    }

    #[test]
    fn status_signatures_retain_every_observed_effect() {
        let statuses = [
            ActiveStatusEvidence {
                effect_id: 55417,
                instance_id: Some(1),
                source_entity_uuid: Some(10),
                stacks: None,
                level: None,
                duration_millis: None,
                origin_source_type_id: None,
                origin_source_config_id: None,
                last_observed_micros: 0,
                expires_at_observed_micros: None,
            },
            ActiveStatusEvidence {
                effect_id: 683115,
                instance_id: Some(2),
                source_entity_uuid: Some(20),
                stacks: None,
                level: None,
                duration_millis: None,
                origin_source_type_id: None,
                origin_source_config_id: None,
                last_observed_micros: 0,
                expires_at_observed_micros: None,
            },
        ];
        let mut counts = BTreeMap::new();
        increment_status_signature(&mut counts, &statuses);
        let report = status_signature_report(counts);
        assert_eq!(report.unique_signatures, 1);
        assert_eq!(report.observations, 1);
        assert_eq!(report.top_signatures[0].effect_ids, vec![55417, 683115]);
        assert_eq!(report.top_signatures[0].count, 1);
    }

    #[test]
    fn compact_examples_keep_hp_while_lossless_rlog_retains_unknown_attributes() {
        let scalar = |attribute_id, value| StateScalar {
            attribute_id,
            raw_length: 0,
            raw_sha256: "sha256:test".to_owned(),
            raw_hex: Some(String::new()),
            integer_varint: Some(value),
            float32_little_endian: None,
            decoded_shield_list: None,
        };
        let values = [
            (
                CURRENT_HP_ATTRIBUTE_ID,
                scalar(CURRENT_HP_ATTRIBUTE_ID, 600),
            ),
            (474, scalar(474, 10)),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        let compact = state_snapshot(Some(&values), false);
        assert_eq!(state_integer(&compact.current_hp), Some(600));
        assert!(compact.all_packet_attributes.is_empty());
        let complete = state_snapshot(Some(&values), true);
        assert_eq!(complete.all_packet_attributes.len(), 2);
    }

    #[test]
    fn shield_list_is_retained_as_state_dependent_formula_input() {
        let raw_value = vec![
            10, 17, 8, 190, 3, 16, 102, 24, 225, 152, 36, 32, 136, 247, 25, 40, 173, 235, 54, 10,
            17, 8, 209, 3, 16, 12, 24, 136, 147, 2, 32, 224, 193, 1, 40, 151, 192, 18,
        ];
        let preserved = preserve_attribute(&rlogs_events::EntityAttribute {
            attribute_id: SHIELD_LIST_ATTRIBUTE_ID,
            decoded: None,
            raw_value,
        });
        let values = [(SHIELD_LIST_ATTRIBUTE_ID, preserved)]
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let snapshot = state_snapshot(Some(&values), false);

        assert_eq!(snapshot.current_shield_total, Some(628_201));
        assert_eq!(
            snapshot
                .current_shields
                .as_ref()
                .map(|shields| shields.shields.len()),
            Some(2)
        );

        let origins = [(
            446,
            ShieldOriginRecord {
                effect_id: 2201561,
                source_entity_uuid: Some(257187840640),
                origin_source_type_id: Some(1),
                origin_source_config_id: Some(2201560),
            },
        )]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        let evidence = shield_origin_evidence(&snapshot, Some(&origins));
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].shield_uuid, Some(446));
        assert_eq!(evidence[0].effect_id, Some(2201561));
        assert_eq!(evidence[0].source_entity_uuid, Some(257187840640));
        assert_eq!(evidence[1].shield_uuid, Some(465));
        assert_eq!(evidence[1].effect_id, None);
    }

    #[test]
    fn shield_provider_summary_keeps_self_external_and_unresolved_evidence() {
        let shields = [
            ShieldOriginEvidence {
                shield_uuid: Some(1),
                shield_type: Some(12),
                current_value: Some(100),
                effect_id: Some(2201561),
                source_entity_uuid: Some(10),
                origin_source_type_id: Some(1),
                origin_source_config_id: Some(2201560),
            },
            ShieldOriginEvidence {
                shield_uuid: Some(2),
                shield_type: None,
                current_value: Some(50),
                effect_id: Some(2404271),
                source_entity_uuid: Some(20),
                origin_source_type_id: Some(1),
                origin_source_config_id: Some(2404270),
            },
            ShieldOriginEvidence {
                shield_uuid: Some(3),
                shield_type: Some(102),
                current_value: Some(200),
                effect_id: None,
                source_entity_uuid: None,
                origin_source_type_id: None,
                origin_source_config_id: None,
            },
        ];
        let mut observations = BTreeMap::new();
        observe_shield_providers(&mut observations, 10, 1_000, &shields);
        let reports = shield_provider_reports(observations);

        assert_eq!(reports.len(), 3);
        let external = reports
            .iter()
            .find(|report| report.provider_relation == ShieldProviderRelation::ExternallyProvided)
            .expect("external shield");
        assert_eq!(external.effect_id, Some(2404271));
        assert_eq!(external.damage_events, 1);
        assert_eq!(external.current_value_sum, "50");
        assert_eq!(external.provider_entity_uuids, vec![20]);
        assert!(reports.iter().any(|report| {
            report.provider_relation == ShieldProviderRelation::SelfProvided
                && report.effect_id == Some(2201561)
        }));
        assert!(reports.iter().any(|report| {
            report.provider_relation == ShieldProviderRelation::UnresolvedProvider
                && report.effect_id.is_none()
        }));
    }

    #[test]
    fn bash_pursuit_caps_the_shield_input_before_the_three_times_multiplier() {
        let formula =
            evaluate_bash_pursuit_formula(818_254, 1_424_416, Some(6_750_595), 6_750_595, 10, &[])
                .expect("formula inputs");
        assert_eq!(formula.current_shield_input_cap, 1_227_381);
        assert_eq!(formula.capped_current_shield_input, 1_227_381);
        assert_eq!(formula.formula_base_before_later_multipliers, 4_500_397);
        assert_eq!(
            formula.normal_to_formula_base_basis_points_floor,
            Some(14_999)
        );
    }

    #[test]
    fn bash_pursuit_preserves_external_shield_with_zero_marginal_damage_at_cap() {
        let external = ShieldOriginEvidence {
            shield_uuid: Some(1541),
            shield_type: None,
            current_value: Some(57_051),
            effect_id: Some(2_404_271),
            source_entity_uuid: Some(20),
            origin_source_type_id: Some(1),
            origin_source_config_id: Some(2_404_270),
        };
        let formula = evaluate_bash_pursuit_formula(
            791_085,
            1_367_944,
            Some(6_526_448),
            6_526_448,
            10,
            &[external],
        )
        .expect("formula inputs");
        let counterfactual = &formula.external_provider_counterfactuals[0];
        assert_eq!(counterfactual.provider_current_shield, Some(57_051));
        assert_eq!(counterfactual.provider_removed_formula_base_delta, Some(0));
        assert_eq!(counterfactual.zero_due_to_existing_shield_cap, Some(true));
    }

    #[test]
    fn bash_pursuit_proves_zero_external_marginal_from_non_provider_cap_lower_bound() {
        let self_shield = ShieldOriginEvidence {
            shield_uuid: Some(1),
            shield_type: Some(12),
            current_value: Some(1_500_000),
            effect_id: Some(11),
            source_entity_uuid: Some(10),
            origin_source_type_id: Some(1),
            origin_source_config_id: Some(10),
        };
        let external_without_current_value = ShieldOriginEvidence {
            shield_uuid: Some(2),
            shield_type: Some(11),
            current_value: None,
            effect_id: Some(22),
            source_entity_uuid: Some(20),
            origin_source_type_id: Some(1),
            origin_source_config_id: Some(21),
        };
        let formula = evaluate_bash_pursuit_formula(
            1_000_000,
            1_500_000,
            Some(5_500_000),
            5_500_000,
            10,
            &[self_shield, external_without_current_value],
        )
        .expect("formula inputs");
        let counterfactual = &formula.external_provider_counterfactuals[0];

        assert_eq!(counterfactual.provider_current_shield, None);
        assert_eq!(
            counterfactual.known_non_provider_current_shield_lower_bound,
            1_500_000
        );
        assert_eq!(
            counterfactual.capped_current_shield_without_provider,
            Some(1_500_000)
        );
        assert_eq!(counterfactual.provider_removed_formula_base_delta, Some(0));
        assert_eq!(counterfactual.zero_due_to_existing_shield_cap, Some(true));
        assert!(counterfactual.zero_proof_basis.is_some());
    }

    #[test]
    fn bash_pursuit_retains_unknown_external_shield_before_cap_without_guessing() {
        let self_shield = ShieldOriginEvidence {
            shield_uuid: Some(1),
            shield_type: Some(12),
            current_value: Some(1_000_000),
            effect_id: Some(11),
            source_entity_uuid: Some(10),
            origin_source_type_id: Some(1),
            origin_source_config_id: Some(10),
        };
        let external_without_current_value = ShieldOriginEvidence {
            shield_uuid: Some(2),
            shield_type: Some(11),
            current_value: None,
            effect_id: Some(22),
            source_entity_uuid: Some(20),
            origin_source_type_id: Some(1),
            origin_source_config_id: Some(21),
        };
        let formula = evaluate_bash_pursuit_formula(
            1_000_000,
            1_000_000,
            Some(4_000_000),
            4_000_000,
            10,
            &[self_shield, external_without_current_value],
        )
        .expect("formula inputs");
        let counterfactual = &formula.external_provider_counterfactuals[0];

        assert_eq!(counterfactual.provider_current_shield, None);
        assert_eq!(
            counterfactual.known_non_provider_current_shield_lower_bound,
            1_000_000
        );
        assert_eq!(counterfactual.provider_removed_formula_base_delta, None);
        assert_eq!(counterfactual.zero_due_to_existing_shield_cap, None);
        assert_eq!(counterfactual.zero_proof_basis, None);
    }

    #[test]
    fn bash_pursuit_keeps_positive_external_hp_scaled_damage_before_the_cap() {
        let external = ShieldOriginEvidence {
            shield_uuid: Some(2),
            shield_type: Some(12),
            current_value: Some(200_000),
            effect_id: Some(99),
            source_entity_uuid: Some(20),
            origin_source_type_id: Some(1),
            origin_source_config_id: Some(98),
        };
        let formula = evaluate_bash_pursuit_formula(
            1_000_000,
            1_000_000,
            Some(4_000_000),
            4_000_000,
            10,
            &[external],
        )
        .expect("formula inputs");
        let counterfactual = &formula.external_provider_counterfactuals[0];
        assert_eq!(
            counterfactual.current_shield_without_provider,
            Some(800_000)
        );
        assert_eq!(
            counterfactual.provider_removed_formula_base_delta,
            Some(600_000)
        );
        assert_eq!(counterfactual.zero_due_to_existing_shield_cap, Some(false));
    }

    #[test]
    fn judgment_pursuit_uses_event_order_max_hp_when_that_is_the_exact_packet_state() {
        let proof = evaluate_judgment_pursuit_formula(2_481_311, Some(801_323), Some(827_104))
            .expect("exact event-order MaxHP proof");

        assert_eq!(proof.inferred_calculation_time_max_hp, 827_104);
        assert_eq!(proof.exact_recomputed_amount, 2_481_311);
        assert!(proof.exact_amount_match);
        assert!(!proof.matches_wire_start_max_hp);
        assert!(proof.matches_event_order_max_hp);
    }

    #[test]
    fn judgment_pursuit_uses_wire_start_max_hp_when_notification_order_is_later() {
        let proof = evaluate_judgment_pursuit_formula(2_737_001, Some(912_334), Some(930_294))
            .expect("exact wire-start MaxHP proof");

        assert_eq!(proof.inferred_calculation_time_max_hp, 912_334);
        assert_eq!(proof.exact_recomputed_amount, 2_737_001);
        assert!(proof.exact_amount_match);
        assert!(proof.matches_wire_start_max_hp);
        assert!(!proof.matches_event_order_max_hp);
    }

    #[test]
    fn judgment_pursuit_does_not_promote_an_unmatched_hp_state() {
        assert!(
            evaluate_judgment_pursuit_formula(2_737_001, Some(900_000), Some(930_294)).is_none()
        );
    }

    #[test]
    fn bash_pursuit_exactly_infers_the_unique_transient_shield_between_packet_states() {
        let snapshot = infer_transient_bash_pursuit_snapshot(
            MAX_HP_TOTAL_ATTRIBUTE_ID,
            816_466,
            875_020,
            Some(1_335_566),
            5_887_315,
            3,
            2,
            1_224_699,
        )
        .expect("unique transient snapshot");

        assert_eq!(
            snapshot.max_hp_basis_attribute_id,
            MAX_HP_TOTAL_ATTRIBUTE_ID
        );
        assert_eq!(snapshot.inferred_current_shield_total, 1_036_137);
        assert_eq!(snapshot.wire_start_to_inferred_shield_delta, 161_117);
        assert_eq!(snapshot.inferred_to_event_order_shield_delta, Some(299_429));
        assert_eq!(
            snapshot.inferred_formula_base_before_later_multiplier,
            3_924_877
        );
        assert_eq!(snapshot.exact_recomputed_amount, 5_887_315);
        assert!(snapshot.exact_amount_match);
        assert!(!snapshot.provider_attribution_allowed);
    }

    #[test]
    fn transient_shield_bounds_preserve_every_feasible_per_instance_allocation() {
        let wire = ShieldListSnapshot {
            shields: vec![
                ShieldInstanceSnapshot {
                    uuid: Some(314),
                    shield_type: Some(102),
                    current_value: Some(621_884),
                    initial_value: Some(454_193),
                    max_value: Some(909_382),
                },
                ShieldInstanceSnapshot {
                    uuid: Some(983),
                    shield_type: Some(12),
                    current_value: Some(253_136),
                    initial_value: None,
                    max_value: Some(426_184),
                },
            ],
        };
        let event = ShieldListSnapshot {
            shields: vec![
                ShieldInstanceSnapshot {
                    uuid: Some(314),
                    shield_type: Some(102),
                    current_value: Some(909_382),
                    initial_value: Some(454_193),
                    max_value: Some(909_382),
                },
                ShieldInstanceSnapshot {
                    uuid: Some(983),
                    shield_type: Some(12),
                    current_value: Some(426_184),
                    initial_value: None,
                    max_value: Some(426_184),
                },
            ],
        };

        let bounds = transient_shield_instance_bounds(Some(&wire), Some(&event), 1_036_137);

        assert_eq!(bounds.len(), 2);
        assert_eq!(bounds[0].shield_uuid, Some(314));
        assert_eq!(bounds[0].inferred_current_value_min, 621_884);
        assert_eq!(bounds[0].inferred_current_value_max, 783_001);
        assert_eq!(bounds[0].inferred_delta_from_wire_min, 0);
        assert_eq!(bounds[0].inferred_delta_from_wire_max, 161_117);
        assert_eq!(bounds[1].shield_uuid, Some(983));
        assert_eq!(bounds[1].inferred_current_value_min, 253_136);
        assert_eq!(bounds[1].inferred_current_value_max, 414_253);
        assert_eq!(bounds[1].inferred_delta_from_wire_min, 0);
        assert_eq!(bounds[1].inferred_delta_from_wire_max, 161_117);
    }

    #[test]
    fn transient_shield_owner_resolves_when_every_ambiguous_instance_has_one_provider() {
        let bounds = vec![
            ShieldInstanceTransientBounds {
                shield_uuid: Some(314),
                shield_type: Some(102),
                observed_wire_start_current_value: 621_884,
                observed_event_order_current_value: 909_382,
                inferred_current_value_min: 621_884,
                inferred_current_value_max: 783_001,
                inferred_delta_from_wire_min: 0,
                inferred_delta_from_wire_max: 161_117,
            },
            ShieldInstanceTransientBounds {
                shield_uuid: Some(983),
                shield_type: Some(12),
                observed_wire_start_current_value: 253_136,
                observed_event_order_current_value: 426_184,
                inferred_current_value_min: 253_136,
                inferred_current_value_max: 414_253,
                inferred_delta_from_wire_min: 0,
                inferred_delta_from_wire_max: 161_117,
            },
        ];
        let provider = 257_187_840_640;
        let origins = vec![
            ShieldOriginEvidence {
                shield_uuid: Some(314),
                shield_type: Some(102),
                current_value: Some(621_884),
                effect_id: Some(50_024),
                source_entity_uuid: Some(provider),
                origin_source_type_id: None,
                origin_source_config_id: None,
            },
            ShieldOriginEvidence {
                shield_uuid: Some(983),
                shield_type: Some(12),
                current_value: Some(253_136),
                effect_id: Some(2_201_561),
                source_entity_uuid: Some(provider),
                origin_source_type_id: None,
                origin_source_config_id: Some(2_201_560),
            },
        ];

        let ownership =
            resolve_transient_shield_provider_ownership(provider, &bounds, &origins, &[]);

        assert!(ownership.provider_ownership_complete);
        assert_eq!(ownership.candidate_provider_entity_uuids, vec![provider]);
        assert_eq!(ownership.resolved_provider_entity_uuid, Some(provider));
        assert_eq!(
            ownership.resolved_provider_relation,
            Some(ShieldProviderRelation::SelfProvided)
        );
        assert_eq!(ownership.external_rdps_transfer_required, Some(false));
    }

    #[test]
    fn transient_shield_owner_remains_unresolved_across_multiple_providers() {
        let bounds = vec![
            ShieldInstanceTransientBounds {
                shield_uuid: Some(314),
                shield_type: Some(102),
                observed_wire_start_current_value: 10,
                observed_event_order_current_value: 20,
                inferred_current_value_min: 10,
                inferred_current_value_max: 20,
                inferred_delta_from_wire_min: 0,
                inferred_delta_from_wire_max: 10,
            },
            ShieldInstanceTransientBounds {
                shield_uuid: Some(983),
                shield_type: Some(12),
                observed_wire_start_current_value: 30,
                observed_event_order_current_value: 40,
                inferred_current_value_min: 30,
                inferred_current_value_max: 40,
                inferred_delta_from_wire_min: 0,
                inferred_delta_from_wire_max: 10,
            },
        ];
        let origins = vec![
            ShieldOriginEvidence {
                shield_uuid: Some(314),
                shield_type: Some(102),
                current_value: Some(10),
                effect_id: Some(50_024),
                source_entity_uuid: Some(10),
                origin_source_type_id: None,
                origin_source_config_id: None,
            },
            ShieldOriginEvidence {
                shield_uuid: Some(983),
                shield_type: Some(12),
                current_value: Some(30),
                effect_id: Some(2_201_561),
                source_entity_uuid: Some(20),
                origin_source_type_id: None,
                origin_source_config_id: Some(2_201_560),
            },
        ];

        let ownership = resolve_transient_shield_provider_ownership(10, &bounds, &origins, &[]);

        assert!(ownership.provider_ownership_complete);
        assert_eq!(ownership.candidate_provider_entity_uuids, vec![10, 20]);
        assert_eq!(ownership.resolved_provider_entity_uuid, None);
        assert_eq!(ownership.resolved_provider_relation, None);
        assert_eq!(ownership.external_rdps_transfer_required, None);
    }

    #[test]
    fn bash_pursuit_does_not_infer_a_transient_state_outside_observed_bounds() {
        let snapshot = infer_transient_bash_pursuit_snapshot(
            MAX_HP_TOTAL_ATTRIBUTE_ID,
            816_466,
            875_020,
            Some(900_000),
            5_887_315,
            3,
            2,
            1_224_699,
        );
        assert!(snapshot.is_none());
    }

    #[test]
    fn bash_pursuit_does_not_infer_a_reduced_damage_hit_as_source_state() {
        assert!(
            infer_transient_bash_pursuit_snapshot(
                MAX_HP_TOTAL_ATTRIBUTE_ID,
                1_000,
                1_000,
                Some(1_200),
                3_000,
                3,
                2,
                1_500,
            )
            .is_none()
        );
    }

    #[test]
    fn status_provider_attributes_use_wire_start_and_never_future_backfill() {
        let status = |provider_entity_uuid| ActiveStatusEvidence {
            effect_id: 2206241,
            instance_id: None,
            source_entity_uuid: Some(provider_entity_uuid),
            stacks: Some(1),
            level: Some(1),
            duration_millis: None,
            origin_source_type_id: Some(1),
            origin_source_config_id: Some(2206240),
            last_observed_micros: 1,
            expires_at_observed_micros: None,
        };
        let scalar = |value| StateScalar {
            attribute_id: 10030,
            raw_length: 0,
            raw_sha256: String::new(),
            raw_hex: None,
            integer_varint: Some(value),
            float32_little_endian: None,
            decoded_shield_list: None,
        };
        let wire_start = HashMap::from([((1, 7), BTreeMap::from([(10030, scalar(100))]))]);
        let current = HashMap::from([((1, 7), BTreeMap::from([(10030, scalar(200))]))]);
        let snapshots = status_provider_attribute_snapshots(
            1,
            Some(&[status(7)]),
            Some(&[status(7), status(8)]),
            &wire_start,
            &current,
        );
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].provider_entity_uuid, 7);
        assert_eq!(
            snapshots[0]
                .state_at_wire_message_start
                .as_ref()
                .unwrap()
                .all_packet_attributes[0]
                .integer_varint,
            Some(100)
        );
        assert_eq!(snapshots[1].provider_entity_uuid, 8);
        assert!(snapshots[1].state_at_wire_message_start.is_none());
    }

    fn formula_sample(
        sequence: u64,
        observed_micros: u64,
        hit_event_id: i32,
        amount: i64,
    ) -> FormulaCohortSample {
        FormulaCohortSample {
            rlog: "test.rlog".to_owned(),
            session_id: "session".to_owned(),
            run_ordinal: 1,
            sequence,
            observed_micros,
            wire_capture_sequence: Some(1),
            scene_id: Some(1),
            source_entity_uuid: 10,
            direct_source_entity_uuid: None,
            target_entity_uuid: 20,
            source_actor_identity: None,
            direct_source_actor_identity: None,
            target_actor_identity: None,
            source_position_at_wire_message_start: None,
            direct_source_position_at_wire_message_start: None,
            target_position_at_wire_message_start: None,
            ability_id: COEFFICIENT_PAIR_ABILITY_ID,
            passive_uuid: None,
            hit_event_id: Some(hit_event_id),
            amount,
            actual_amount: Some(amount),
            normal_value: Some(amount),
            lucky_value: None,
            hp_loss: Some(amount),
            shield_loss: Some(0),
            damage_source: Some(1),
            damage_type: Some(1),
            critical: Some(false),
            lucky: Some(false),
            packet: DamagePacketDetail {
                normal_value: Some(amount),
                property: Some(7),
                damage_mode: Some(1),
                ..DamagePacketDetail::default()
            },
            source_attribute_state_id: 0,
            direct_source_attribute_state_id: None,
            target_attribute_state_id: 0,
            source_status_state_id: 0,
            target_status_state_id: 0,
            status_provider_attribute_states: Vec::new(),
        }
    }

    #[test]
    fn position_at_wire_start_never_backfills_a_same_message_first_observation() {
        let key = (1, 20);
        let current = CompactFormulaPositionObservation {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            facing_radians: None,
            sequence: 10,
            observed_micros: 100,
            wire_capture_sequence: Some(5),
        };
        let positions = [(key, current)].into_iter().collect();
        let at_wire_start = [(key, None)].into_iter().collect();

        assert!(position_at_wire_message_start(&at_wire_start, &positions, key).is_none());
    }

    #[test]
    fn position_at_wire_start_uses_the_prior_observation_not_the_same_message_update() {
        let key = (1, 20);
        let prior = CompactFormulaPositionObservation {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            facing_radians: None,
            sequence: 9,
            observed_micros: 90,
            wire_capture_sequence: Some(4),
        };
        let current = CompactFormulaPositionObservation {
            x: 4.0,
            y: 5.0,
            z: 6.0,
            facing_radians: Some(0.5),
            sequence: 10,
            observed_micros: 100,
            wire_capture_sequence: Some(5),
        };
        let positions = [(key, current)].into_iter().collect();
        let at_wire_start = [(key, Some(prior))].into_iter().collect();

        let observed = position_at_wire_message_start(&at_wire_start, &positions, key)
            .expect("prior observation");
        assert_eq!(observed.x, 1.0);
        assert_eq!(observed.sequence, 9);
        assert_eq!(observed.wire_capture_sequence, Some(4));
    }

    fn formula_test_cohort(samples: Vec<FormulaCohortSample>) -> FormulaCohortAccumulator {
        FormulaCohortAccumulator {
            attribute_states: vec![Vec::new()],
            status_states: vec![Vec::new()],
            samples,
            ..FormulaCohortAccumulator::default()
        }
    }

    #[test]
    fn identical_formula_inputs_expose_divergent_server_outputs() {
        let cohort = formula_test_cohort(vec![
            formula_sample(1, 100, COEFFICIENT_PAIR_LOW_EVENT_ID, 1_000),
            formula_sample(2, 200, COEFFICIENT_PAIR_LOW_EVENT_ID, 1_001),
        ]);

        let report = formula_input_determinism_report(&cohort).expect("determinism report");

        assert!(report.proof_authority);
        assert_eq!(report.input_groups, 1);
        assert_eq!(report.repeated_input_groups, 1);
        assert_eq!(report.divergent_repeated_groups, 1);
        assert_eq!(report.divergent_repeated_samples, 2);
        assert_eq!(report.examples.len(), 1);
        assert_eq!(report.examples[0].outcomes.len(), 2);
    }

    #[test]
    fn message_scope_report_preserves_invariant_targets_and_divergent_wires() {
        let mut first_target = formula_sample(1, 100, COEFFICIENT_PAIR_LOW_EVENT_ID, 1_000);
        first_target.wire_capture_sequence = Some(10);
        let mut second_target = first_target.clone();
        second_target.sequence = 2;
        second_target.target_entity_uuid = 21;

        let mut later_first_target = first_target.clone();
        later_first_target.sequence = 3;
        later_first_target.observed_micros = 200;
        later_first_target.wire_capture_sequence = Some(11);
        later_first_target.amount = 1_100;
        later_first_target.actual_amount = Some(1_100);
        later_first_target.normal_value = Some(1_100);
        later_first_target.hp_loss = Some(1_100);
        later_first_target.packet.normal_value = Some(1_100);
        let mut later_second_target = later_first_target.clone();
        later_second_target.sequence = 4;
        later_second_target.target_entity_uuid = 21;

        let mut first_alternate = first_target.clone();
        first_alternate.sequence = 5;
        first_alternate.packet.property = Some(2);
        first_alternate.amount = 2_000;
        first_alternate.actual_amount = Some(2_000);
        first_alternate.normal_value = Some(2_000);
        first_alternate.hp_loss = Some(2_000);
        first_alternate.packet.normal_value = Some(2_000);
        let mut second_alternate = first_alternate.clone();
        second_alternate.sequence = 6;
        second_alternate.target_entity_uuid = 21;

        let mut later_first_alternate = first_alternate.clone();
        later_first_alternate.sequence = 7;
        later_first_alternate.observed_micros = 200;
        later_first_alternate.wire_capture_sequence = Some(11);
        later_first_alternate.amount = 2_200;
        later_first_alternate.actual_amount = Some(2_200);
        later_first_alternate.normal_value = Some(2_200);
        later_first_alternate.hp_loss = Some(2_200);
        later_first_alternate.packet.normal_value = Some(2_200);
        let mut later_second_alternate = later_first_alternate.clone();
        later_second_alternate.sequence = 8;
        later_second_alternate.target_entity_uuid = 21;

        let cohort = formula_test_cohort(vec![
            first_target,
            second_target,
            later_first_target,
            later_second_target,
            first_alternate,
            second_alternate,
            later_first_alternate,
            later_second_alternate,
        ]);
        let report = formula_message_scope_report(&cohort).expect("message report");

        assert!(!report.proof_authority);
        assert_eq!(report.wire_groups, 4);
        assert_eq!(report.multi_target_wire_groups, 4);
        assert_eq!(report.invariant_multi_target_wire_groups, 4);
        assert_eq!(report.divergent_multi_target_wire_groups, 0);
        assert_eq!(report.cross_wire_control_groups, 2);
        assert_eq!(report.divergent_cross_wire_control_groups, 2);
        assert_eq!(report.invariant_wires_in_divergent_control_groups, 4);
        assert_eq!(report.target_samples_in_divergent_control_groups, 8);
        assert_eq!(report.examples.len(), 2);
        assert_eq!(report.examples[0].wires.len(), 2);
        assert_eq!(report.shared_scalar.multi_signature_wire_pairs, 1);
        assert_eq!(report.shared_scalar.identity_ratio_wire_pairs, 0);
        assert_eq!(report.shared_scalar.changed_ratio_wire_pairs, 1);
        assert_eq!(report.shared_scalar.floor_interval_consistent_wire_pairs, 1);
        assert_eq!(
            report.shared_scalar.floor_interval_inconsistent_wire_pairs,
            0
        );
        assert_eq!(report.shared_scalar.maximum_signature_support, 2);
        assert_eq!(report.shared_scalar.examples.len(), 1);
        assert!(report.shared_scalar.examples[0].floor_interval_consistent);
    }

    #[test]
    fn coefficient_pair_proves_exact_ratio_only_when_packet_inputs_also_match() {
        let cohort = formula_test_cohort(vec![
            formula_sample(1, 100, COEFFICIENT_PAIR_HIGH_EVENT_ID, 6_250),
            formula_sample(2, 100, COEFFICIENT_PAIR_LOW_EVENT_ID, 1_000),
        ]);

        let report = formula_coefficient_pair_report(&cohort).expect("coefficient report");

        assert_eq!(report.candidate_groups, 1);
        assert_eq!(report.candidate_comparisons, 1);
        assert_eq!(report.exact_proportional_comparisons, 1);
        assert_eq!(report.packet_inputs_equal_comparisons, 1);
        assert!(report.formula_stage_authority);
        assert_eq!(report.residual_min, Some(0));
        assert_eq!(report.residual_max, Some(0));
    }

    #[test]
    fn coefficient_pair_retains_event_specific_packet_differences() {
        let mut high = formula_sample(1, 100, COEFFICIENT_PAIR_HIGH_EVENT_ID, 6_250);
        high.packet.owner_stage = Some(1);
        let low = formula_sample(2, 100, COEFFICIENT_PAIR_LOW_EVENT_ID, 1_000);
        let cohort = formula_test_cohort(vec![high, low]);

        let report = formula_coefficient_pair_report(&cohort).expect("coefficient report");

        assert_eq!(report.exact_proportional_comparisons, 1);
        assert_eq!(report.packet_inputs_equal_comparisons, 0);
        assert!(!report.formula_stage_authority);
        assert_ne!(
            report.examples[0].high_event_packet_inputs,
            report.examples[0].low_event_packet_inputs
        );
    }

    #[test]
    fn fixed_point_factor_interval_inverts_one_floor_stage() {
        assert_eq!(
            fixed_point_factor_interval(15_000, 10_000),
            Some((15_000, 15_000))
        );
        assert_eq!(
            fixed_point_factor_interval(2_400, 1_600),
            Some((15_000, 15_006))
        );
        assert_eq!(
            fixed_point_factor_interval(2_401, 1_600),
            Some((15_007, 15_012))
        );
    }

    #[test]
    fn post_coefficient_stage_intersects_same_target_event_factor_intervals() {
        let cohort = FormulaCohortAccumulator {
            attribute_states: vec![vec![CompactFormulaAttribute {
                attribute_id: PHYSICAL_ATTACK_ATTRIBUTE_ID,
                value: 2_000,
            }]],
            status_states: vec![Vec::new()],
            samples: vec![
                formula_sample(1, 100, COEFFICIENT_PAIR_HIGH_EVENT_ID, 15_000),
                formula_sample(2, 100, COEFFICIENT_PAIR_LOW_EVENT_ID, 2_400),
            ],
            ..FormulaCohortAccumulator::default()
        };

        let report = formula_post_coefficient_stage_report(&cohort);

        assert!(!report.proof_authority);
        assert_eq!(report.samples_with_source_attack, 2);
        assert_eq!(report.paired_groups_with_source_attack, 1);
        assert_eq!(report.paired_integer_factor_interval_consistent_groups, 1);
        assert_eq!(report.paired_integer_factor_interval_inconsistent_groups, 0);
        assert_eq!(report.paired_exact_integer_factor_groups, 1);
        assert_eq!(report.factor_intervals.len(), 1);
        assert_eq!(
            report.factor_intervals[0].minimum_factor_basis_points,
            15_000
        );
        assert_eq!(
            report.factor_intervals[0].maximum_factor_basis_points,
            15_000
        );
    }

    #[test]
    fn post_coefficient_stage_preserves_incompatible_same_target_pairs() {
        let cohort = FormulaCohortAccumulator {
            attribute_states: vec![vec![CompactFormulaAttribute {
                attribute_id: PHYSICAL_ATTACK_ATTRIBUTE_ID,
                value: 2_000,
            }]],
            status_states: vec![Vec::new()],
            samples: vec![
                formula_sample(1, 100, COEFFICIENT_PAIR_HIGH_EVENT_ID, 15_000),
                formula_sample(2, 100, COEFFICIENT_PAIR_LOW_EVENT_ID, 2_401),
            ],
            ..FormulaCohortAccumulator::default()
        };

        let report = formula_post_coefficient_stage_report(&cohort);

        assert_eq!(report.paired_integer_factor_interval_consistent_groups, 0);
        assert_eq!(report.paired_integer_factor_interval_inconsistent_groups, 1);
        assert!(report.factor_intervals.is_empty());
        assert_eq!(report.examples.len(), 1);
        assert!(
            report.examples[0]
                .shared_factor_interval_minimum_basis_points
                .is_none()
        );
    }

    #[test]
    fn post_coefficient_stage_is_explicitly_not_applicable_without_ability_2352() {
        let mut sample = formula_sample(1, 100, COEFFICIENT_PAIR_LOW_EVENT_ID, 1_000);
        sample.ability_id = 2_233;
        let report = formula_post_coefficient_stage_report(&formula_test_cohort(vec![sample]));

        assert!(!report.applicable);
        assert!(report.not_applicable_reason.is_some());
        assert_eq!(report.samples_with_source_attack, 0);
    }

    #[test]
    fn season_target_input_proof_requires_exact_source_11450_target_11440_equality() {
        let mut matching = formula_sample(1, 100, COEFFICIENT_PAIR_LOW_EVENT_ID, 1_000);
        matching.target_attribute_state_id = 1;
        let matching_cohort = FormulaCohortAccumulator {
            attribute_states: vec![
                vec![CompactFormulaAttribute {
                    attribute_id: SEASON_TARGET_INPUT_ATTRIBUTE_ID,
                    value: 2_420,
                }],
                vec![CompactFormulaAttribute {
                    attribute_id: SEASON_STRENGTH_ATTRIBUTE_ID,
                    value: 2_420,
                }],
            ],
            status_states: vec![Vec::new()],
            samples: vec![matching],
            ..FormulaCohortAccumulator::default()
        };
        let matching_report = season_target_input_proof(&matching_cohort);
        assert!(matching_report.proof_authority);
        assert_eq!(matching_report.comparable_samples, 1);
        assert_eq!(matching_report.exact_match_samples, 1);
        assert_eq!(matching_report.mismatch_samples, 0);

        let mut mismatching = formula_sample(2, 200, COEFFICIENT_PAIR_LOW_EVENT_ID, 1_000);
        mismatching.target_attribute_state_id = 1;
        let mismatching_cohort = FormulaCohortAccumulator {
            attribute_states: vec![
                vec![CompactFormulaAttribute {
                    attribute_id: SEASON_TARGET_INPUT_ATTRIBUTE_ID,
                    value: 2_240,
                }],
                vec![CompactFormulaAttribute {
                    attribute_id: SEASON_STRENGTH_ATTRIBUTE_ID,
                    value: 2_420,
                }],
            ],
            status_states: vec![Vec::new()],
            samples: vec![mismatching],
            ..FormulaCohortAccumulator::default()
        };
        let mismatching_report = season_target_input_proof(&mismatching_cohort);
        assert!(!mismatching_report.proof_authority);
        assert_eq!(mismatching_report.mismatch_samples, 1);
        assert_eq!(mismatching_report.distinct_mismatches.len(), 1);
    }

    #[test]
    fn empty_controlled_scope_never_claims_formula_authority() {
        let report = controlled_scope_report_from_groups(BTreeMap::new(), true)
            .expect("empty controlled scope report");

        assert!(!report.proof_authority);
        assert_eq!(report.controlled_groups, 0);
        assert_eq!(report.controlled_samples, 0);
        assert!(report.transitions.is_empty());
    }
}
