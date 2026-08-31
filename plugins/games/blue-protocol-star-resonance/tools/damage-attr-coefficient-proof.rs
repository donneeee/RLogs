#![allow(
    clippy::enum_variant_names,
    clippy::filter_map_bool_then,
    clippy::too_many_arguments
)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorKind, CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, EvidenceSource,
    RunState, StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 11;
const DEFAULT_EXAMPLE_LIMIT: usize = 8;

type SourceRouteLookup = BTreeMap<(i64, i32, i32), String>;

#[derive(Debug)]
struct Arguments {
    game_build: String,
    packet_build: String,
    surface: PathBuf,
    decoded_table: PathBuf,
    route_proof: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    example_limit: usize,
    include_hp_scaling: bool,
    coefficient_families: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum CombatResultKind {
    Damage,
    Healing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum RowSelectionAuthority {
    /// The current-build DamageAttr lookup contains exactly one numeric row.
    UniqueDamageAttrLookup,
    /// The packet omitted `hit_event_id`, but the current-build table contains
    /// exactly one DamageAttr row for the packet's exact ability id.
    UniqueAbilityDamageAttrLookup,
    /// The packet omitted `hit_event_id`, but its canonical result kind left
    /// exactly one candidate after excluding families proven incompatible by
    /// exact hit-bearing packet rows from the same replay corpus.
    PacketResultKindFamilyExhaustion,
    /// An otherwise ambiguous lookup was selected by the packet's exact
    /// `damage_source` discriminant and a current-build route proof.
    PacketDamageSourceRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DamageIdSelection {
    damage_id: String,
    authority: RowSelectionAuthority,
}

#[derive(Debug, Clone)]
struct DamageRow {
    damage_id: String,
    damage_script: String,
    type_enum: i64,
    pve_damage_ratio: Vec<i64>,
    pve_fixed_parameter: Vec<i64>,
    /// The complete current-build decoded row. DamageScript fields are
    /// polymorphic, so preserving the row verbatim is safer than assigning
    /// universal meanings to every column here.
    semantic_row: Value,
}

impl DamageRow {
    fn family(&self) -> &str {
        if self.damage_script.is_empty() {
            "<missing>"
        } else {
            &self.damage_script
        }
    }

    fn supports_coefficient_comparison(&self, coefficient_families: &BTreeSet<String>) -> bool {
        coefficient_families.contains(&self.damage_script)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct WireKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone)]
struct Hit {
    result_kind: CombatResultKind,
    row_selection_authority: RowSelectionAuthority,
    rlog: String,
    session_id: String,
    sequence: u64,
    ability_id: i64,
    hit_event_id: i32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
    blocked: Option<bool>,
    periodic: Option<bool>,
    missed: Option<bool>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    normal_hit: Option<bool>,
    packet_position: Option<PositionKey>,
    hit_parts: Vec<HitPartKey>,
    hit_part_observations: Vec<HitPartObservation>,
    damage_weight: Option<PositionKey>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    skill_effect_uuid: Option<i64>,
    skill_effect_total_damage: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    run_ordinal: u32,
    source_attribute_fingerprint: u64,
    direct_source_attribute_fingerprint: u64,
    target_attribute_fingerprint: u64,
    source_hp_independent_attribute_fingerprint: u64,
    direct_source_hp_independent_attribute_fingerprint: u64,
    target_hp_independent_attribute_fingerprint: u64,
    source_full_attribute_fingerprint: u64,
    direct_source_full_attribute_fingerprint: u64,
    target_full_attribute_fingerprint: u64,
    source_status_fingerprint: u64,
    direct_source_status_fingerprint: u64,
    target_status_fingerprint: u64,
    source_hp_evidence: HpEvidenceKey,
    direct_source_hp_evidence: HpEvidenceKey,
    target_hp_evidence: HpEvidenceKey,
    row: DamageRow,
}

/// Borrowed, lossless view of the two canonical events backed by BPSR's
/// `DamageInfo` protobuf. Keeping one analysis path prevents healing and
/// shield formula families from drifting away from ordinary damage proof.
struct CombatResultRef<'a> {
    kind: CombatResultKind,
    source: &'a rlogs_events::EntityRef,
    direct_source: Option<&'a rlogs_events::EntityRef>,
    target: &'a rlogs_events::EntityRef,
    ability: Option<rlogs_events::AbilityId>,
    hit_event_id: Option<i32>,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    effective_healing: Option<i64>,
    critical: Option<bool>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
    blocked: Option<bool>,
    periodic: Option<bool>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    packet: &'a rlogs_events::DamagePacketDetail,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct HpEvidenceKey {
    authoritative_sequence: Option<u64>,
    authoritative_current_hp: Option<i64>,
    authoritative_max_hp: Option<i64>,
    subsequent_damage_events: u64,
    subsequent_hp_loss: i64,
    subsequent_healing_events: u64,
    subsequent_healing_amount: i64,
    subsequent_life_events: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct HpEvidenceTracker {
    key: HpEvidenceKey,
}

impl HpEvidenceTracker {
    fn observe_attributes(&mut self, sequence: u64, current_hp: Option<i64>, max_hp: Option<i64>) {
        if let Some(max_hp) = max_hp {
            self.key.authoritative_max_hp = Some(max_hp);
        }
        if let Some(current_hp) = current_hp {
            self.key.authoritative_sequence = Some(sequence);
            self.key.authoritative_current_hp = Some(current_hp);
            self.key.subsequent_damage_events = 0;
            self.key.subsequent_hp_loss = 0;
            self.key.subsequent_healing_events = 0;
            self.key.subsequent_healing_amount = 0;
            self.key.subsequent_life_events = 0;
        }
    }

    fn observe_damage(&mut self, hp_loss: Option<i64>) {
        self.key.subsequent_damage_events = self.key.subsequent_damage_events.saturating_add(1);
        if let Some(hp_loss) = hp_loss {
            self.key.subsequent_hp_loss = self.key.subsequent_hp_loss.saturating_add(hp_loss);
        }
    }

    fn observe_healing(&mut self, amount: i64) {
        self.key.subsequent_healing_events = self.key.subsequent_healing_events.saturating_add(1);
        self.key.subsequent_healing_amount =
            self.key.subsequent_healing_amount.saturating_add(amount);
    }

    fn observe_life(&mut self) {
        self.key.subsequent_life_events = self.key.subsequent_life_events.saturating_add(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HitPartKey {
    part_id: Option<i32>,
    position: Option<PositionKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct HitPartObservation {
    part_id: Option<i32>,
    position: Option<PositionKey>,
    damage_value: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct PositionKey {
    x_bits: Option<u32>,
    y_bits: Option<u32>,
    z_bits: Option<u32>,
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
        let mut hash = EMPTY_FINGERPRINT;
        for (key, value) in &self.active {
            for scalar in [
                key.effect_id,
                key.source_entity_uuid.unwrap_or(i64::MIN),
                i64::from(value.stacks.unwrap_or(u32::MAX)),
                i64::from(value.level.unwrap_or(i32::MIN)),
                i64::from(value.part_id.unwrap_or(i32::MIN + 1)),
                i64::from(value.count.unwrap_or(i32::MIN + 2)),
            ] {
                hash_scalar(&mut hash, scalar);
            }
        }
        hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PairKey {
    result_kind: CombatResultKind,
    damage_script: String,
    first_row_selection_authority: RowSelectionAuthority,
    second_row_selection_authority: RowSelectionAuthority,
    ability_id: i64,
    first_hit_event_id: i32,
    second_hit_event_id: i32,
    first_damage_id: String,
    second_damage_id: String,
}

#[derive(Debug, Default)]
struct PairAccumulator {
    paired_events: u64,
    equal_owner_level: u64,
    equal_owner_stage: u64,
    comparable_normal_values: u64,
    candidate_indexes: BTreeMap<usize, CandidateAccumulator>,
    normalized_ratio_basis_points: BTreeMap<i64, u64>,
    examples: Vec<PairExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RepeatedStateKey {
    result_kind: CombatResultKind,
    damage_script: String,
    row_selection_authority: RowSelectionAuthority,
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
    blocked: Option<bool>,
    periodic: Option<bool>,
    missed: Option<bool>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    normal_hit: Option<bool>,
    packet_position: Option<PositionKey>,
    hit_parts: Vec<HitPartKey>,
    damage_weight: Option<PositionKey>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    source_full_attribute_fingerprint: u64,
    direct_source_full_attribute_fingerprint: u64,
    target_full_attribute_fingerprint: u64,
    source_status_fingerprint: u64,
    direct_source_status_fingerprint: u64,
    target_status_fingerprint: u64,
    source_hp_evidence: HpEvidenceKey,
    direct_source_hp_evidence: HpEvidenceKey,
    target_hp_evidence: HpEvidenceKey,
}

#[derive(Debug, Default)]
struct RepeatedStateAccumulator {
    events: u64,
    normal_values: BTreeMap<i64, u64>,
    example_sequences: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HpScalingFamilyKey {
    result_kind: CombatResultKind,
    damage_script: String,
    row_selection_authority: RowSelectionAuthority,
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
    blocked: Option<bool>,
    periodic: Option<bool>,
    missed: Option<bool>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    reported_critical: Option<bool>,
    type_flags: Option<i32>,
    normal_hit: Option<bool>,
    packet_position: Option<PositionKey>,
    hit_parts: Vec<HitPartKey>,
    damage_weight: Option<PositionKey>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    source_hp_independent_attribute_fingerprint: u64,
    direct_source_hp_independent_attribute_fingerprint: u64,
    target_hp_independent_attribute_fingerprint: u64,
    source_status_fingerprint: u64,
    direct_source_status_fingerprint: u64,
    target_status_fingerprint: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ResolvedHpState {
    current_hp: i64,
    max_hp: i64,
    missing_hp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HpScalingObservationKey {
    source_hp: Option<ResolvedHpState>,
    direct_source_hp: Option<ResolvedHpState>,
    target_hp: Option<ResolvedHpState>,
    normal_value: Option<i64>,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
}

#[derive(Debug, Default)]
struct HpScalingAccumulator {
    events: u64,
    observations: BTreeMap<HpScalingObservationKey, HpScalingObservationAccumulator>,
}

#[derive(Debug, Default)]
struct HpScalingObservationAccumulator {
    events: u64,
    example_sequences: Vec<u64>,
}

#[derive(Debug, Default)]
struct CandidateAccumulator {
    coefficient_pairs: BTreeSet<(i64, i64)>,
    exact_normal_proportions: u64,
    exact_amount_proportions: u64,
    exact_level_adjusted_normal_proportions: u64,
    integer_floor_compatible_normal_pairs: u64,
    level_outside_floor_compatible_normal_pairs: u64,
    level_inside_floor_compatible_normal_pairs: u64,
    normal_cross_residual_sum: u128,
    normal_cross_residual_max: u128,
    level_adjusted_cross_residual_sum: u128,
    level_adjusted_cross_residual_max: u128,
}

#[derive(Debug, Clone, Serialize)]
struct PairExample {
    rlog: String,
    session_id: String,
    wire_capture_sequence: u64,
    result_kind: CombatResultKind,
    first_row_selection_authority: RowSelectionAuthority,
    second_row_selection_authority: RowSelectionAuthority,
    first_sequence: u64,
    second_sequence: u64,
    ability_id: i64,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    first_hit_event_id: i32,
    second_hit_event_id: i32,
    first_damage_id: String,
    second_damage_id: String,
    first_amount: i64,
    second_amount: i64,
    first_actual_amount: Option<i64>,
    second_actual_amount: Option<i64>,
    first_hp_loss: Option<i64>,
    second_hp_loss: Option<i64>,
    first_shield_loss: Option<i64>,
    second_shield_loss: Option<i64>,
    first_skill_effect_total_damage: Option<i64>,
    second_skill_effect_total_damage: Option<i64>,
    first_hit_parts: Vec<HitPartObservation>,
    second_hit_parts: Vec<HitPartObservation>,
    first_normal_value: Option<i64>,
    second_normal_value: Option<i64>,
    first_pve_damage_ratio: Vec<i64>,
    second_pve_damage_ratio: Vec<i64>,
    first_selected_pve_fixed_parameter: Option<i64>,
    second_selected_pve_fixed_parameter: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    packet_build: String,
    policy: AuditPolicy,
    surface_source: Value,
    decoded_table_source: DecodedTableSource,
    route_proof_source: Value,
    exact_family_result_kind_authority: BTreeMap<String, ResultKindCounts>,
    observed_ability_result_kinds: Vec<ObservedAbilityResultKinds>,
    observed_damage_rows: Vec<ObservedDamageRow>,
    sessions: Vec<SessionSummary>,
    coverage: Coverage,
    sibling_pairs: Vec<PairReport>,
    repeated_state_variation: RepeatedStateVariation,
    hp_scaling_variation: HpScalingVariation,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_formula_authority: bool,
    comparison_scope: &'static str,
    row_identity_authority: &'static str,
    pve_damage_ratio_semantics_assumed: bool,
    pve_fixed_parameter_semantics_assumed: bool,
    integer_floor_semantics_tested: bool,
    pve_fixed_parameter_placements_tested: &'static [&'static str],
    coefficient_families: Vec<String>,
    nonstandard_damage_script_policy: &'static str,
    unresolved_packet_evidence_is_hidden: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct DecodedTableSource {
    path: String,
    decoded_rows: usize,
    surface_rows: usize,
    every_surface_row_semantically_joined: bool,
    surface_arrays_match_decoded_semantic_fields: bool,
}

#[derive(Debug, Serialize)]
struct ObservedDamageRow {
    damage_id: String,
    damage_script: String,
    type_enum: i64,
    packet_damage_results: u64,
    packet_healing_results: u64,
    /// Packet value fields retained for this exact damage row. These counts
    /// distinguish server-authored component executors (for example Lucky
    /// damage rows whose `amount` is the packet's `lucky_value`) from rows
    /// that must be reconstructed from a normal-value formula.
    packet_damage_value_shape: PacketValueShapeCounts,
    /// Source actor domains observed on packet damage results for this exact
    /// row. Unknown remains explicit; it is never treated as a player or
    /// monster by exclusion.
    packet_damage_source_actor_kinds: BTreeMap<String, u64>,
    /// Target actor domains observed on packet damage results for this exact
    /// row. This lets downstream rDPS gates distinguish outgoing party damage
    /// from incoming/TPS-only formulas without dropping either event class.
    packet_damage_target_actor_kinds: BTreeMap<String, u64>,
    semantic_row: Value,
}

#[derive(Debug, Default, Serialize)]
struct ResultKindCounts {
    damage: u64,
    healing: u64,
}

#[derive(Debug, Default, Clone, Serialize)]
struct PacketValueShapeCounts {
    results: u64,
    amount_zero: u64,
    amount_nonzero: u64,
    with_normal_value: u64,
    with_lucky_value: u64,
    with_both_values: u64,
    without_component_value: u64,
    zero_without_component_value: u64,
    nonzero_without_component_value: u64,
    amount_matches_normal_value: u64,
    amount_matches_lucky_value: u64,
    amount_matches_normal_plus_lucky: u64,
    lucky_flag_true: u64,
    causes_lucky_true: u64,
}

#[derive(Debug, Default)]
struct AbilityResultKindCounts {
    damage: u64,
    healing: u64,
    with_hit_event_id: u64,
    without_hit_event_id: u64,
}

#[derive(Debug, Serialize)]
struct ObservedAbilityResultKinds {
    ability_id: i64,
    packet_damage_results: u64,
    packet_healing_results: u64,
    results_with_hit_event_id: u64,
    results_without_hit_event_id: u64,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    rlog: String,
    session_id: String,
    client_build: String,
    damage_events: u64,
    healing_events: u64,
    wire_messages_with_combat_results: u64,
    combat_results_with_ability_and_hit: u64,
    mapped_combat_results: u64,
    mapped_by_unique_damage_attr_lookup: u64,
    mapped_without_hit_by_unique_ability_lookup: u64,
    mapped_without_hit_by_packet_result_kind_family_exhaustion: u64,
    mapped_by_packet_damage_source_route: u64,
    mapped_standard_coefficient_results: u64,
    mapped_nonstandard_family_results: u64,
    mapped_results_by_damage_script: BTreeMap<String, u64>,
    mapped_damage_results_by_damage_script: BTreeMap<String, u64>,
    mapped_healing_results_by_damage_script: BTreeMap<String, u64>,
    unresolved_combat_results: u64,
    unresolved_examples: Vec<UnresolvedResultExample>,
    sibling_pairs_compared: u64,
}

#[derive(Debug, Serialize)]
struct UnresolvedResultExample {
    reason: &'static str,
    sequence: u64,
    result_kind: CombatResultKind,
    ability_id: i64,
    hit_event_id: i32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    candidate_damage_ids: Vec<String>,
    selected_damage_id: Option<String>,
    packet: Value,
}

#[derive(Debug, Serialize)]
struct Coverage {
    sessions: usize,
    sibling_pair_families: usize,
    observed_damage_rows: usize,
    observed_damage_script_families: usize,
    mapped_results_by_damage_script: BTreeMap<String, u64>,
    mapped_damage_results_by_damage_script: BTreeMap<String, u64>,
    mapped_healing_results_by_damage_script: BTreeMap<String, u64>,
    paired_events: u64,
    comparable_normal_values: u64,
    families_with_an_exact_candidate_for_every_normal_pair: usize,
    families_with_an_exact_level_adjusted_candidate_for_every_normal_pair: usize,
    families_with_an_integer_floor_candidate_for_every_normal_pair: usize,
    families_with_a_level_outside_floor_candidate_for_every_normal_pair: usize,
    families_with_a_level_inside_floor_candidate_for_every_normal_pair: usize,
    repeated_causal_state_groups: usize,
    repeated_causal_state_groups_with_multiple_normal_values: usize,
    hp_scaling_formula_families_observed: usize,
    hp_scaling_formula_families_with_multiple_exact_states: usize,
    hp_scaling_exact_affine_candidates_with_three_or_more_states: usize,
}

#[derive(Debug, Serialize)]
struct RepeatedStateVariation {
    conclusion: &'static str,
    groups: Vec<RepeatedStateReport>,
}

#[derive(Debug, Serialize)]
struct RepeatedStateReport {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    result_kind: CombatResultKind,
    damage_script: String,
    row_selection_authority: RowSelectionAuthority,
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    source_hp_evidence: HpEvidenceKey,
    direct_source_hp_evidence: HpEvidenceKey,
    target_hp_evidence: HpEvidenceKey,
    events: u64,
    distinct_normal_values: usize,
    minimum_normal_value: i64,
    maximum_normal_value: i64,
    normal_values: Vec<ValueCount>,
    example_sequences: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct ValueCount {
    value: i64,
    events: u64,
}

#[derive(Debug, Serialize)]
struct HpScalingVariation {
    runtime_formula_authority: bool,
    conclusion: &'static str,
    formula_families: Vec<HpScalingFamilyReport>,
}

#[derive(Debug, Serialize)]
struct HpScalingFamilyReport {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    result_kind: CombatResultKind,
    damage_script: String,
    row_selection_authority: RowSelectionAuthority,
    ability_id: i64,
    hit_event_id: i32,
    damage_id: String,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
    events: u64,
    distinct_exact_hp_states: usize,
    observations: Vec<HpScalingObservationReport>,
    affine_candidates: Vec<HpAffineCandidate>,
}

#[derive(Debug, Serialize)]
struct HpScalingObservationReport {
    source_hp: Option<ResolvedHpState>,
    direct_source_hp: Option<ResolvedHpState>,
    target_hp: Option<ResolvedHpState>,
    normal_value: Option<i64>,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    events: u64,
    example_sequences: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum HpBasis {
    SourceCurrentHp,
    SourceMaxHp,
    SourceMissingHp,
    DirectSourceCurrentHp,
    DirectSourceMaxHp,
    DirectSourceMissingHp,
    TargetCurrentHp,
    TargetMaxHp,
    TargetMissingHp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum OutcomeMetric {
    NormalValue,
    Amount,
    ActualAmount,
    HpLoss,
    ShieldLoss,
}

#[derive(Debug, Serialize)]
struct HpAffineCandidate {
    basis: HpBasis,
    outcome: OutcomeMetric,
    distinct_basis_values: usize,
    distinct_points: usize,
    deterministic_at_each_basis_value: bool,
    exact_affine_fit: bool,
    sufficient_for_promotion_candidate: bool,
    slope_numerator: Option<String>,
    slope_denominator: Option<String>,
    intercept_numerator: Option<String>,
    intercept_denominator: Option<String>,
    zero_intercept: Option<bool>,
}

#[derive(Debug, Serialize)]
struct PairReport {
    result_kind: CombatResultKind,
    damage_script: String,
    first_row_selection_authority: RowSelectionAuthority,
    second_row_selection_authority: RowSelectionAuthority,
    ability_id: i64,
    first_hit_event_id: i32,
    second_hit_event_id: i32,
    first_damage_id: String,
    second_damage_id: String,
    paired_events: u64,
    equal_owner_level: u64,
    equal_owner_stage: u64,
    comparable_normal_values: u64,
    normalized_ratio_basis_points: Vec<RatioCount>,
    candidates: Vec<CandidateReport>,
    examples: Vec<PairExample>,
}

#[derive(Debug, Serialize)]
struct RatioCount {
    basis_points_floor: i64,
    events: u64,
}

#[derive(Debug, Serialize)]
struct CandidateReport {
    shared_array_index: usize,
    coefficient_pairs: Vec<CoefficientPair>,
    exact_normal_proportions: u64,
    exact_amount_proportions: u64,
    exact_level_adjusted_normal_proportions: u64,
    integer_floor_compatible_normal_pairs: u64,
    level_outside_floor_compatible_normal_pairs: u64,
    level_inside_floor_compatible_normal_pairs: u64,
    normal_cross_residual_sum: u128,
    normal_cross_residual_max: u128,
    level_adjusted_cross_residual_sum: u128,
    level_adjusted_cross_residual_max: u128,
    exact_for_every_comparable_normal_pair: bool,
    level_adjusted_exact_for_every_comparable_normal_pair: bool,
    integer_floor_compatible_for_every_comparable_normal_pair: bool,
    level_outside_floor_compatible_for_every_comparable_normal_pair: bool,
    level_inside_floor_compatible_for_every_comparable_normal_pair: bool,
}

#[derive(Debug, Serialize)]
struct CoefficientPair {
    first: i64,
    second: i64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("DamageAttr coefficient proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let surface: Value = serde_json::from_reader(BufReader::new(File::open(&args.surface)?))?;
    let decoded_table: Value =
        serde_json::from_reader(BufReader::new(File::open(&args.decoded_table)?))?;
    let route_proof: Value =
        serde_json::from_reader(BufReader::new(File::open(&args.route_proof)?))?;
    let source_routes = parse_source_routes(&route_proof, &args.game_build)?;
    let lookup = parse_lookup(&surface)?;
    let rows = parse_rows(&surface, &decoded_table)?;
    let ability_lookup = build_ability_lookup(&rows);
    let mut exact_family_result_kind_authority = BTreeMap::new();
    for rlog in &args.rlogs {
        scan_exact_family_result_kinds(
            rlog,
            &args.packet_build,
            &lookup,
            &source_routes,
            &rows,
            &mut exact_family_result_kind_authority,
        )?;
    }
    let mut accumulators = BTreeMap::<PairKey, PairAccumulator>::new();
    let mut repeated_states = BTreeMap::<RepeatedStateKey, RepeatedStateAccumulator>::new();
    let mut hp_scaling_families = BTreeMap::<HpScalingFamilyKey, HpScalingAccumulator>::new();
    let mut observed_damage_ids = BTreeSet::<String>::new();
    let mut observed_result_kinds = BTreeMap::<String, ResultKindCounts>::new();
    let mut observed_damage_value_shapes = BTreeMap::<String, PacketValueShapeCounts>::new();
    let mut observed_damage_source_actor_kinds = BTreeMap::<String, BTreeMap<String, u64>>::new();
    let mut observed_damage_target_actor_kinds = BTreeMap::<String, BTreeMap<String, u64>>::new();
    let mut observed_ability_result_kinds = BTreeMap::<i64, AbilityResultKindCounts>::new();
    let mut sessions = Vec::new();
    for rlog in &args.rlogs {
        sessions.push(read_session(
            rlog,
            &args.packet_build,
            &lookup,
            &ability_lookup,
            &exact_family_result_kind_authority,
            &source_routes,
            &rows,
            &args.coefficient_families,
            &mut accumulators,
            &mut repeated_states,
            &mut hp_scaling_families,
            &mut observed_damage_ids,
            &mut observed_result_kinds,
            &mut observed_damage_value_shapes,
            &mut observed_damage_source_actor_kinds,
            &mut observed_damage_target_actor_kinds,
            &mut observed_ability_result_kinds,
            args.example_limit,
            args.include_hp_scaling,
        )?);
    }

    let sibling_pairs = accumulators
        .into_iter()
        .map(|(key, accumulator)| pair_report(key, accumulator))
        .collect::<Vec<_>>();
    let paired_events = sibling_pairs.iter().map(|pair| pair.paired_events).sum();
    let comparable_normal_values = sibling_pairs
        .iter()
        .map(|pair| pair.comparable_normal_values)
        .sum();
    let families_with_an_exact_candidate_for_every_normal_pair = sibling_pairs
        .iter()
        .filter(|pair| {
            pair.comparable_normal_values > 0
                && pair
                    .candidates
                    .iter()
                    .any(|candidate| candidate.exact_for_every_comparable_normal_pair)
        })
        .count();
    let families_with_an_exact_level_adjusted_candidate_for_every_normal_pair = sibling_pairs
        .iter()
        .filter(|pair| {
            pair.comparable_normal_values > 0
                && pair.candidates.iter().any(|candidate| {
                    candidate.level_adjusted_exact_for_every_comparable_normal_pair
                })
        })
        .count();
    let families_with_an_integer_floor_candidate_for_every_normal_pair = sibling_pairs
        .iter()
        .filter(|pair| {
            pair.comparable_normal_values > 0
                && pair.candidates.iter().any(|candidate| {
                    candidate.integer_floor_compatible_for_every_comparable_normal_pair
                })
        })
        .count();
    let families_with_a_level_outside_floor_candidate_for_every_normal_pair = sibling_pairs
        .iter()
        .filter(|pair| {
            pair.comparable_normal_values > 0
                && pair.candidates.iter().any(|candidate| {
                    candidate.level_outside_floor_compatible_for_every_comparable_normal_pair
                })
        })
        .count();
    let families_with_a_level_inside_floor_candidate_for_every_normal_pair = sibling_pairs
        .iter()
        .filter(|pair| {
            pair.comparable_normal_values > 0
                && pair.candidates.iter().any(|candidate| {
                    candidate.level_inside_floor_compatible_for_every_comparable_normal_pair
                })
        })
        .count();
    let repeated_state_variation = repeated_state_variation(repeated_states);
    let repeated_causal_state_groups = repeated_state_variation.groups.len();
    let repeated_causal_state_groups_with_multiple_normal_values = repeated_state_variation
        .groups
        .iter()
        .filter(|group| group.distinct_normal_values > 1)
        .count();
    let hp_scaling_formula_families_observed = hp_scaling_families.len();
    let hp_scaling_variation = hp_scaling_variation(hp_scaling_families);
    let hp_scaling_formula_families_with_multiple_exact_states = hp_scaling_variation
        .formula_families
        .iter()
        .filter(|family| family.distinct_exact_hp_states > 1)
        .count();
    let hp_scaling_exact_affine_candidates_with_three_or_more_states = hp_scaling_variation
        .formula_families
        .iter()
        .flat_map(|family| &family.affine_candidates)
        .filter(|candidate| candidate.sufficient_for_promotion_candidate)
        .count();
    let observed_damage_rows = observed_damage_ids
        .into_iter()
        .filter_map(|damage_id| rows.get(&damage_id))
        .map(|row| ObservedDamageRow {
            damage_id: row.damage_id.clone(),
            damage_script: row.family().to_owned(),
            type_enum: row.type_enum,
            packet_damage_results: observed_result_kinds
                .get(&row.damage_id)
                .map_or(0, |counts| counts.damage),
            packet_healing_results: observed_result_kinds
                .get(&row.damage_id)
                .map_or(0, |counts| counts.healing),
            packet_damage_value_shape: observed_damage_value_shapes
                .remove(&row.damage_id)
                .unwrap_or_default(),
            packet_damage_source_actor_kinds: observed_damage_source_actor_kinds
                .remove(&row.damage_id)
                .unwrap_or_default(),
            packet_damage_target_actor_kinds: observed_damage_target_actor_kinds
                .remove(&row.damage_id)
                .unwrap_or_default(),
            semantic_row: row.semantic_row.clone(),
        })
        .collect::<Vec<_>>();
    let observed_ability_result_kinds = observed_ability_result_kinds
        .into_iter()
        .map(|(ability_id, counts)| ObservedAbilityResultKinds {
            ability_id,
            packet_damage_results: counts.damage,
            packet_healing_results: counts.healing,
            results_with_hit_event_id: counts.with_hit_event_id,
            results_without_hit_event_id: counts.without_hit_event_id,
        })
        .collect::<Vec<_>>();
    let observed_damage_row_count = observed_damage_rows.len();
    let mut mapped_results_by_damage_script = BTreeMap::<String, u64>::new();
    let mut mapped_damage_results_by_damage_script = BTreeMap::<String, u64>::new();
    let mut mapped_healing_results_by_damage_script = BTreeMap::<String, u64>::new();
    for session in &sessions {
        for (family, events) in &session.mapped_results_by_damage_script {
            *mapped_results_by_damage_script
                .entry(family.clone())
                .or_default() += events;
        }
        for (family, events) in &session.mapped_damage_results_by_damage_script {
            *mapped_damage_results_by_damage_script
                .entry(family.clone())
                .or_default() += events;
        }
        for (family, events) in &session.mapped_healing_results_by_damage_script {
            *mapped_healing_results_by_damage_script
                .entry(family.clone())
                .or_default() += events;
        }
    }
    let observed_damage_script_families = mapped_results_by_damage_script.len();
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-damage-attr-coefficient-proof",
        game_build: args.game_build,
        packet_build: args.packet_build,
        policy: AuditPolicy {
            runtime_formula_authority: false,
            comparison_scope: "different hit_event_id rows for the same combat-result kind, ability, source, direct source, target, packet flags, property, damage type/mode, wire message, authoritative HP snapshots, and intervening HP evidence",
            row_identity_authority: "current-build DamageAttr surface plus current-decoder canonical packet events",
            pve_damage_ratio_semantics_assumed: false,
            pve_fixed_parameter_semantics_assumed: false,
            integer_floor_semantics_tested: true,
            pve_fixed_parameter_placements_tested: &[
                "normal = floor(base * PVEDamageRadio / 10000) + selected_PVEFixedParameter",
                "normal = floor((base + selected_PVEFixedParameter) * PVEDamageRadio / 10000)",
            ],
            coefficient_families: args.coefficient_families.iter().cloned().collect(),
            nonstandard_damage_script_policy: "retain every mapped event and complete semantic row, group it by exact DamageScript family, and compare coefficients only for explicitly selected families",
            unresolved_packet_evidence_is_hidden: false,
            promotion_requirement: "an offset or array index becomes formula authority only after exact replay evidence distinguishes it from competing candidates",
        },
        surface_source: surface.get("source").cloned().unwrap_or(Value::Null),
        decoded_table_source: DecodedTableSource {
            path: args.decoded_table.display().to_string(),
            decoded_rows: decoded_table.as_object().map_or(0, serde_json::Map::len),
            surface_rows: rows.len(),
            every_surface_row_semantically_joined: true,
            surface_arrays_match_decoded_semantic_fields: true,
        },
        route_proof_source: serde_json::json!({
            "schema_version": route_proof.get("schema_version"),
            "game_build": route_proof.get("game_build"),
            "generated_by": route_proof.get("generated_by"),
            "summary": route_proof.get("summary"),
        }),
        exact_family_result_kind_authority,
        observed_ability_result_kinds,
        observed_damage_rows,
        coverage: Coverage {
            sessions: sessions.len(),
            sibling_pair_families: sibling_pairs.len(),
            observed_damage_rows: observed_damage_row_count,
            observed_damage_script_families,
            mapped_results_by_damage_script,
            mapped_damage_results_by_damage_script,
            mapped_healing_results_by_damage_script,
            paired_events,
            comparable_normal_values,
            families_with_an_exact_candidate_for_every_normal_pair,
            families_with_an_exact_level_adjusted_candidate_for_every_normal_pair,
            families_with_an_integer_floor_candidate_for_every_normal_pair,
            families_with_a_level_outside_floor_candidate_for_every_normal_pair,
            families_with_a_level_inside_floor_candidate_for_every_normal_pair,
            repeated_causal_state_groups,
            repeated_causal_state_groups_with_multiple_normal_values,
            hp_scaling_formula_families_observed,
            hp_scaling_formula_families_with_multiple_exact_states,
            hp_scaling_exact_affine_candidates_with_three_or_more_states,
        },
        sessions,
        sibling_pairs,
        repeated_state_variation,
        hp_scaling_variation,
    };
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn read_session(
    path: &Path,
    expected_game_build: &str,
    lookup: &BTreeMap<String, Vec<String>>,
    ability_lookup: &BTreeMap<i64, Vec<String>>,
    exact_family_result_kind_authority: &BTreeMap<String, ResultKindCounts>,
    source_routes: &SourceRouteLookup,
    rows: &BTreeMap<String, DamageRow>,
    coefficient_families: &BTreeSet<String>,
    accumulators: &mut BTreeMap<PairKey, PairAccumulator>,
    repeated_states: &mut BTreeMap<RepeatedStateKey, RepeatedStateAccumulator>,
    hp_scaling_families: &mut BTreeMap<HpScalingFamilyKey, HpScalingAccumulator>,
    observed_damage_ids: &mut BTreeSet<String>,
    observed_result_kinds: &mut BTreeMap<String, ResultKindCounts>,
    observed_damage_value_shapes: &mut BTreeMap<String, PacketValueShapeCounts>,
    observed_damage_source_actor_kinds: &mut BTreeMap<String, BTreeMap<String, u64>>,
    observed_damage_target_actor_kinds: &mut BTreeMap<String, BTreeMap<String, u64>>,
    observed_ability_result_kinds: &mut BTreeMap<i64, AbilityResultKindCounts>,
    example_limit: usize,
    include_hp_scaling: bool,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let client_build = reader.header().region.client_build.clone();
    if client_build != expected_game_build {
        return Err(format!(
            "{} contains client build {} but --build requires {}",
            file_label(path),
            client_build,
            expected_game_build
        )
        .into());
    }
    let mut active_wire = None::<WireKey>;
    let mut wire_hits = Vec::<Hit>::new();
    let mut session_id = None::<String>;
    let mut damage_events = 0_u64;
    let mut healing_events = 0_u64;
    let mut wire_messages_with_combat_results = 0_u64;
    let mut combat_results_with_ability_and_hit = 0_u64;
    let mut mapped_combat_results = 0_u64;
    let mut mapped_by_unique_damage_attr_lookup = 0_u64;
    let mut mapped_without_hit_by_unique_ability_lookup = 0_u64;
    let mut mapped_without_hit_by_packet_result_kind_family_exhaustion = 0_u64;
    let mut mapped_by_packet_damage_source_route = 0_u64;
    let mut mapped_standard_coefficient_results = 0_u64;
    let mut mapped_nonstandard_family_results = 0_u64;
    let mut mapped_results_by_damage_script = BTreeMap::<String, u64>::new();
    let mut mapped_damage_results_by_damage_script = BTreeMap::<String, u64>::new();
    let mut mapped_healing_results_by_damage_script = BTreeMap::<String, u64>::new();
    let mut unresolved_combat_results = 0_u64;
    let mut unresolved_examples = Vec::new();
    let mut sibling_pairs_compared = 0_u64;
    let mut current_run_ordinal = 0_u32;
    let mut attributes = HashMap::<(u32, i64), BTreeMap<i32, i64>>::new();
    let mut statuses = HashMap::<(u32, i64), StatusTracker>::new();
    let mut hp_evidence = HashMap::<(u32, i64), HpEvidenceTracker>::new();
    let mut actor_kinds = HashMap::<i64, ActorKind>::new();
    let mut owner_by_direct_entity = HashMap::<i64, i64>::new();

    while let Some(envelope) = reader.next_event()? {
        session_id.get_or_insert_with(|| envelope.session_id.clone());
        let wire = wire_key(&envelope.provenance.source);
        if wire != active_wire {
            if let Some(previous) = active_wire {
                if !wire_hits.is_empty() {
                    wire_messages_with_combat_results =
                        wire_messages_with_combat_results.saturating_add(1);
                    sibling_pairs_compared = sibling_pairs_compared.saturating_add(compare_wire(
                        previous,
                        &wire_hits,
                        coefficient_families,
                        accumulators,
                        example_limit,
                    ));
                }
            }
            active_wire = wire;
            wire_hits.clear();
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        let result = match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => {
                match state {
                    RunState::Entered => {
                        current_run_ordinal = current_run_ordinal.saturating_add(1);
                    }
                    RunState::Started if current_run_ordinal == 0 => {
                        current_run_ordinal = 1;
                    }
                    _ => {}
                }
                continue;
            }
            TimelineEventKind::Actor(actor) => {
                actor_kinds.insert(actor.actor.entity_uuid.0, actor.kind);
                continue;
            }
            TimelineEventKind::EntityAttributes(event) => {
                let snapshot = attributes
                    .entry((current_run_ordinal, event.actor.entity_uuid.0))
                    .or_default();
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    snapshot.clear();
                }
                let mut current_hp = None;
                let mut max_hp = None;
                for attribute in &event.attributes {
                    if let Some(value) = decode_attribute(attribute) {
                        snapshot.insert(attribute.attribute_id, value);
                        match attribute.attribute_id {
                            11_310 => current_hp = Some(value),
                            11_320 => max_hp = Some(value),
                            _ => {}
                        }
                    }
                }
                hp_evidence
                    .entry((current_run_ordinal, event.actor.entity_uuid.0))
                    .or_default()
                    .observe_attributes(envelope.sequence, current_hp, max_hp);
                continue;
            }
            TimelineEventKind::Status(status) => {
                statuses
                    .entry((current_run_ordinal, status.target.entity_uuid.0))
                    .or_default()
                    .observe(
                        StatusKey {
                            effect_id: status.effect.0,
                            source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                        },
                        StatusValue {
                            stacks: status.stacks,
                            level: status.level,
                            part_id: status.part_id,
                            count: status.count,
                        },
                        status.state,
                    );
                continue;
            }
            TimelineEventKind::Healing(healing) => {
                healing_events = healing_events.saturating_add(1);
                CombatResultRef {
                    kind: CombatResultKind::Healing,
                    source: &healing.source,
                    direct_source: healing.direct_source.as_ref(),
                    target: &healing.target,
                    ability: healing.ability,
                    hit_event_id: healing.hit_event_id,
                    amount: healing.amount,
                    actual_amount: healing.actual_amount,
                    hp_loss: healing.hp_loss,
                    shield_loss: healing.shield_loss,
                    effective_healing: healing.effective_amount,
                    critical: healing.critical,
                    lucky: Some(healing.packet.lucky_value.is_some_and(|value| value != 0)),
                    causes_lucky: None,
                    blocked: None,
                    periodic: healing.periodic,
                    damage_source: healing.damage_source,
                    damage_type: healing.damage_type,
                    packet: &healing.packet,
                }
            }
            TimelineEventKind::Life { actor, .. } => {
                hp_evidence
                    .entry((current_run_ordinal, actor.entity_uuid.0))
                    .or_default()
                    .observe_life();
                continue;
            }
            TimelineEventKind::Damage(damage) => {
                damage_events = damage_events.saturating_add(1);
                CombatResultRef {
                    kind: CombatResultKind::Damage,
                    source: &damage.source,
                    direct_source: damage.direct_source.as_ref(),
                    target: &damage.target,
                    ability: damage.ability,
                    hit_event_id: damage.hit_event_id,
                    amount: damage.amount,
                    actual_amount: damage.actual_amount,
                    hp_loss: damage.hp_loss,
                    shield_loss: damage.shield_loss,
                    effective_healing: None,
                    critical: damage.flags.critical,
                    lucky: damage.flags.lucky,
                    causes_lucky: damage.flags.causes_lucky,
                    blocked: damage.flags.blocked,
                    periodic: damage.flags.periodic,
                    damage_source: damage.damage_source,
                    damage_type: damage.damage_type,
                    packet: &damage.packet,
                }
            }
            _ => continue,
        };
        let (Some(wire), Some(ability)) = (wire, result.ability) else {
            observe_combat_result_hp_evidence(&mut hp_evidence, current_run_ordinal, &result);
            continue;
        };
        let ability_result_counts = observed_ability_result_kinds.entry(ability.0).or_default();
        match result.kind {
            CombatResultKind::Damage => {
                ability_result_counts.damage = ability_result_counts.damage.saturating_add(1);
            }
            CombatResultKind::Healing => {
                ability_result_counts.healing = ability_result_counts.healing.saturating_add(1);
            }
        }
        if result.hit_event_id.is_some() {
            ability_result_counts.with_hit_event_id =
                ability_result_counts.with_hit_event_id.saturating_add(1);
        } else {
            ability_result_counts.without_hit_event_id =
                ability_result_counts.without_hit_event_id.saturating_add(1);
        }
        let (hit_event_id, candidates, selection) = if let Some(hit_event_id) = result.hit_event_id
        {
            combat_results_with_ability_and_hit =
                combat_results_with_ability_and_hit.saturating_add(1);
            let lookup_key = format!("{}:{hit_event_id}", ability.0);
            let candidates = lookup.get(&lookup_key).cloned().unwrap_or_default();
            let selection = resolve_damage_id(
                &candidates,
                source_routes,
                ability.0,
                hit_event_id,
                result.damage_source,
            );
            (hit_event_id, candidates, selection)
        } else {
            let candidates = ability_lookup.get(&ability.0).cloned().unwrap_or_default();
            let selection = resolve_damage_id_without_hit(
                &candidates,
                rows,
                result.kind,
                exact_family_result_kind_authority,
            );
            let inferred_hit_event_id = selection
                .as_ref()
                .and_then(|selection| rows.get(&selection.damage_id))
                .and_then(|row| row.damage_id.parse::<i64>().ok())
                .map(|damage_id| damage_id.rem_euclid(100) as i32)
                .unwrap_or_default();
            (inferred_hit_event_id, candidates, selection)
        };
        let Some(selection) = selection else {
            unresolved_combat_results = unresolved_combat_results.saturating_add(1);
            if unresolved_examples.len() < example_limit {
                unresolved_examples.push(unresolved_result_example(
                    "no_exact_damage_attr_selection",
                    envelope.sequence,
                    &result,
                    ability.0,
                    hit_event_id,
                    candidates,
                    None,
                ));
            }
            observe_combat_result_hp_evidence(&mut hp_evidence, current_run_ordinal, &result);
            continue;
        };
        let Some(row) = rows.get(&selection.damage_id).cloned() else {
            unresolved_combat_results = unresolved_combat_results.saturating_add(1);
            if unresolved_examples.len() < example_limit {
                unresolved_examples.push(unresolved_result_example(
                    "selected_damage_attr_absent_from_joined_table",
                    envelope.sequence,
                    &result,
                    ability.0,
                    hit_event_id,
                    candidates,
                    Some(selection.damage_id),
                ));
            }
            observe_combat_result_hp_evidence(&mut hp_evidence, current_run_ordinal, &result);
            continue;
        };
        observed_damage_ids.insert(row.damage_id.clone());
        let result_kind_counts = observed_result_kinds
            .entry(row.damage_id.clone())
            .or_default();
        match result.kind {
            CombatResultKind::Damage => {
                result_kind_counts.damage = result_kind_counts.damage.saturating_add(1);
                observe_packet_damage_value_shape(
                    observed_damage_value_shapes
                        .entry(row.damage_id.clone())
                        .or_default(),
                    result.amount,
                    result.packet.normal_value,
                    result.packet.lucky_value,
                    result.lucky,
                    result.causes_lucky,
                );
                *mapped_damage_results_by_damage_script
                    .entry(row.family().to_owned())
                    .or_default() += 1;
            }
            CombatResultKind::Healing => {
                result_kind_counts.healing = result_kind_counts.healing.saturating_add(1);
                *mapped_healing_results_by_damage_script
                    .entry(row.family().to_owned())
                    .or_default() += 1;
            }
        }
        *mapped_results_by_damage_script
            .entry(row.family().to_owned())
            .or_default() += 1;
        if row.supports_coefficient_comparison(coefficient_families) {
            mapped_standard_coefficient_results =
                mapped_standard_coefficient_results.saturating_add(1);
        } else {
            mapped_nonstandard_family_results = mapped_nonstandard_family_results.saturating_add(1);
        }
        mapped_combat_results = mapped_combat_results.saturating_add(1);
        match selection.authority {
            RowSelectionAuthority::UniqueDamageAttrLookup => {
                mapped_by_unique_damage_attr_lookup =
                    mapped_by_unique_damage_attr_lookup.saturating_add(1);
            }
            RowSelectionAuthority::UniqueAbilityDamageAttrLookup => {
                mapped_without_hit_by_unique_ability_lookup =
                    mapped_without_hit_by_unique_ability_lookup.saturating_add(1);
            }
            RowSelectionAuthority::PacketResultKindFamilyExhaustion => {
                mapped_without_hit_by_packet_result_kind_family_exhaustion =
                    mapped_without_hit_by_packet_result_kind_family_exhaustion.saturating_add(1);
            }
            RowSelectionAuthority::PacketDamageSourceRoute => {
                mapped_by_packet_damage_source_route =
                    mapped_by_packet_damage_source_route.saturating_add(1);
            }
        }
        if active_wire != Some(wire) {
            active_wire = Some(wire);
        }
        let source_entity_uuid = result.source.entity_uuid.0;
        let direct_source_entity_uuid = result.direct_source.map(|source| source.entity_uuid.0);
        let target_entity_uuid = result.target.entity_uuid.0;
        if let Some(direct_source) =
            direct_source_entity_uuid.filter(|direct| *direct != source_entity_uuid)
        {
            owner_by_direct_entity
                .entry(direct_source)
                .or_insert(source_entity_uuid);
        }
        if result.kind == CombatResultKind::Damage {
            let resolved_source_entity_uuid = owner_by_direct_entity
                .get(&source_entity_uuid)
                .copied()
                .unwrap_or(source_entity_uuid);
            let source_kind = actor_kinds
                .get(&resolved_source_entity_uuid)
                .copied()
                .or_else(|| {
                    direct_source_entity_uuid.and_then(|direct| actor_kinds.get(&direct).copied())
                });
            let target_kind = actor_kinds.get(&target_entity_uuid).copied();
            increment_actor_kind(
                observed_damage_source_actor_kinds,
                &row.damage_id,
                source_kind,
            );
            increment_actor_kind(
                observed_damage_target_actor_kinds,
                &row.damage_id,
                target_kind,
            );
        }
        let hit = Hit {
            result_kind: result.kind,
            row_selection_authority: selection.authority,
            rlog: file_label(path),
            session_id: envelope.session_id.clone(),
            sequence: envelope.sequence,
            ability_id: ability.0,
            hit_event_id,
            source_entity_uuid,
            direct_source_entity_uuid,
            target_entity_uuid,
            raw_attacker_uuid: result.packet.attacker_uuid,
            raw_top_summoner_uuid: result.packet.top_summoner_uuid,
            raw_owner_id: result.packet.owner_id,
            amount: result.amount,
            actual_amount: result.actual_amount,
            hp_loss: result.hp_loss,
            shield_loss: result.shield_loss,
            normal_value: result.packet.normal_value,
            lucky_value: result.packet.lucky_value,
            owner_level: result.packet.owner_level,
            owner_stage: result.packet.owner_stage,
            critical: result.critical,
            lucky: result.lucky,
            causes_lucky: result.causes_lucky,
            blocked: result.blocked,
            periodic: result.periodic,
            missed: result.packet.missed,
            damage_source: result.damage_source,
            damage_type: result.damage_type,
            property: result.packet.property,
            damage_mode: result.packet.damage_mode,
            reported_critical: result.packet.reported_critical,
            type_flags: result.packet.type_flags,
            normal_hit: result.packet.normal_hit,
            packet_position: result.packet.position.as_ref().map(position_key),
            hit_parts: result
                .packet
                .hit_parts
                .iter()
                .map(|part| HitPartKey {
                    part_id: part.part_id,
                    position: part.position.as_ref().map(position_key),
                })
                .collect(),
            hit_part_observations: result
                .packet
                .hit_parts
                .iter()
                .map(|part| HitPartObservation {
                    part_id: part.part_id,
                    position: part.position.as_ref().map(position_key),
                    damage_value: part.damage_value,
                })
                .collect(),
            damage_weight: result.packet.damage_weight.as_ref().map(position_key),
            passive_uuid: result.packet.passive_uuid,
            rainbow: result.packet.rainbow,
            skill_effect_uuid: result.packet.skill_effect_uuid,
            skill_effect_total_damage: result.packet.skill_effect_total_damage,
            skill_effect_group_index: result.packet.skill_effect_group_index,
            skill_effect_component_index: result.packet.skill_effect_component_index,
            skill_effect_component_count: result.packet.skill_effect_component_count,
            run_ordinal: current_run_ordinal,
            source_attribute_fingerprint: attribute_fingerprint(
                attributes.get(&(current_run_ordinal, source_entity_uuid)),
                false,
            ),
            direct_source_attribute_fingerprint: attribute_fingerprint(
                direct_source_entity_uuid
                    .and_then(|uuid| attributes.get(&(current_run_ordinal, uuid))),
                false,
            ),
            target_attribute_fingerprint: attribute_fingerprint(
                attributes.get(&(current_run_ordinal, target_entity_uuid)),
                false,
            ),
            source_hp_independent_attribute_fingerprint: hp_independent_attribute_fingerprint(
                attributes.get(&(current_run_ordinal, source_entity_uuid)),
            ),
            direct_source_hp_independent_attribute_fingerprint:
                hp_independent_attribute_fingerprint(
                    direct_source_entity_uuid
                        .and_then(|uuid| attributes.get(&(current_run_ordinal, uuid))),
                ),
            target_hp_independent_attribute_fingerprint: hp_independent_attribute_fingerprint(
                attributes.get(&(current_run_ordinal, target_entity_uuid)),
            ),
            source_full_attribute_fingerprint: attribute_fingerprint(
                attributes.get(&(current_run_ordinal, source_entity_uuid)),
                true,
            ),
            direct_source_full_attribute_fingerprint: attribute_fingerprint(
                direct_source_entity_uuid
                    .and_then(|uuid| attributes.get(&(current_run_ordinal, uuid))),
                true,
            ),
            target_full_attribute_fingerprint: attribute_fingerprint(
                attributes.get(&(current_run_ordinal, target_entity_uuid)),
                true,
            ),
            source_status_fingerprint: status_fingerprint(
                statuses.get(&(current_run_ordinal, source_entity_uuid)),
            ),
            direct_source_status_fingerprint: status_fingerprint(
                direct_source_entity_uuid
                    .and_then(|uuid| statuses.get(&(current_run_ordinal, uuid))),
            ),
            target_status_fingerprint: status_fingerprint(
                statuses.get(&(current_run_ordinal, target_entity_uuid)),
            ),
            source_hp_evidence: hp_evidence_key(
                hp_evidence.get(&(current_run_ordinal, source_entity_uuid)),
            ),
            direct_source_hp_evidence: hp_evidence_key(
                direct_source_entity_uuid
                    .and_then(|uuid| hp_evidence.get(&(current_run_ordinal, uuid))),
            ),
            target_hp_evidence: hp_evidence_key(
                hp_evidence.get(&(current_run_ordinal, target_entity_uuid)),
            ),
            row,
        };
        observe_repeated_state(repeated_states, &hit, example_limit);
        if include_hp_scaling {
            observe_hp_scaling_family(hp_scaling_families, &hit, example_limit);
        }
        wire_hits.push(hit);
        observe_combat_result_hp_evidence(&mut hp_evidence, current_run_ordinal, &result);
    }
    if let Some(wire) = active_wire {
        if !wire_hits.is_empty() {
            wire_messages_with_combat_results = wire_messages_with_combat_results.saturating_add(1);
            sibling_pairs_compared = sibling_pairs_compared.saturating_add(compare_wire(
                wire,
                &wire_hits,
                coefficient_families,
                accumulators,
                example_limit,
            ));
        }
    }
    Ok(SessionSummary {
        rlog: file_label(path),
        session_id: session_id.unwrap_or_else(|| "unobserved".to_owned()),
        client_build,
        damage_events,
        healing_events,
        wire_messages_with_combat_results,
        combat_results_with_ability_and_hit,
        mapped_combat_results,
        mapped_by_unique_damage_attr_lookup,
        mapped_without_hit_by_unique_ability_lookup,
        mapped_without_hit_by_packet_result_kind_family_exhaustion,
        mapped_by_packet_damage_source_route,
        mapped_standard_coefficient_results,
        mapped_nonstandard_family_results,
        mapped_results_by_damage_script,
        mapped_damage_results_by_damage_script,
        mapped_healing_results_by_damage_script,
        unresolved_combat_results,
        unresolved_examples,
        sibling_pairs_compared,
    })
}

fn unresolved_result_example(
    reason: &'static str,
    sequence: u64,
    result: &CombatResultRef<'_>,
    ability_id: i64,
    hit_event_id: i32,
    candidate_damage_ids: Vec<String>,
    selected_damage_id: Option<String>,
) -> UnresolvedResultExample {
    UnresolvedResultExample {
        reason,
        sequence,
        result_kind: result.kind,
        ability_id,
        hit_event_id,
        source_entity_uuid: result.source.entity_uuid.0,
        direct_source_entity_uuid: result.direct_source.map(|source| source.entity_uuid.0),
        target_entity_uuid: result.target.entity_uuid.0,
        raw_attacker_uuid: result.packet.attacker_uuid,
        raw_top_summoner_uuid: result.packet.top_summoner_uuid,
        raw_owner_id: result.packet.owner_id,
        amount: result.amount,
        actual_amount: result.actual_amount,
        hp_loss: result.hp_loss,
        shield_loss: result.shield_loss,
        damage_source: result.damage_source,
        damage_type: result.damage_type,
        candidate_damage_ids,
        selected_damage_id,
        packet: serde_json::to_value(result.packet).unwrap_or(Value::Null),
    }
}

fn compare_wire(
    wire: WireKey,
    hits: &[Hit],
    coefficient_families: &BTreeSet<String>,
    accumulators: &mut BTreeMap<PairKey, PairAccumulator>,
    example_limit: usize,
) -> u64 {
    let mut compared = 0_u64;
    for first_index in 0..hits.len() {
        for second_index in (first_index + 1)..hits.len() {
            let mut first = &hits[first_index];
            let mut second = &hits[second_index];
            if !first
                .row
                .supports_coefficient_comparison(coefficient_families)
                || !second
                    .row
                    .supports_coefficient_comparison(coefficient_families)
                || first.row.damage_script != second.row.damage_script
                || first.result_kind != second.result_kind
                || first.ability_id != second.ability_id
                || first.hit_event_id == second.hit_event_id
                || first.source_entity_uuid != second.source_entity_uuid
                || first.direct_source_entity_uuid != second.direct_source_entity_uuid
                || first.target_entity_uuid != second.target_entity_uuid
                || first.raw_attacker_uuid != second.raw_attacker_uuid
                || first.raw_top_summoner_uuid != second.raw_top_summoner_uuid
                || first.raw_owner_id != second.raw_owner_id
                || first.owner_level != second.owner_level
                || first.owner_stage != second.owner_stage
                || first.critical != second.critical
                || first.lucky != second.lucky
                || first.causes_lucky != second.causes_lucky
                || first.blocked != second.blocked
                || first.periodic != second.periodic
                || first.missed != second.missed
                || first.damage_source != second.damage_source
                || first.damage_type != second.damage_type
                || first.property != second.property
                || first.damage_mode != second.damage_mode
                || first.reported_critical != second.reported_critical
                || first.type_flags != second.type_flags
                || first.normal_hit != second.normal_hit
                || first.packet_position != second.packet_position
                || first.hit_parts != second.hit_parts
                || first.damage_weight != second.damage_weight
                || first.passive_uuid != second.passive_uuid
                || first.rainbow != second.rainbow
                || first.skill_effect_uuid != second.skill_effect_uuid
                || first.skill_effect_group_index != second.skill_effect_group_index
                || first.skill_effect_component_count != second.skill_effect_component_count
                || first.source_attribute_fingerprint != second.source_attribute_fingerprint
                || first.direct_source_attribute_fingerprint
                    != second.direct_source_attribute_fingerprint
                || first.target_attribute_fingerprint != second.target_attribute_fingerprint
                || first.source_status_fingerprint != second.source_status_fingerprint
                || first.direct_source_status_fingerprint != second.direct_source_status_fingerprint
                || first.target_status_fingerprint != second.target_status_fingerprint
                || first.lucky_value.is_some()
                || second.lucky_value.is_some()
            {
                continue;
            }
            if first.hit_event_id > second.hit_event_id {
                std::mem::swap(&mut first, &mut second);
            }
            let key = PairKey {
                result_kind: first.result_kind,
                damage_script: first.row.damage_script.clone(),
                first_row_selection_authority: first.row_selection_authority,
                second_row_selection_authority: second.row_selection_authority,
                ability_id: first.ability_id,
                first_hit_event_id: first.hit_event_id,
                second_hit_event_id: second.hit_event_id,
                first_damage_id: first.row.damage_id.clone(),
                second_damage_id: second.row.damage_id.clone(),
            };
            let accumulator = accumulators.entry(key).or_default();
            accumulator.paired_events = accumulator.paired_events.saturating_add(1);
            accumulator.equal_owner_level = accumulator.equal_owner_level.saturating_add(1);
            accumulator.equal_owner_stage = accumulator.equal_owner_stage.saturating_add(1);
            let first_level =
                selected_pve_fixed_parameter(&first.row.pve_fixed_parameter, first.owner_level);
            let second_level =
                selected_pve_fixed_parameter(&second.row.pve_fixed_parameter, second.owner_level);
            if let (Some(first_normal), Some(second_normal)) =
                (first.normal_value, second.normal_value)
            {
                accumulator.comparable_normal_values =
                    accumulator.comparable_normal_values.saturating_add(1);
                let shared_len = first
                    .row
                    .pve_damage_ratio
                    .len()
                    .min(second.row.pve_damage_ratio.len());
                if let (Some(first_coefficient), Some(second_coefficient)) = (
                    first.row.pve_damage_ratio.first().copied(),
                    second.row.pve_damage_ratio.first().copied(),
                ) {
                    if first_coefficient != 0 && second_normal != 0 {
                        let numerator = i128::from(first_normal)
                            .saturating_mul(i128::from(second_coefficient))
                            .saturating_mul(10_000);
                        let denominator =
                            i128::from(second_normal).saturating_mul(i128::from(first_coefficient));
                        if denominator != 0 {
                            let ratio = numerator / denominator;
                            if let Ok(ratio) = i64::try_from(ratio) {
                                *accumulator
                                    .normalized_ratio_basis_points
                                    .entry(ratio)
                                    .or_default() += 1;
                            }
                        }
                    }
                }
                for index in 0..shared_len {
                    let first_coefficient = first.row.pve_damage_ratio[index];
                    let second_coefficient = second.row.pve_damage_ratio[index];
                    if first_coefficient == 0 || second_coefficient == 0 {
                        continue;
                    }
                    let candidate = accumulator.candidate_indexes.entry(index).or_default();
                    candidate
                        .coefficient_pairs
                        .insert((first_coefficient, second_coefficient));
                    let normal_residual = cross_residual(
                        first_normal,
                        second_normal,
                        first_coefficient,
                        second_coefficient,
                    );
                    observe_residual(
                        normal_residual,
                        &mut candidate.exact_normal_proportions,
                        &mut candidate.normal_cross_residual_sum,
                        &mut candidate.normal_cross_residual_max,
                    );
                    if floor_scaled_pair_has_shared_base(
                        first_normal,
                        second_normal,
                        first_coefficient,
                        second_coefficient,
                    ) {
                        candidate.integer_floor_compatible_normal_pairs = candidate
                            .integer_floor_compatible_normal_pairs
                            .saturating_add(1);
                    }
                    if cross_residual(
                        first.amount,
                        second.amount,
                        first_coefficient,
                        second_coefficient,
                    ) == 0
                    {
                        candidate.exact_amount_proportions =
                            candidate.exact_amount_proportions.saturating_add(1);
                    }
                    if let (Some(first_flat), Some(second_flat)) = (first_level, second_level) {
                        let adjusted = cross_residual(
                            first_normal.saturating_sub(first_flat),
                            second_normal.saturating_sub(second_flat),
                            first_coefficient,
                            second_coefficient,
                        );
                        observe_residual(
                            adjusted,
                            &mut candidate.exact_level_adjusted_normal_proportions,
                            &mut candidate.level_adjusted_cross_residual_sum,
                            &mut candidate.level_adjusted_cross_residual_max,
                        );
                        if floor_scaled_pair_with_output_offsets_has_shared_base(
                            first_normal,
                            second_normal,
                            first_coefficient,
                            second_coefficient,
                            first_flat,
                            second_flat,
                        ) {
                            candidate.level_outside_floor_compatible_normal_pairs = candidate
                                .level_outside_floor_compatible_normal_pairs
                                .saturating_add(1);
                        }
                        if floor_scaled_pair_with_input_offsets_has_shared_base(
                            first_normal,
                            second_normal,
                            first_coefficient,
                            second_coefficient,
                            first_flat,
                            second_flat,
                        ) {
                            candidate.level_inside_floor_compatible_normal_pairs = candidate
                                .level_inside_floor_compatible_normal_pairs
                                .saturating_add(1);
                        }
                    }
                }
            }
            if accumulator.examples.len() < example_limit {
                accumulator.examples.push(PairExample {
                    rlog: first.rlog.clone(),
                    session_id: first.session_id.clone(),
                    wire_capture_sequence: wire.capture_sequence,
                    result_kind: first.result_kind,
                    first_row_selection_authority: first.row_selection_authority,
                    second_row_selection_authority: second.row_selection_authority,
                    first_sequence: first.sequence,
                    second_sequence: second.sequence,
                    ability_id: first.ability_id,
                    source_entity_uuid: first.source_entity_uuid,
                    direct_source_entity_uuid: first.direct_source_entity_uuid,
                    target_entity_uuid: first.target_entity_uuid,
                    raw_attacker_uuid: first.raw_attacker_uuid,
                    raw_top_summoner_uuid: first.raw_top_summoner_uuid,
                    raw_owner_id: first.raw_owner_id,
                    owner_level: first.owner_level,
                    owner_stage: first.owner_stage,
                    first_hit_event_id: first.hit_event_id,
                    second_hit_event_id: second.hit_event_id,
                    first_damage_id: first.row.damage_id.clone(),
                    second_damage_id: second.row.damage_id.clone(),
                    first_amount: first.amount,
                    second_amount: second.amount,
                    first_actual_amount: first.actual_amount,
                    second_actual_amount: second.actual_amount,
                    first_hp_loss: first.hp_loss,
                    second_hp_loss: second.hp_loss,
                    first_shield_loss: first.shield_loss,
                    second_shield_loss: second.shield_loss,
                    first_skill_effect_total_damage: first.skill_effect_total_damage,
                    second_skill_effect_total_damage: second.skill_effect_total_damage,
                    first_hit_parts: first.hit_part_observations.clone(),
                    second_hit_parts: second.hit_part_observations.clone(),
                    first_normal_value: first.normal_value,
                    second_normal_value: second.normal_value,
                    first_pve_damage_ratio: first.row.pve_damage_ratio.clone(),
                    second_pve_damage_ratio: second.row.pve_damage_ratio.clone(),
                    first_selected_pve_fixed_parameter: first_level,
                    second_selected_pve_fixed_parameter: second_level,
                });
            }
            compared = compared.saturating_add(1);
        }
    }
    compared
}

fn pair_report(key: PairKey, accumulator: PairAccumulator) -> PairReport {
    let comparable = accumulator.comparable_normal_values;
    let mut normalized_ratio_basis_points = accumulator
        .normalized_ratio_basis_points
        .into_iter()
        .map(|(basis_points_floor, events)| RatioCount {
            basis_points_floor,
            events,
        })
        .collect::<Vec<_>>();
    normalized_ratio_basis_points.sort_by(|left, right| {
        right
            .events
            .cmp(&left.events)
            .then_with(|| left.basis_points_floor.cmp(&right.basis_points_floor))
    });
    normalized_ratio_basis_points.truncate(128);
    let candidates = accumulator
        .candidate_indexes
        .into_iter()
        .map(|(index, candidate)| CandidateReport {
            shared_array_index: index,
            coefficient_pairs: candidate
                .coefficient_pairs
                .into_iter()
                .map(|(first, second)| CoefficientPair { first, second })
                .collect(),
            exact_normal_proportions: candidate.exact_normal_proportions,
            exact_amount_proportions: candidate.exact_amount_proportions,
            exact_level_adjusted_normal_proportions: candidate
                .exact_level_adjusted_normal_proportions,
            integer_floor_compatible_normal_pairs: candidate.integer_floor_compatible_normal_pairs,
            level_outside_floor_compatible_normal_pairs: candidate
                .level_outside_floor_compatible_normal_pairs,
            level_inside_floor_compatible_normal_pairs: candidate
                .level_inside_floor_compatible_normal_pairs,
            normal_cross_residual_sum: candidate.normal_cross_residual_sum,
            normal_cross_residual_max: candidate.normal_cross_residual_max,
            level_adjusted_cross_residual_sum: candidate.level_adjusted_cross_residual_sum,
            level_adjusted_cross_residual_max: candidate.level_adjusted_cross_residual_max,
            exact_for_every_comparable_normal_pair: comparable > 0
                && candidate.exact_normal_proportions == comparable,
            level_adjusted_exact_for_every_comparable_normal_pair: comparable > 0
                && candidate.exact_level_adjusted_normal_proportions == comparable,
            integer_floor_compatible_for_every_comparable_normal_pair: comparable > 0
                && candidate.integer_floor_compatible_normal_pairs == comparable,
            level_outside_floor_compatible_for_every_comparable_normal_pair: comparable > 0
                && candidate.level_outside_floor_compatible_normal_pairs == comparable,
            level_inside_floor_compatible_for_every_comparable_normal_pair: comparable > 0
                && candidate.level_inside_floor_compatible_normal_pairs == comparable,
        })
        .collect();
    PairReport {
        result_kind: key.result_kind,
        damage_script: key.damage_script,
        first_row_selection_authority: key.first_row_selection_authority,
        second_row_selection_authority: key.second_row_selection_authority,
        ability_id: key.ability_id,
        first_hit_event_id: key.first_hit_event_id,
        second_hit_event_id: key.second_hit_event_id,
        first_damage_id: key.first_damage_id,
        second_damage_id: key.second_damage_id,
        paired_events: accumulator.paired_events,
        equal_owner_level: accumulator.equal_owner_level,
        equal_owner_stage: accumulator.equal_owner_stage,
        comparable_normal_values: comparable,
        normalized_ratio_basis_points,
        candidates,
        examples: accumulator.examples,
    }
}

fn observe_repeated_state(
    repeated_states: &mut BTreeMap<RepeatedStateKey, RepeatedStateAccumulator>,
    hit: &Hit,
    example_limit: usize,
) {
    let Some(normal_value) = hit.normal_value.filter(|value| *value > 0) else {
        return;
    };
    let key = RepeatedStateKey {
        rlog: hit.rlog.clone(),
        session_id: hit.session_id.clone(),
        run_ordinal: hit.run_ordinal,
        result_kind: hit.result_kind,
        damage_script: hit.row.family().to_owned(),
        row_selection_authority: hit.row_selection_authority,
        ability_id: hit.ability_id,
        hit_event_id: hit.hit_event_id,
        damage_id: hit.row.damage_id.clone(),
        source_entity_uuid: hit.source_entity_uuid,
        direct_source_entity_uuid: hit.direct_source_entity_uuid,
        target_entity_uuid: hit.target_entity_uuid,
        raw_attacker_uuid: hit.raw_attacker_uuid,
        raw_top_summoner_uuid: hit.raw_top_summoner_uuid,
        raw_owner_id: hit.raw_owner_id,
        owner_level: hit.owner_level,
        owner_stage: hit.owner_stage,
        critical: hit.critical,
        lucky: hit.lucky,
        causes_lucky: hit.causes_lucky,
        blocked: hit.blocked,
        periodic: hit.periodic,
        missed: hit.missed,
        damage_source: hit.damage_source,
        damage_type: hit.damage_type,
        property: hit.property,
        damage_mode: hit.damage_mode,
        reported_critical: hit.reported_critical,
        type_flags: hit.type_flags,
        normal_hit: hit.normal_hit,
        packet_position: hit.packet_position,
        hit_parts: hit.hit_parts.clone(),
        damage_weight: hit.damage_weight,
        passive_uuid: hit.passive_uuid,
        rainbow: hit.rainbow,
        skill_effect_uuid: hit.skill_effect_uuid,
        skill_effect_group_index: hit.skill_effect_group_index,
        skill_effect_component_index: hit.skill_effect_component_index,
        skill_effect_component_count: hit.skill_effect_component_count,
        source_full_attribute_fingerprint: hit.source_full_attribute_fingerprint,
        direct_source_full_attribute_fingerprint: hit.direct_source_full_attribute_fingerprint,
        target_full_attribute_fingerprint: hit.target_full_attribute_fingerprint,
        source_status_fingerprint: hit.source_status_fingerprint,
        direct_source_status_fingerprint: hit.direct_source_status_fingerprint,
        target_status_fingerprint: hit.target_status_fingerprint,
        source_hp_evidence: hit.source_hp_evidence,
        direct_source_hp_evidence: hit.direct_source_hp_evidence,
        target_hp_evidence: hit.target_hp_evidence,
    };
    let accumulator = repeated_states.entry(key).or_default();
    accumulator.events = accumulator.events.saturating_add(1);
    *accumulator.normal_values.entry(normal_value).or_default() += 1;
    if accumulator.example_sequences.len() < example_limit {
        accumulator.example_sequences.push(hit.sequence);
    }
}

fn repeated_state_variation(
    repeated_states: BTreeMap<RepeatedStateKey, RepeatedStateAccumulator>,
) -> RepeatedStateVariation {
    let mut groups = repeated_states
        .into_iter()
        .filter(|(_, accumulator)| accumulator.events >= 2)
        .map(|(key, accumulator)| repeated_state_report(key, accumulator))
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        right
            .events
            .cmp(&left.events)
            .then_with(|| {
                right
                    .distinct_normal_values
                    .cmp(&left.distinct_normal_values)
            })
            .then_with(|| left.damage_id.cmp(&right.damage_id))
    });

    RepeatedStateVariation {
        conclusion: "identical exposed packet state can still produce multiple normal values; this preserves evidence of an unexposed random or cast-snapshot input without assigning semantics to nested DamageAttr fields",
        groups,
    }
}

fn repeated_state_report(
    key: RepeatedStateKey,
    accumulator: RepeatedStateAccumulator,
) -> RepeatedStateReport {
    let values = accumulator
        .normal_values
        .keys()
        .copied()
        .collect::<Vec<_>>();
    let minimum_normal_value = values.first().copied().unwrap_or_default();
    let maximum_normal_value = values.last().copied().unwrap_or_default();
    RepeatedStateReport {
        rlog: key.rlog,
        session_id: key.session_id,
        run_ordinal: key.run_ordinal,
        result_kind: key.result_kind,
        damage_script: key.damage_script,
        row_selection_authority: key.row_selection_authority,
        ability_id: key.ability_id,
        hit_event_id: key.hit_event_id,
        damage_id: key.damage_id,
        source_entity_uuid: key.source_entity_uuid,
        direct_source_entity_uuid: key.direct_source_entity_uuid,
        target_entity_uuid: key.target_entity_uuid,
        raw_attacker_uuid: key.raw_attacker_uuid,
        raw_top_summoner_uuid: key.raw_top_summoner_uuid,
        raw_owner_id: key.raw_owner_id,
        source_hp_evidence: key.source_hp_evidence,
        direct_source_hp_evidence: key.direct_source_hp_evidence,
        target_hp_evidence: key.target_hp_evidence,
        events: accumulator.events,
        distinct_normal_values: values.len(),
        minimum_normal_value,
        maximum_normal_value,
        normal_values: accumulator
            .normal_values
            .into_iter()
            .map(|(value, events)| ValueCount { value, events })
            .collect(),
        example_sequences: accumulator.example_sequences,
    }
}

fn observe_hp_scaling_family(
    families: &mut BTreeMap<HpScalingFamilyKey, HpScalingAccumulator>,
    hit: &Hit,
    example_limit: usize,
) {
    let observation = HpScalingObservationKey {
        source_hp: resolved_hp_state(hit.source_hp_evidence),
        direct_source_hp: resolved_hp_state(hit.direct_source_hp_evidence),
        target_hp: resolved_hp_state(hit.target_hp_evidence),
        normal_value: hit.normal_value,
        amount: hit.amount,
        actual_amount: hit.actual_amount,
        hp_loss: hit.hp_loss,
        shield_loss: hit.shield_loss,
    };
    if observation.source_hp.is_none()
        && observation.direct_source_hp.is_none()
        && observation.target_hp.is_none()
    {
        return;
    }
    let key = HpScalingFamilyKey {
        result_kind: hit.result_kind,
        damage_script: hit.row.family().to_owned(),
        row_selection_authority: hit.row_selection_authority,
        rlog: hit.rlog.clone(),
        session_id: hit.session_id.clone(),
        run_ordinal: hit.run_ordinal,
        ability_id: hit.ability_id,
        hit_event_id: hit.hit_event_id,
        damage_id: hit.row.damage_id.clone(),
        source_entity_uuid: hit.source_entity_uuid,
        direct_source_entity_uuid: hit.direct_source_entity_uuid,
        target_entity_uuid: hit.target_entity_uuid,
        raw_attacker_uuid: hit.raw_attacker_uuid,
        raw_top_summoner_uuid: hit.raw_top_summoner_uuid,
        raw_owner_id: hit.raw_owner_id,
        owner_level: hit.owner_level,
        owner_stage: hit.owner_stage,
        critical: hit.critical,
        lucky: hit.lucky,
        causes_lucky: hit.causes_lucky,
        blocked: hit.blocked,
        periodic: hit.periodic,
        missed: hit.missed,
        damage_source: hit.damage_source,
        damage_type: hit.damage_type,
        property: hit.property,
        damage_mode: hit.damage_mode,
        reported_critical: hit.reported_critical,
        type_flags: hit.type_flags,
        normal_hit: hit.normal_hit,
        packet_position: hit.packet_position,
        hit_parts: hit.hit_parts.clone(),
        damage_weight: hit.damage_weight,
        passive_uuid: hit.passive_uuid,
        rainbow: hit.rainbow,
        skill_effect_uuid: hit.skill_effect_uuid,
        skill_effect_group_index: hit.skill_effect_group_index,
        skill_effect_component_index: hit.skill_effect_component_index,
        skill_effect_component_count: hit.skill_effect_component_count,
        source_hp_independent_attribute_fingerprint: hit
            .source_hp_independent_attribute_fingerprint,
        direct_source_hp_independent_attribute_fingerprint: hit
            .direct_source_hp_independent_attribute_fingerprint,
        target_hp_independent_attribute_fingerprint: hit
            .target_hp_independent_attribute_fingerprint,
        source_status_fingerprint: hit.source_status_fingerprint,
        direct_source_status_fingerprint: hit.direct_source_status_fingerprint,
        target_status_fingerprint: hit.target_status_fingerprint,
    };
    let family = families.entry(key).or_default();
    family.events = family.events.saturating_add(1);
    let sequence = hit.sequence;
    let observation = family.observations.entry(observation).or_default();
    observation.events = observation.events.saturating_add(1);
    if observation.example_sequences.len() < example_limit {
        observation.example_sequences.push(sequence);
    }
}

fn resolved_hp_state(evidence: HpEvidenceKey) -> Option<ResolvedHpState> {
    if evidence.subsequent_life_events != 0 {
        return None;
    }
    let current = i128::from(evidence.authoritative_current_hp?)
        .checked_sub(i128::from(evidence.subsequent_hp_loss))?
        .checked_add(i128::from(evidence.subsequent_healing_amount))?;
    let maximum = i128::from(evidence.authoritative_max_hp?);
    if maximum <= 0 || current < 0 || current > maximum {
        return None;
    }
    let current_hp = i64::try_from(current).ok()?;
    let max_hp = i64::try_from(maximum).ok()?;
    Some(ResolvedHpState {
        current_hp,
        max_hp,
        missing_hp: max_hp.checked_sub(current_hp)?,
    })
}

fn hp_scaling_variation(
    families: BTreeMap<HpScalingFamilyKey, HpScalingAccumulator>,
) -> HpScalingVariation {
    let mut formula_families = families
        .into_iter()
        .map(|(key, accumulator)| hp_scaling_family_report(key, accumulator))
        .collect::<Vec<_>>();
    formula_families.sort_by(|left, right| {
        right
            .distinct_exact_hp_states
            .cmp(&left.distinct_exact_hp_states)
            .then_with(|| right.events.cmp(&left.events))
            .then_with(|| left.damage_id.cmp(&right.damage_id))
    });
    HpScalingVariation {
        runtime_formula_authority: false,
        conclusion: "pre-event HP is reconstructed only from an authoritative HP snapshot plus intervening canonical HP loss and effective healing; exact affine fits are evidence candidates, two-state fits are explicitly insufficient, and no fit is promoted without same-build replay and provider conservation",
        formula_families,
    }
}

fn hp_scaling_family_report(
    key: HpScalingFamilyKey,
    accumulator: HpScalingAccumulator,
) -> HpScalingFamilyReport {
    let distinct_exact_hp_states = accumulator
        .observations
        .keys()
        .map(|observation| {
            (
                observation.source_hp,
                observation.direct_source_hp,
                observation.target_hp,
            )
        })
        .collect::<BTreeSet<_>>()
        .len();
    let affine_candidates = hp_affine_candidates(&accumulator.observations);
    let observations = accumulator
        .observations
        .into_iter()
        .map(|(key, accumulator)| HpScalingObservationReport {
            source_hp: key.source_hp,
            direct_source_hp: key.direct_source_hp,
            target_hp: key.target_hp,
            normal_value: key.normal_value,
            amount: key.amount,
            actual_amount: key.actual_amount,
            hp_loss: key.hp_loss,
            shield_loss: key.shield_loss,
            events: accumulator.events,
            example_sequences: accumulator.example_sequences,
        })
        .collect();
    HpScalingFamilyReport {
        rlog: key.rlog,
        session_id: key.session_id,
        run_ordinal: key.run_ordinal,
        result_kind: key.result_kind,
        damage_script: key.damage_script,
        row_selection_authority: key.row_selection_authority,
        ability_id: key.ability_id,
        hit_event_id: key.hit_event_id,
        damage_id: key.damage_id,
        source_entity_uuid: key.source_entity_uuid,
        direct_source_entity_uuid: key.direct_source_entity_uuid,
        target_entity_uuid: key.target_entity_uuid,
        raw_attacker_uuid: key.raw_attacker_uuid,
        raw_top_summoner_uuid: key.raw_top_summoner_uuid,
        raw_owner_id: key.raw_owner_id,
        events: accumulator.events,
        distinct_exact_hp_states,
        observations,
        affine_candidates,
    }
}

fn hp_affine_candidates(
    observations: &BTreeMap<HpScalingObservationKey, HpScalingObservationAccumulator>,
) -> Vec<HpAffineCandidate> {
    const BASES: [HpBasis; 9] = [
        HpBasis::SourceCurrentHp,
        HpBasis::SourceMaxHp,
        HpBasis::SourceMissingHp,
        HpBasis::DirectSourceCurrentHp,
        HpBasis::DirectSourceMaxHp,
        HpBasis::DirectSourceMissingHp,
        HpBasis::TargetCurrentHp,
        HpBasis::TargetMaxHp,
        HpBasis::TargetMissingHp,
    ];
    const OUTCOMES: [OutcomeMetric; 5] = [
        OutcomeMetric::NormalValue,
        OutcomeMetric::Amount,
        OutcomeMetric::ActualAmount,
        OutcomeMetric::HpLoss,
        OutcomeMetric::ShieldLoss,
    ];
    let mut reports = Vec::new();
    for basis in BASES {
        for outcome in OUTCOMES {
            let mut values = BTreeMap::<i64, BTreeSet<i64>>::new();
            for observation in observations.keys() {
                if let (Some(input), Some(output)) = (
                    hp_basis_value(observation, basis),
                    outcome_value(observation, outcome),
                ) {
                    values.entry(input).or_default().insert(output);
                }
            }
            if values.len() < 2 {
                continue;
            }
            let deterministic_at_each_basis_value =
                values.values().all(|outputs| outputs.len() == 1);
            let distinct_points = values.values().map(BTreeSet::len).sum();
            let points = values
                .iter()
                .filter_map(|(input, outputs)| {
                    (outputs.len() == 1).then(|| (*input, *outputs.first().unwrap()))
                })
                .collect::<Vec<_>>();
            let fit = deterministic_at_each_basis_value
                .then(|| exact_affine_fit(&points))
                .flatten();
            reports.push(HpAffineCandidate {
                basis,
                outcome,
                distinct_basis_values: values.len(),
                distinct_points,
                deterministic_at_each_basis_value,
                exact_affine_fit: fit.is_some(),
                sufficient_for_promotion_candidate: fit.is_some() && values.len() >= 3,
                slope_numerator: fit.map(|value| value.0.to_string()),
                slope_denominator: fit.map(|value| value.1.to_string()),
                intercept_numerator: fit.map(|value| value.2.to_string()),
                intercept_denominator: fit.map(|value| value.3.to_string()),
                zero_intercept: fit.map(|value| value.2 == 0),
            });
        }
    }
    reports
}

fn hp_basis_value(observation: &HpScalingObservationKey, basis: HpBasis) -> Option<i64> {
    let (state, field) = match basis {
        HpBasis::SourceCurrentHp => (observation.source_hp?, 0),
        HpBasis::SourceMaxHp => (observation.source_hp?, 1),
        HpBasis::SourceMissingHp => (observation.source_hp?, 2),
        HpBasis::DirectSourceCurrentHp => (observation.direct_source_hp?, 0),
        HpBasis::DirectSourceMaxHp => (observation.direct_source_hp?, 1),
        HpBasis::DirectSourceMissingHp => (observation.direct_source_hp?, 2),
        HpBasis::TargetCurrentHp => (observation.target_hp?, 0),
        HpBasis::TargetMaxHp => (observation.target_hp?, 1),
        HpBasis::TargetMissingHp => (observation.target_hp?, 2),
    };
    Some(match field {
        0 => state.current_hp,
        1 => state.max_hp,
        _ => state.missing_hp,
    })
}

fn outcome_value(observation: &HpScalingObservationKey, outcome: OutcomeMetric) -> Option<i64> {
    match outcome {
        OutcomeMetric::NormalValue => observation.normal_value,
        OutcomeMetric::Amount => Some(observation.amount),
        OutcomeMetric::ActualAmount => observation.actual_amount,
        OutcomeMetric::HpLoss => observation.hp_loss,
        OutcomeMetric::ShieldLoss => observation.shield_loss,
    }
}

/// Returns reduced `(slope numerator, slope denominator, intercept numerator,
/// intercept denominator)` when every point lies on one exact rational line.
fn exact_affine_fit(points: &[(i64, i64)]) -> Option<(i128, i128, i128, i128)> {
    if points.len() < 2 {
        return None;
    }
    let (x0, y0) = (i128::from(points[0].0), i128::from(points[0].1));
    let (x1, y1) = (i128::from(points[1].0), i128::from(points[1].1));
    let mut slope_numerator = y1.checked_sub(y0)?;
    let mut slope_denominator = x1.checked_sub(x0)?;
    if slope_denominator == 0 {
        return None;
    }
    if slope_denominator < 0 {
        slope_denominator = -slope_denominator;
        slope_numerator = -slope_numerator;
    }
    let slope_gcd = gcd_i128(slope_numerator, slope_denominator);
    slope_numerator /= slope_gcd;
    slope_denominator /= slope_gcd;
    for &(x, y) in &points[2..] {
        let left = i128::from(y)
            .checked_sub(y0)?
            .checked_mul(slope_denominator)?;
        let right = slope_numerator.checked_mul(i128::from(x).checked_sub(x0)?)?;
        if left != right {
            return None;
        }
    }
    let mut intercept_numerator = y0
        .checked_mul(slope_denominator)?
        .checked_sub(slope_numerator.checked_mul(x0)?)?;
    let mut intercept_denominator = slope_denominator;
    let intercept_gcd = gcd_i128(intercept_numerator, intercept_denominator);
    intercept_numerator /= intercept_gcd;
    intercept_denominator /= intercept_gcd;
    Some((
        slope_numerator,
        slope_denominator,
        intercept_numerator,
        intercept_denominator,
    ))
}

fn gcd_i128(mut left: i128, mut right: i128) -> i128 {
    left = left.abs();
    right = right.abs();
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn parse_source_routes(
    route_proof: &Value,
    expected_game_build: &str,
) -> Result<SourceRouteLookup, String> {
    let schema = route_proof
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "route proof is missing schema_version".to_owned())?;
    if schema < 6 {
        return Err(format!(
            "route proof schema {schema} is too old; schema 6 or newer is required"
        ));
    }
    let actual_build = route_proof
        .get("game_build")
        .and_then(Value::as_str)
        .ok_or_else(|| "route proof is missing game_build".to_owned())?;
    if actual_build != expected_game_build {
        return Err(format!(
            "route proof build {actual_build} does not match --build {expected_game_build}"
        ));
    }
    let keys = route_proof
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| "route proof is missing keys".to_owned())?;
    let mut routes = SourceRouteLookup::new();
    for key in keys {
        let ability_id = key
            .get("ability_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| "route key is missing ability_id".to_owned())?;
        let hit_event_id = key
            .get("hit_event_id")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| "route key has an invalid hit_event_id".to_owned())?;
        for selection in key
            .get("selection_by_damage_source")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let damage_source = selection
                .get("damage_source_id")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| "route selection has an invalid damage_source_id".to_owned())?;
            let damage_id = value_as_id(
                selection
                    .get("damage_attr_id")
                    .ok_or_else(|| "route selection is missing damage_attr_id".to_owned())?,
            )?;
            let route_key = (ability_id, hit_event_id, damage_source);
            if let Some(previous) = routes.insert(route_key, damage_id.clone())
                && previous != damage_id
            {
                return Err(format!(
                    "route proof conflicts for {ability_id}:{hit_event_id} source {damage_source}: {previous} versus {damage_id}"
                ));
            }
        }
    }
    Ok(routes)
}

fn resolve_damage_id(
    candidates: &[String],
    source_routes: &SourceRouteLookup,
    ability_id: i64,
    hit_event_id: i32,
    damage_source: Option<i32>,
) -> Option<DamageIdSelection> {
    if candidates.len() == 1 {
        return candidates
            .first()
            .cloned()
            .map(|damage_id| DamageIdSelection {
                damage_id,
                authority: RowSelectionAuthority::UniqueDamageAttrLookup,
            });
    }
    let selected = source_routes.get(&(ability_id, hit_event_id, damage_source?))?;
    candidates
        .iter()
        .any(|candidate| candidate == selected)
        .then(|| DamageIdSelection {
            damage_id: selected.clone(),
            authority: RowSelectionAuthority::PacketDamageSourceRoute,
        })
}

fn resolve_damage_id_without_hit(
    candidates: &[String],
    rows: &BTreeMap<String, DamageRow>,
    result_kind: CombatResultKind,
    exact_family_result_kind_authority: &BTreeMap<String, ResultKindCounts>,
) -> Option<DamageIdSelection> {
    if candidates.len() == 1 {
        let damage_id = candidates.first()?.clone();
        return rows.contains_key(&damage_id).then_some(DamageIdSelection {
            damage_id,
            authority: RowSelectionAuthority::UniqueAbilityDamageAttrLookup,
        });
    }
    let compatible = candidates
        .iter()
        .filter(|damage_id| {
            let Some(row) = rows.get(*damage_id) else {
                return false;
            };
            exact_family_result_kind_authority
                .get(row.family())
                .is_none_or(|counts| match result_kind {
                    CombatResultKind::Damage => counts.damage > 0 || counts.healing == 0,
                    CombatResultKind::Healing => counts.healing > 0 || counts.damage == 0,
                })
        })
        .collect::<Vec<_>>();
    (compatible.len() == 1).then(|| DamageIdSelection {
        damage_id: compatible[0].clone(),
        authority: RowSelectionAuthority::PacketResultKindFamilyExhaustion,
    })
}

fn scan_exact_family_result_kinds(
    path: &Path,
    expected_game_build: &str,
    lookup: &BTreeMap<String, Vec<String>>,
    source_routes: &SourceRouteLookup,
    rows: &BTreeMap<String, DamageRow>,
    result_kinds: &mut BTreeMap<String, ResultKindCounts>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let client_build = reader.header().region.client_build.clone();
    if client_build != expected_game_build {
        return Err(format!(
            "{} contains client build {} but --packet-build requires {}",
            file_label(path),
            client_build,
            expected_game_build
        )
        .into());
    }
    while let Some(envelope) = reader.next_event()? {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        let (kind, ability, hit_event_id, damage_source) = match &timeline.kind {
            TimelineEventKind::Damage(event) => (
                CombatResultKind::Damage,
                event.ability,
                event.hit_event_id,
                event.damage_source,
            ),
            TimelineEventKind::Healing(event) => (
                CombatResultKind::Healing,
                event.ability,
                event.hit_event_id,
                event.damage_source,
            ),
            _ => continue,
        };
        let (Some(ability), Some(hit_event_id)) = (ability, hit_event_id) else {
            continue;
        };
        let lookup_key = format!("{}:{hit_event_id}", ability.0);
        let candidates = lookup.get(&lookup_key).cloned().unwrap_or_default();
        let Some(selection) = resolve_damage_id(
            &candidates,
            source_routes,
            ability.0,
            hit_event_id,
            damage_source,
        ) else {
            continue;
        };
        let Some(row) = rows.get(&selection.damage_id) else {
            continue;
        };
        let counts = result_kinds.entry(row.family().to_owned()).or_default();
        match kind {
            CombatResultKind::Damage => counts.damage = counts.damage.saturating_add(1),
            CombatResultKind::Healing => counts.healing = counts.healing.saturating_add(1),
        }
    }
    Ok(())
}

fn build_ability_lookup(rows: &BTreeMap<String, DamageRow>) -> BTreeMap<i64, Vec<String>> {
    let mut lookup = BTreeMap::<i64, Vec<String>>::new();
    for row in rows.values() {
        lookup
            .entry(row.type_enum)
            .or_default()
            .push(row.damage_id.clone());
    }
    lookup
}

fn parse_lookup(surface: &Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let object = surface
        .get("linked_hit_event_candidate_lookup")
        .and_then(Value::as_object)
        .ok_or_else(|| "surface is missing linked_hit_event_candidate_lookup".to_owned())?;
    object
        .iter()
        .map(|(key, values)| {
            let values = values
                .as_array()
                .ok_or_else(|| format!("lookup {key} is not an array"))?
                .iter()
                .map(value_as_id)
                .collect::<Result<Vec<_>, _>>()?;
            Ok((key.clone(), values))
        })
        .collect()
}

fn parse_rows(
    surface: &Value,
    decoded_table: &Value,
) -> Result<BTreeMap<String, DamageRow>, String> {
    let rows = surface
        .get("rows")
        .and_then(Value::as_object)
        .ok_or_else(|| "surface is missing rows".to_owned())?;
    let decoded_rows = decoded_table
        .as_object()
        .ok_or_else(|| "decoded DamageAttrTable root is not an object".to_owned())?;
    rows.iter()
        .map(|(damage_id, row)| {
            let surface_damage_id = row
                .get("damage_id")
                .map(value_as_id)
                .transpose()?
                .ok_or_else(|| format!("surface row {damage_id} is missing damage_id"))?;
            if surface_damage_id != *damage_id {
                return Err(format!(
                    "surface row key {damage_id} disagrees with damage_id {surface_damage_id}"
                ));
            }
            let semantic_row = decoded_rows.get(damage_id).cloned().ok_or_else(|| {
                format!("surface row {damage_id} is absent from decoded DamageAttrTable")
            })?;
            let semantic_id = semantic_row
                .get("Id")
                .map(value_as_id)
                .transpose()?
                .ok_or_else(|| format!("decoded DamageAttr row {damage_id} is missing Id"))?;
            if semantic_id != *damage_id {
                return Err(format!(
                    "decoded DamageAttr row key {damage_id} disagrees with Id {semantic_id}"
                ));
            }
            let arrays = row
                .get("int_array_pool_1_candidates_by_offset")
                .and_then(Value::as_object);
            let surface_pve_damage_ratio =
                array_values(arrays.and_then(|values| values.get("28")))?;
            let surface_pve_fixed_parameter =
                array_values(arrays.and_then(|values| values.get("32")))?;
            let pve_damage_ratio = semantic_array_values(
                &semantic_row,
                "PVEDamageRadio",
                damage_id,
            )?;
            let pve_fixed_parameter = semantic_array_values(
                &semantic_row,
                "PVEFixedParameter",
                damage_id,
            )?;
            if surface_pve_damage_ratio != pve_damage_ratio {
                return Err(format!(
                    "DamageAttr row {damage_id} surface offset 28 does not equal decoded PVEDamageRadio"
                ));
            }
            if surface_pve_fixed_parameter != pve_fixed_parameter {
                return Err(format!(
                    "DamageAttr row {damage_id} surface offset 32 does not equal decoded PVEFixedParameter"
                ));
            }
            let damage_script = semantic_row
                .get("DamageScript")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    format!("decoded DamageAttr row {damage_id} has no string DamageScript")
                })?
                .to_owned();
            let type_enum = semantic_row
                .get("TypeEnum")
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    format!("decoded DamageAttr row {damage_id} has no integer TypeEnum")
                })?;
            Ok((
                damage_id.clone(),
                DamageRow {
                    damage_id: damage_id.clone(),
                    damage_script,
                    type_enum,
                    pve_damage_ratio,
                    pve_fixed_parameter,
                    semantic_row,
                },
            ))
        })
        .collect()
}

fn semantic_array_values(row: &Value, field: &str, damage_id: &str) -> Result<Vec<i64>, String> {
    row.get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("decoded DamageAttr row {damage_id} has no array {field}"))?
        .iter()
        .map(|value| {
            value.as_i64().ok_or_else(|| {
                format!("decoded DamageAttr row {damage_id} field {field} contains a non-integer")
            })
        })
        .collect()
}

fn array_values(value: Option<&Value>) -> Result<Vec<i64>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "array candidate is missing values".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "array value is not an integer".to_owned())
        })
        .collect()
}

fn value_as_id(value: &Value) -> Result<String, String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err("damage id is not a string or number".to_owned()),
    }
}

fn selected_pve_fixed_parameter(values: &[i64], owner_level: Option<i32>) -> Option<i64> {
    let level = usize::try_from(owner_level?).ok()?;
    level
        .checked_sub(1)
        .and_then(|index| values.get(index))
        .copied()
}

#[derive(Debug, Clone, Copy)]
struct RationalInterval {
    lower_numerator: i128,
    upper_numerator: i128,
    denominator: i128,
}

fn floor_scaled_base_interval(
    output: i64,
    coefficient: i64,
    output_offset: i64,
    input_offset: i64,
) -> Option<RationalInterval> {
    if coefficient <= 0 {
        return None;
    }
    let adjusted_output = i128::from(output) - i128::from(output_offset);
    if adjusted_output < 0 {
        return None;
    }
    let coefficient = i128::from(coefficient);
    let input_offset_term = i128::from(input_offset).saturating_mul(coefficient);
    Some(RationalInterval {
        lower_numerator: adjusted_output
            .saturating_mul(10_000)
            .saturating_sub(input_offset_term),
        upper_numerator: adjusted_output
            .saturating_add(1)
            .saturating_mul(10_000)
            .saturating_sub(input_offset_term),
        denominator: coefficient,
    })
}

fn rational_less(
    left_numerator: i128,
    left_denominator: i128,
    right_numerator: i128,
    right_denominator: i128,
) -> bool {
    left_numerator.saturating_mul(right_denominator)
        < right_numerator.saturating_mul(left_denominator)
}

fn floor_intervals_share_nonnegative_base(
    first: RationalInterval,
    second: RationalInterval,
) -> bool {
    rational_less(
        first.lower_numerator,
        first.denominator,
        second.upper_numerator,
        second.denominator,
    ) && rational_less(
        second.lower_numerator,
        second.denominator,
        first.upper_numerator,
        first.denominator,
    ) && first.upper_numerator > 0
        && second.upper_numerator > 0
}

fn floor_scaled_pair_with_offsets_has_shared_base(
    first_output: i64,
    second_output: i64,
    first_coefficient: i64,
    second_coefficient: i64,
    first_output_offset: i64,
    second_output_offset: i64,
    first_input_offset: i64,
    second_input_offset: i64,
) -> bool {
    let Some(first) = floor_scaled_base_interval(
        first_output,
        first_coefficient,
        first_output_offset,
        first_input_offset,
    ) else {
        return false;
    };
    let Some(second) = floor_scaled_base_interval(
        second_output,
        second_coefficient,
        second_output_offset,
        second_input_offset,
    ) else {
        return false;
    };
    floor_intervals_share_nonnegative_base(first, second)
}

fn floor_scaled_pair_has_shared_base(
    first_output: i64,
    second_output: i64,
    first_coefficient: i64,
    second_coefficient: i64,
) -> bool {
    floor_scaled_pair_with_offsets_has_shared_base(
        first_output,
        second_output,
        first_coefficient,
        second_coefficient,
        0,
        0,
        0,
        0,
    )
}

fn floor_scaled_pair_with_output_offsets_has_shared_base(
    first_output: i64,
    second_output: i64,
    first_coefficient: i64,
    second_coefficient: i64,
    first_output_offset: i64,
    second_output_offset: i64,
) -> bool {
    floor_scaled_pair_with_offsets_has_shared_base(
        first_output,
        second_output,
        first_coefficient,
        second_coefficient,
        first_output_offset,
        second_output_offset,
        0,
        0,
    )
}

fn floor_scaled_pair_with_input_offsets_has_shared_base(
    first_output: i64,
    second_output: i64,
    first_coefficient: i64,
    second_coefficient: i64,
    first_input_offset: i64,
    second_input_offset: i64,
) -> bool {
    floor_scaled_pair_with_offsets_has_shared_base(
        first_output,
        second_output,
        first_coefficient,
        second_coefficient,
        0,
        0,
        first_input_offset,
        second_input_offset,
    )
}

fn cross_residual(
    first: i64,
    second: i64,
    first_coefficient: i64,
    second_coefficient: i64,
) -> i128 {
    i128::from(first)
        .saturating_mul(i128::from(second_coefficient))
        .saturating_sub(i128::from(second).saturating_mul(i128::from(first_coefficient)))
}

fn observe_residual(residual: i128, exact: &mut u64, sum: &mut u128, maximum: &mut u128) {
    let absolute = residual.unsigned_abs();
    if absolute == 0 {
        *exact = exact.saturating_add(1);
    }
    *sum = sum.saturating_add(absolute);
    *maximum = (*maximum).max(absolute);
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

fn position_key(position: &rlogs_events::DamagePosition) -> PositionKey {
    PositionKey {
        x_bits: position.x.map(f32::to_bits),
        y_bits: position.y.map(f32::to_bits),
        z_bits: position.z.map(f32::to_bits),
    }
}

fn hp_evidence_key(tracker: Option<&HpEvidenceTracker>) -> HpEvidenceKey {
    tracker.map(|tracker| tracker.key).unwrap_or_default()
}

fn observe_combat_result_hp_evidence(
    trackers: &mut HashMap<(u32, i64), HpEvidenceTracker>,
    run_ordinal: u32,
    result: &CombatResultRef<'_>,
) {
    let tracker = trackers
        .entry((run_ordinal, result.target.entity_uuid.0))
        .or_default();
    match result.kind {
        CombatResultKind::Damage => tracker.observe_damage(result.hp_loss),
        CombatResultKind::Healing => {
            tracker.observe_healing(result.effective_healing.unwrap_or(result.amount))
        }
    }
}

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
    match attribute.decoded.as_ref() {
        Some(EntityAttributeValue::Integer(value)) => Some(*value),
        Some(EntityAttributeValue::Text(_)) | Some(EntityAttributeValue::Position { .. }) => None,
        None => decode_varint(&attribute.raw_value).map(|value| value as i64),
    }
}

fn increment_actor_kind(
    observations: &mut BTreeMap<String, BTreeMap<String, u64>>,
    damage_id: &str,
    kind: Option<ActorKind>,
) {
    let label = match kind {
        Some(ActorKind::Player) => "player".to_owned(),
        Some(ActorKind::Monster) => "monster".to_owned(),
        Some(ActorKind::Npc) => "npc".to_owned(),
        Some(ActorKind::SceneObject) => "scene_object".to_owned(),
        Some(ActorKind::Zone) => "zone".to_owned(),
        Some(ActorKind::Projectile) => "projectile".to_owned(),
        Some(ActorKind::Pet) => "pet".to_owned(),
        Some(ActorKind::TrainingDummy) => "training_dummy".to_owned(),
        Some(ActorKind::Drop) => "drop".to_owned(),
        Some(ActorKind::Field) => "field".to_owned(),
        Some(ActorKind::Trap) => "trap".to_owned(),
        Some(ActorKind::Collection) => "collection".to_owned(),
        Some(ActorKind::StaticObject) => "static_object".to_owned(),
        Some(ActorKind::Vehicle) => "vehicle".to_owned(),
        Some(ActorKind::Toy) => "toy".to_owned(),
        Some(ActorKind::Housing) => "housing".to_owned(),
        Some(ActorKind::Unknown(value)) => format!("unknown_{value}"),
        None => "unresolved".to_owned(),
    };
    *observations
        .entry(damage_id.to_owned())
        .or_default()
        .entry(label)
        .or_default() += 1;
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

fn is_volatile_attribute(attribute_id: i32) -> bool {
    matches!(attribute_id, 11310 | 20010)
}

fn attribute_fingerprint(attributes: Option<&BTreeMap<i32, i64>>, include_volatile: bool) -> u64 {
    let Some(attributes) = attributes else {
        return EMPTY_FINGERPRINT;
    };
    let mut hash = EMPTY_FINGERPRINT;
    for (attribute_id, value) in attributes {
        if !include_volatile && is_volatile_attribute(*attribute_id) {
            continue;
        }
        hash_scalar(&mut hash, i64::from(*attribute_id));
        hash_scalar(&mut hash, *value);
    }
    hash
}

fn hp_independent_attribute_fingerprint(attributes: Option<&BTreeMap<i32, i64>>) -> u64 {
    let Some(attributes) = attributes else {
        return EMPTY_FINGERPRINT;
    };
    let mut hash = EMPTY_FINGERPRINT;
    for (attribute_id, value) in attributes {
        if matches!(attribute_id, 11_310 | 11_320) {
            continue;
        }
        hash_scalar(&mut hash, i64::from(*attribute_id));
        hash_scalar(&mut hash, *value);
    }
    hash
}

fn status_fingerprint(statuses: Option<&StatusTracker>) -> u64 {
    statuses
        .map(StatusTracker::semantic_fingerprint)
        .unwrap_or(EMPTY_FINGERPRINT)
}

fn hash_scalar(hash: &mut u64, scalar: i64) {
    for byte in scalar.to_le_bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

const EMPTY_FINGERPRINT: u64 = 0xcbf29ce484222325_u64;

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn observe_packet_damage_value_shape(
    counts: &mut PacketValueShapeCounts,
    amount: i64,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
) {
    counts.results = counts.results.saturating_add(1);
    if amount == 0 {
        counts.amount_zero = counts.amount_zero.saturating_add(1);
    } else {
        counts.amount_nonzero = counts.amount_nonzero.saturating_add(1);
    }
    if normal_value.is_none() && lucky_value.is_none() {
        counts.without_component_value = counts.without_component_value.saturating_add(1);
        if amount == 0 {
            counts.zero_without_component_value =
                counts.zero_without_component_value.saturating_add(1);
        } else {
            counts.nonzero_without_component_value =
                counts.nonzero_without_component_value.saturating_add(1);
        }
    }
    if normal_value.is_some() {
        counts.with_normal_value = counts.with_normal_value.saturating_add(1);
    }
    if lucky_value.is_some() {
        counts.with_lucky_value = counts.with_lucky_value.saturating_add(1);
    }
    if normal_value.is_some() && lucky_value.is_some() {
        counts.with_both_values = counts.with_both_values.saturating_add(1);
    }
    if normal_value.is_some_and(|value| amount == value) {
        counts.amount_matches_normal_value = counts.amount_matches_normal_value.saturating_add(1);
    }
    if lucky_value.is_some_and(|value| amount == value) {
        counts.amount_matches_lucky_value = counts.amount_matches_lucky_value.saturating_add(1);
    }
    if let (Some(normal), Some(lucky)) = (normal_value, lucky_value) {
        if amount == normal.saturating_add(lucky) {
            counts.amount_matches_normal_plus_lucky =
                counts.amount_matches_normal_plus_lucky.saturating_add(1);
        }
    }
    if lucky == Some(true) {
        counts.lucky_flag_true = counts.lucky_flag_true.saturating_add(1);
    }
    if causes_lucky == Some(true) {
        counts.causes_lucky_true = counts.causes_lucky_true.saturating_add(1);
    }
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let game_build = take_value(&mut values, "--build")?
        .to_string_lossy()
        .into_owned();
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".to_owned());
    }
    let packet_build = take_optional_value(&mut values, "--packet-build")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| game_build.clone());
    if packet_build.is_empty() || !packet_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--packet-build requires a numeric client build".to_owned());
    }
    let surface = PathBuf::from(take_value(&mut values, "--surface")?);
    let decoded_table = PathBuf::from(take_value(&mut values, "--decoded-table")?);
    let route_proof = PathBuf::from(take_value(&mut values, "--route-proof")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let example_limit = take_optional_value(&mut values, "--example-limit")
        .map(|value| parse_usize(value, "--example-limit"))
        .transpose()?
        .unwrap_or(DEFAULT_EXAMPLE_LIMIT);
    let include_hp_scaling = if let Some(position) = values
        .iter()
        .position(|value| value == "--include-hp-scaling")
    {
        values.remove(position);
        true
    } else {
        false
    };
    let mut coefficient_families = BTreeSet::from(["Attack".to_owned(), "MAttack".to_owned()]);
    while let Some(position) = values
        .iter()
        .position(|value| value == "--coefficient-family")
    {
        if position + 1 >= values.len() {
            return Err("--coefficient-family requires a non-empty DamageScript value".to_owned());
        }
        values.remove(position);
        let family = values.remove(position).to_string_lossy().into_owned();
        if family.is_empty() {
            return Err("--coefficient-family requires a non-empty DamageScript value".to_owned());
        }
        coefficient_families.insert(family);
    }
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        values.remove(position);
        rlogs.push(PathBuf::from(values.remove(position)));
    }
    if rlogs.is_empty() {
        return Err("at least one --rlog is required".to_owned());
    }
    if !values.is_empty() {
        return Err(format!("unrecognized arguments: {values:?}"));
    }
    Ok(Arguments {
        game_build,
        packet_build,
        surface,
        decoded_table,
        route_proof,
        rlogs,
        output,
        example_limit,
        include_hp_scaling,
        coefficient_families,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let position = values
        .iter()
        .position(|value| value == flag)
        .ok_or_else(|| format!("missing {flag}"))?;
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

fn parse_usize(value: OsString, flag: &str) -> Result<usize, String> {
    value
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|error| format!("invalid {flag}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_route_resolves_only_the_packet_selected_candidate() {
        let proof = json!({
            "schema_version": 6,
            "game_build": "24568685",
            "keys": [{
                "ability_id": 920201,
                "hit_event_id": 1,
                "selection_by_damage_source": [
                    {"damage_source_id": 0, "damage_attr_id": 19202010101_i64},
                    {"damage_source_id": 1, "damage_attr_id": 392020101_i64}
                ]
            }]
        });
        let routes = parse_source_routes(&proof, "24568685").unwrap();
        let candidates = vec!["392020101".to_owned(), "19202010101".to_owned()];
        let skill_route = resolve_damage_id(&candidates, &routes, 920201, 1, Some(0)).unwrap();
        assert_eq!(skill_route.damage_id, "19202010101");
        assert_eq!(
            skill_route.authority,
            RowSelectionAuthority::PacketDamageSourceRoute
        );
        let bullet_route = resolve_damage_id(&candidates, &routes, 920201, 1, Some(1)).unwrap();
        assert_eq!(bullet_route.damage_id, "392020101");
        assert_eq!(
            bullet_route.authority,
            RowSelectionAuthority::PacketDamageSourceRoute
        );
        assert_eq!(
            resolve_damage_id(&candidates, &routes, 920201, 1, None),
            None
        );
    }

    #[test]
    fn one_damage_attr_candidate_retains_unique_lookup_authority() {
        let candidates = vec!["13015370102".to_owned()];
        let selection =
            resolve_damage_id(&candidates, &SourceRouteLookup::new(), 301537, 2, None).unwrap();
        assert_eq!(selection.damage_id, "13015370102");
        assert_eq!(
            selection.authority,
            RowSelectionAuthority::UniqueDamageAttrLookup
        );
    }

    #[test]
    fn missing_hit_resolves_only_an_ability_with_one_current_build_row() {
        let mut rows = BTreeMap::new();
        rows.insert(
            "10101".to_owned(),
            DamageRow {
                damage_id: "10101".to_owned(),
                damage_script: "Heal".to_owned(),
                type_enum: 101,
                pve_damage_ratio: vec![],
                pve_fixed_parameter: vec![],
                semantic_row: json!({}),
            },
        );
        let lookup = build_ability_lookup(&rows);
        let selection = resolve_damage_id_without_hit(
            &lookup[&101],
            &rows,
            CombatResultKind::Healing,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(selection.damage_id, "10101");
        assert_eq!(
            selection.authority,
            RowSelectionAuthority::UniqueAbilityDamageAttrLookup
        );

        rows.insert(
            "10102".to_owned(),
            DamageRow {
                damage_id: "10102".to_owned(),
                damage_script: "HealLucky".to_owned(),
                type_enum: 101,
                pve_damage_ratio: vec![],
                pve_fixed_parameter: vec![],
                semantic_row: json!({}),
            },
        );
        let lookup = build_ability_lookup(&rows);
        assert!(
            resolve_damage_id_without_hit(
                &lookup[&101],
                &rows,
                CombatResultKind::Healing,
                &BTreeMap::new(),
            )
            .is_none()
        );
    }

    #[test]
    fn missing_hit_uses_packet_kind_only_after_exact_family_exhaustion() {
        let rows = BTreeMap::from([
            (
                "10101".to_owned(),
                DamageRow {
                    damage_id: "10101".to_owned(),
                    damage_script: "AttackLucky".to_owned(),
                    type_enum: 101,
                    pve_damage_ratio: vec![],
                    pve_fixed_parameter: vec![],
                    semantic_row: json!({}),
                },
            ),
            (
                "10102".to_owned(),
                DamageRow {
                    damage_id: "10102".to_owned(),
                    damage_script: "PHealLucky".to_owned(),
                    type_enum: 101,
                    pve_damage_ratio: vec![],
                    pve_fixed_parameter: vec![],
                    semantic_row: json!({}),
                },
            ),
        ]);
        let candidates = build_ability_lookup(&rows)[&101].clone();
        let authority = BTreeMap::from([(
            "AttackLucky".to_owned(),
            ResultKindCounts {
                damage: 50,
                healing: 0,
            },
        )]);
        let healing = resolve_damage_id_without_hit(
            &candidates,
            &rows,
            CombatResultKind::Healing,
            &authority,
        )
        .unwrap();
        assert_eq!(healing.damage_id, "10102");
        assert_eq!(
            healing.authority,
            RowSelectionAuthority::PacketResultKindFamilyExhaustion
        );
        assert!(
            resolve_damage_id_without_hit(
                &candidates,
                &rows,
                CombatResultKind::Healing,
                &BTreeMap::new(),
            )
            .is_none()
        );
    }

    #[test]
    fn decoded_damage_row_is_joined_losslessly_and_gates_standard_families() {
        let surface = json!({
            "rows": {
                "123": {
                    "damage_id": 123,
                    "int_array_pool_1_candidates_by_offset": {
                        "28": {"values": [5200, 3400]},
                        "32": {"values": [17]}
                    }
                }
            }
        });
        let decoded = json!({
            "123": {
                "Id": 123,
                "DamageScript": "Attack",
                "TypeEnum": 55,
                "PVEDamageRadio": [5200, 3400],
                "PVEFixedParameter": [17],
                "PVEStunnedDamage": [91],
                "PartDamageRadio": [22],
                "AbnormalDamage": [[0]],
                "DamageWeight": [],
                "IsProfession": true
            }
        });
        let rows = parse_rows(&surface, &decoded).unwrap();
        let row = rows.get("123").unwrap();
        assert_eq!(row.damage_script, "Attack");
        assert_eq!(row.type_enum, 55);
        assert_eq!(row.semantic_row["PVEStunnedDamage"], json!([91]));
        assert!(row.supports_coefficient_comparison(&BTreeSet::from([
            "Attack".to_owned(),
            "MAttack".to_owned(),
        ])));
    }

    #[test]
    fn coefficient_family_extension_is_explicit() {
        let row = DamageRow {
            damage_id: "129008400103".to_owned(),
            damage_script: "AutoAttack".to_owned(),
            type_enum: 1,
            pve_damage_ratio: vec![34_500],
            pve_fixed_parameter: vec![34],
            semantic_row: json!({}),
        };
        let defaults = BTreeSet::from(["Attack".to_owned(), "MAttack".to_owned()]);
        assert!(!row.supports_coefficient_comparison(&defaults));
        let extended = BTreeSet::from([
            "Attack".to_owned(),
            "MAttack".to_owned(),
            "AutoAttack".to_owned(),
        ]);
        assert!(row.supports_coefficient_comparison(&extended));
    }

    #[test]
    fn decoded_damage_row_rejects_a_surface_array_mismatch() {
        let surface = json!({
            "rows": {
                "123": {
                    "damage_id": 123,
                    "int_array_pool_1_candidates_by_offset": {
                        "28": {"values": [520]},
                        "32": {"values": []}
                    }
                }
            }
        });
        let decoded = json!({
            "123": {
                "Id": 123,
                "DamageScript": "AddShieldByHp",
                "TypeEnum": 55,
                "PVEDamageRadio": [5200],
                "PVEFixedParameter": []
            }
        });
        let error = parse_rows(&surface, &decoded).unwrap_err();
        assert!(error.contains("does not equal decoded PVEDamageRadio"));
    }

    #[test]
    fn source_route_rejects_a_stale_build() {
        let proof = json!({
            "schema_version": 6,
            "game_build": "24252055",
            "keys": []
        });
        assert!(
            parse_source_routes(&proof, "24568685")
                .unwrap_err()
                .contains("does not match")
        );
    }

    #[test]
    fn pve_fixed_parameter_selection_is_one_based() {
        assert_eq!(
            selected_pve_fixed_parameter(&[12, 28, 44], Some(1)),
            Some(12)
        );
        assert_eq!(
            selected_pve_fixed_parameter(&[12, 28, 44], Some(3)),
            Some(44)
        );
        assert_eq!(selected_pve_fixed_parameter(&[12, 28, 44], Some(0)), None);
        assert_eq!(selected_pve_fixed_parameter(&[12, 28, 44], Some(4)), None);
    }

    #[test]
    fn cross_residual_detects_exact_proportion() {
        assert_eq!(cross_residual(300, 200, 12_000, 8_000), 0);
        assert_ne!(cross_residual(301, 200, 12_000, 8_000), 0);
    }

    #[test]
    fn floor_intervals_accept_rounding_residuals_with_a_shared_base() {
        assert!(floor_scaled_pair_has_shared_base(
            34_643, 23_095, 12_000, 8_000
        ));
        assert!(!floor_scaled_pair_has_shared_base(
            34_644, 23_095, 12_000, 8_000
        ));
    }

    #[test]
    fn fixed_value_after_coefficient_uses_output_offsets() {
        assert!(floor_scaled_pair_with_output_offsets_has_shared_base(
            1_210, 820, 12_000, 8_000, 10, 20
        ));
        assert!(!floor_scaled_pair_with_output_offsets_has_shared_base(
            1_212, 820, 12_000, 8_000, 10, 20
        ));
    }

    #[test]
    fn fixed_value_before_coefficient_uses_input_offsets() {
        assert!(floor_scaled_pair_with_input_offsets_has_shared_base(
            1_320, 960, 12_000, 8_000, 100, 200
        ));
        assert!(!floor_scaled_pair_with_input_offsets_has_shared_base(
            1_322, 960, 12_000, 8_000, 100, 200
        ));
    }

    #[test]
    fn output_offsets_cannot_exceed_the_observed_output() {
        assert!(!floor_scaled_pair_with_output_offsets_has_shared_base(
            9, 820, 12_000, 8_000, 10, 20
        ));
    }

    #[test]
    fn hp_state_is_reconstructed_before_the_current_result() {
        let state = resolved_hp_state(HpEvidenceKey {
            authoritative_sequence: Some(10),
            authoritative_current_hp: Some(9_000),
            authoritative_max_hp: Some(10_000),
            subsequent_damage_events: 2,
            subsequent_hp_loss: 1_500,
            subsequent_healing_events: 1,
            subsequent_healing_amount: 400,
            subsequent_life_events: 0,
        })
        .unwrap();
        assert_eq!(state.current_hp, 7_900);
        assert_eq!(state.max_hp, 10_000);
        assert_eq!(state.missing_hp, 2_100);
    }

    #[test]
    fn hp_state_reconstruction_fails_closed_across_life_or_invalid_bounds() {
        let mut evidence = HpEvidenceKey {
            authoritative_sequence: Some(10),
            authoritative_current_hp: Some(9_000),
            authoritative_max_hp: Some(10_000),
            subsequent_damage_events: 0,
            subsequent_hp_loss: 0,
            subsequent_healing_events: 0,
            subsequent_healing_amount: 0,
            subsequent_life_events: 1,
        };
        assert_eq!(resolved_hp_state(evidence), None);
        evidence.subsequent_life_events = 0;
        evidence.subsequent_healing_amount = 2_000;
        assert_eq!(resolved_hp_state(evidence), None);
    }

    #[test]
    fn exact_affine_fit_retains_a_reduced_rational_formula() {
        assert_eq!(
            exact_affine_fit(&[(10, 8), (14, 14), (22, 26)]),
            Some((3, 2, -7, 1))
        );
        assert_eq!(exact_affine_fit(&[(10, 8), (14, 14), (22, 27)]), None);
    }

    #[test]
    fn hp_independent_fingerprint_ignores_only_current_and_max_hp() {
        let first = BTreeMap::from([(11_310, 5_000), (11_320, 10_000), (20_010, 3)]);
        let hp_changed = BTreeMap::from([(11_310, 4_000), (11_320, 12_000), (20_010, 3)]);
        let energy_changed = BTreeMap::from([(11_310, 5_000), (11_320, 10_000), (20_010, 4)]);
        assert_eq!(
            hp_independent_attribute_fingerprint(Some(&first)),
            hp_independent_attribute_fingerprint(Some(&hp_changed))
        );
        assert_ne!(
            hp_independent_attribute_fingerprint(Some(&first)),
            hp_independent_attribute_fingerprint(Some(&energy_changed))
        );
    }
}
