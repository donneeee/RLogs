//! Versioned BPSR rDPS formula configuration with its authored-build identity.
//!
//! Formula discovery and game-file scanning stay offline. The live path parses
//! this compact pack once and then reads the validated values through a shared
//! immutable reference. Exact build identity and an explicit global or
//! component-scoped promotion gate must match before the projector emits any
//! provider credit. Candidate formulas remain available for offline audit
//! while a later promotion can replace data and proofs without scattering
//! build IDs or coefficients through the projector code.

use std::{collections::HashMap, sync::OnceLock};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::state_formula::CriticalDamageFactorInterpretation;

const RDPS_RUNTIME_SCHEMA_VERSION: u16 = 37;
const RDPS_PROMOTION_INVENTORY_SCHEMA_VERSION: u16 = 1;

const KNOWN_PROMOTION_BLOCKERS: [&str; 6] = [
    "protocol-pack-identity",
    "canonical-replay-conservation",
    "protocol-event-coverage",
    "critical-damage-factor-interpretation-authority",
    "party-support-formula-frontier",
    "historical-build-runtime-promotion-not-reviewed",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RdpsRuntimePolicy {
    canonical_events_retained: bool,
    unresolved_events_hidden: bool,
    candidate_rules_enabled: bool,
    critical_damage_factor_interpretation_authority: bool,
    party_support_formula_frontier_complete: bool,
    promotion_requires_exact_conservation: bool,
    game_files_are_identity_and_coefficient_evidence_not_packet_occurrence_evidence: bool,
    runtime_promotion_allowed: bool,
    same_deployment_build_mismatch: String,
    warn_on_build_mismatch: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackFamilyRuntimeConfig {
    pub final_attribute_id: i32,
    pub intermediate_attribute_id: i32,
    pub base_add_attribute_id: i32,
    pub extra_add_attribute_id: i32,
    pub raw_percent_attribute_id: i32,
}

impl AttackFamilyRuntimeConfig {
    fn attribute_ids(self) -> [i32; 5] {
        [
            self.final_attribute_id,
            self.intermediate_attribute_id,
            self.base_add_attribute_id,
            self.extra_add_attribute_id,
            self.raw_percent_attribute_id,
        ]
    }

    fn is_valid(self) -> bool {
        let values = self.attribute_ids();
        values.iter().all(|value| *value > 0)
            && values
                .iter()
                .enumerate()
                .all(|(index, value)| !values[..index].contains(value))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackFamiliesRuntimeConfig {
    pub physical: AttackFamilyRuntimeConfig,
    pub magical: AttackFamilyRuntimeConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DamageStageCatalogRuntimeConfig {
    pub authored_game_build: String,
    pub source_table: String,
    pub source_table_hash: i64,
    pub source_row_count: usize,
    pub unique_lookup_keys: usize,
    pub ambiguous_lookup_keys: usize,
    pub standard_attack_rules: usize,
    pub standard_magic_attack_rules: usize,
    pub standard_rules: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TeamLuckLuckyDamageRoute {
    pub ability_id: i64,
    pub hit_event_id: i32,
}

const TEAM_LUCK_CURRENT_LUCKY_DAMAGE_ROUTES: [TeamLuckLuckyDamageRoute; 9] = [
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_101,
        hit_event_id: 3,
    },
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_102,
        hit_event_id: 3,
    },
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_103,
        hit_event_id: 3,
    },
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_104,
        hit_event_id: 3,
    },
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_105,
        hit_event_id: 3,
    },
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_107,
        hit_event_id: 3,
    },
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_109,
        hit_event_id: 3,
    },
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_110,
        hit_event_id: 3,
    },
    TeamLuckLuckyDamageRoute {
        ability_id: 2_031_111,
        hit_event_id: 3,
    },
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TeamLuckCriticalDamageRatioProof {
    content_sha256: String,
    pair_audit_sha256: String,
    exact_build_cohort_sha256: String,
    current_build_preflight_sha256: String,
    exact_build_rlogs: usize,
    strict_normal_critical_pairs: usize,
    authority_pairs: usize,
    unresolved_hidden_state_pairs: usize,
    sessions: usize,
    abilities: usize,
    targets: usize,
    critical_damage_raw_values: usize,
    maximum_pair_gap_micros: u64,
    additive_maximum_absolute_residual: u64,
    rejected_direct_minimum_absolute_residual: u64,
}

impl TeamLuckCriticalDamageRatioProof {
    fn is_valid(&self) -> bool {
        self.content_sha256 == "24eb04c1cffae2014da6aaab9066125fc17cbc4b4a3af0993a0bb9315b3c1c02"
            && self.pair_audit_sha256
                == "04659a18ad261fa4717b92a90edc3a96a3410291057a98876353a72e77bc3290"
            && self.exact_build_cohort_sha256
                == "5e9c52a0a166463dab85c04c4b8e291e9ddf1ffab48596ac362e8bbcd75e0457"
            && self.current_build_preflight_sha256
                == "5362fc06d3d64dd7917ae64e6735485a819d89dbfc5d36e71e1847373ca887c2"
            && self.exact_build_rlogs == 26
            && self.strict_normal_critical_pairs == 30
            && self.authority_pairs == 21
            && self.unresolved_hidden_state_pairs == 9
            && self.authority_pairs + self.unresolved_hidden_state_pairs
                == self.strict_normal_critical_pairs
            && self.sessions == 7
            && self.abilities == 5
            && self.targets == 12
            && self.critical_damage_raw_values == 13
            && self.maximum_pair_gap_micros == 1_085_197
            && self.additive_maximum_absolute_residual == 1
            && self.rejected_direct_minimum_absolute_residual == 2_221
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamLuckCombinedDamageProof {
    runtime_diff_sha256: String,
    packet_component_audit_sha256: String,
    conservation_replay_sha256: String,
    exact_build_rlogs: usize,
    exact_build_sessions: usize,
    combined_eligible_events: usize,
    combined_emitted_events: usize,
    combined_suppressed_overlap_events: usize,
    combined_emitted_outside_lifecycle_events: usize,
    invalid_factor_events: usize,
    conservation_replay_schema_version: u16,
    conservation_replay_rlogs: usize,
    conservation_replay_events: u64,
    conserved_rlogs: usize,
    runtime_target_match_rlogs: usize,
    team_luck_emitted_events: u64,
    team_luck_projected_rdmg: u64,
    rational_projection_overflow_count: u64,
    packet_audit_rows: usize,
    packet_audit_damage_rows: usize,
    packet_audit_source_linked_damage_rows: usize,
    combined_packet_rows: usize,
    combined_packet_rows_approved_route: usize,
    combined_packet_rows_lucky_component_only: usize,
    combined_packet_rows_reported_amount_match: usize,
    combined_packet_rows_with_normal_value: usize,
}

impl TeamLuckCombinedDamageProof {
    fn is_valid(&self) -> bool {
        self.runtime_diff_sha256
            == "5496fdacaca45af42e72cc825b195d86552229c6166bcf4775ce9890e55b2d58"
            && self.packet_component_audit_sha256
                == "a31f207d55a5d75ecbdcd38c6b4df7cce72ab5abdba53b5fc6eeb8dd697fe38d"
            && self.conservation_replay_sha256
                == "976632bcf673e0fd23238ab621e7e1d9309d87ca5a4de1c03932706b275ca740"
            && self.exact_build_rlogs == 26
            && self.exact_build_sessions == 20
            && self.combined_eligible_events == 977
            && self.combined_emitted_events == 973
            && self.combined_suppressed_overlap_events == 4
            && self.combined_emitted_events + self.combined_suppressed_overlap_events
                == self.combined_eligible_events
            && self.combined_emitted_outside_lifecycle_events == 0
            && self.invalid_factor_events == 0
            && self.conservation_replay_schema_version == 27
            && self.conservation_replay_rlogs == 26
            && self.conservation_replay_events == 6_411_565
            && self.conserved_rlogs == self.conservation_replay_rlogs
            && self.runtime_target_match_rlogs == self.conservation_replay_rlogs
            && self.team_luck_emitted_events == 204_067
            && self.team_luck_projected_rdmg == 683_146_042
            && self.rational_projection_overflow_count == 0
            && self.packet_audit_rows == 56_090
            && self.packet_audit_damage_rows == 52_465
            && self.packet_audit_source_linked_damage_rows == 51_581
            && self.combined_packet_rows == 5_017
            && self.combined_packet_rows_approved_route == self.combined_packet_rows
            && self.combined_packet_rows_lucky_component_only == self.combined_packet_rows
            && self.combined_packet_rows_reported_amount_match == self.combined_packet_rows
            && self.combined_packet_rows_with_normal_value == 0
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TeamLuckRuntimeConfig {
    pub effect_id: i64,
    pub source_type_id: i32,
    pub source_config_id: i64,
    pub critical_damage_attribute_id: i32,
    pub lucky_damage_attribute_id: i32,
    pub critical_raw_delta: i64,
    pub lucky_raw_delta: i64,
    pub combined_critical_lucky_enabled: bool,
    combined_damage_current_build_packet_component_authority: bool,
    combined_damage_exact_rational_cross_term_authority: bool,
    combined_damage_protocol_pack_migration_authority: bool,
    combined_damage_formula_authority_basis: String,
    combined_damage_proof: TeamLuckCombinedDamageProof,
    pub critical_damage_current_build_lifecycle_authority: bool,
    pub critical_damage_current_build_executor_authority: bool,
    pub critical_damage_exact_rational_attribution_authority: bool,
    pub critical_damage_protocol_pack_migration_authority: bool,
    pub critical_damage_authorized_protocol_pack_digests: Vec<String>,
    critical_damage_formula_authority_basis: String,
    critical_damage_ratio_proof: TeamLuckCriticalDamageRatioProof,
    pub lucky_damage_current_build_lifecycle_authority: bool,
    pub lucky_damage_current_build_executor_authority: bool,
    pub lucky_damage_exact_rational_attribution_authority: bool,
    pub lucky_damage_protocol_pack_migration_authority: bool,
    pub lucky_damage_authorized_protocol_pack_digests: Vec<String>,
    formula_authority_basis: String,
    accounting_method: String,
    pub server_integer_counterfactual_authority: bool,
    rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub critical_damage_runtime_transfer_enabled: bool,
    pub lucky_damage_runtime_transfer_enabled: bool,
    pub lucky_damage_routes: Vec<TeamLuckLuckyDamageRoute>,
}

impl TeamLuckRuntimeConfig {
    pub(crate) fn is_lucky_damage_route(
        &self,
        ability_id: Option<i64>,
        hit_event_id: Option<i32>,
    ) -> bool {
        ability_id
            .zip(hit_event_id)
            .is_some_and(|(ability_id, hit_event_id)| {
                self.lucky_damage_routes.iter().any(|route| {
                    route.ability_id == ability_id && route.hit_event_id == hit_event_id
                })
            })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FunctionalAmpRuntimeConfig {
    pub effect_id: i64,
    pub source_config_id: i64,
    pub self_multiplier_effect_id: i64,
    pub passive_damage_effect_id: i64,
    pub passive_stack_effect_id: i64,
    pub attack_percent_raw_delta: i64,
    damage_scripts: Vec<String>,
    /// Exact packet proof for the historical build establishes the supported
    /// Attack/MAttack component, but cannot promote a newer build by itself.
    pub attack_magic_historical_formula_authority: bool,
    /// The target build's generic status schema preserves provider, recipient,
    /// instance, and lifecycle state under the exact protocol-pack identity.
    pub attack_magic_target_build_lifecycle_schema_authority: bool,
    /// The Attack/MAttack formula is authoritative for this target build. This
    /// may be a direct replay or an explicitly certified cross-build migration.
    pub attack_magic_target_build_formula_authority: bool,
    /// Whether the effect itself occurred in the evidence cohort. A dormant
    /// promoted rule may remain false and must not turn absence into a window.
    pub attack_magic_target_build_effect_occurrence_observed: bool,
    /// Whether target-build damage was replayed while this effect was active.
    /// This remains separate from a certified formula migration.
    pub attack_magic_target_build_damage_replay_observed: bool,
    /// Auditable basis for target-build formula authority.
    formula_authority_basis: String,
    /// Even a migrated production rule activates only from an observed exact-
    /// build lifecycle and reversible recipient transition in the current run.
    pub dormant_activation_requires_observed_lifecycle: bool,
    /// The rDPS allocation contract applied after the current run proves the
    /// migrated component through its exact reversible packet transition.
    accounting_method: String,
    /// The server's hidden per-hit integer boundary remains unclaimed.
    pub server_integer_counterfactual_authority: bool,
    /// Exact fractions are accumulated before one UI projection boundary.
    rational_integer_projection: String,
    /// Unknown same-stage or cross-stage combinations retain ordinary damage
    /// but emit no provider transfer.
    pub unresolved_overlap_fails_closed: bool,
    /// Production authority for only the per-hit Attack/MAttack component.
    pub attack_magic_runtime_transfer_enabled: bool,
    /// Speed is a separate action-opportunity counterfactual and cannot ride
    /// on the per-hit Attack/MAttack proof.
    pub speed_runtime_transfer_enabled: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrimaryAttackLane {
    PhysicalAttack,
    MagicalAttack,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttributeFamilyRounding {
    Floor,
    /// Round positive values to the nearest integer, but reject an exact .5
    /// tie until the client tie-breaking rule is independently proven.
    NearestNonTie,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrimaryStatRecipientRule {
    pub recipient_class_id: i32,
    pub attack_lane: PrimaryAttackLane,
    pub primary_attribute_family: AttackFamilyRuntimeConfig,
    pub primary_percent_raw_delta: i64,
    pub primary_to_attack_numerator: i64,
    pub primary_to_attack_denominator: i64,
    damage_scripts: Vec<String>,
}

impl PrimaryStatRecipientRule {
    fn has_matching_damage_script(&self) -> bool {
        match self.attack_lane {
            PrimaryAttackLane::PhysicalAttack => self.damage_scripts == ["Attack"],
            PrimaryAttackLane::MagicalAttack => self.damage_scripts == ["MAttack"],
        }
    }

    fn is_valid(&self) -> bool {
        self.recipient_class_id > 0
            && self.primary_attribute_family.is_valid()
            && self.primary_percent_raw_delta > 0
            && self.primary_to_attack_numerator > 0
            && self.primary_to_attack_denominator > 0
            && self.has_matching_damage_script()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MechanicalPowerReplayProof {
    pub candidate_content_sha256: String,
    pub candidate_content_bytes: u64,
    pub production_content_sha256: String,
    pub production_content_bytes: u64,
    pub replay_schema_version: u16,
    pub exact_build_rlogs: u32,
    pub total_canonical_events: u64,
    pub runtime_target_match_rlogs: u32,
    pub candidate_target_match_rlogs: u32,
    pub conserved_rlogs: u32,
    pub rational_projection_overflow_count: u64,
    pub emitted_contribution_events: u64,
    pub sessions_with_emissions: u32,
    pub observed_damage: u64,
    pub projected_rdmg: i64,
    pub relationship_rows: u32,
    pub provider_entity_uuids: u32,
    pub recipient_entity_uuids: u32,
    pub affected_ability_ids: u32,
    pub all_damage_context_complete: bool,
}

impl MechanicalPowerReplayProof {
    fn is_current_authority(&self) -> bool {
        self.candidate_content_sha256
            == "66208338429276ee33effbf12ffa51be3701d8bc95a469c869c6d64de0a36f50"
            && self.candidate_content_bytes == 127_645_137
            && self.production_content_sha256
                == "594b7129537f68768823737c5dd2d106c42ca2e28fdf4ace23d1ab1047c0f644"
            && self.production_content_bytes == 150_449_431
            && self.replay_schema_version == 27
            && self.exact_build_rlogs == 26
            && self.total_canonical_events == 6_411_565
            && self.runtime_target_match_rlogs == 26
            && self.candidate_target_match_rlogs == 26
            && self.conserved_rlogs == 26
            && self.rational_projection_overflow_count == 0
            && self.emitted_contribution_events == 23_672
            && self.sessions_with_emissions == 8
            && self.observed_damage == 3_260_149_962
            && self.projected_rdmg == 138_300_062
            && self.relationship_rows == 21
            && self.provider_entity_uuids == 1
            && self.recipient_entity_uuids == 1
            && self.affected_ability_ids == 21
            && self.all_damage_context_complete
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MechanicalPowerRuntimeConfig {
    pub effect_id: i64,
    pub source_config_id: i64,
    pub source_config_must_be_absent: bool,
    pub duration_millis: u64,
    pub haste_attribute_id: i32,
    pub recipient_rules: Vec<PrimaryStatRecipientRule>,
    pub requires_recipient_packet_transition: bool,
    pub haste_is_action_opportunity: bool,
    pub universal_tier_formula_enabled: bool,
    /// Static coefficient rows and packet Attack inputs do not prove where the
    /// server applies the coefficient relative to later damage stages.
    pub damage_stage_operation_order_authority: bool,
    /// The audit projector currently enumerates a floor candidate, but no
    /// retained packet field or controlled pair proves the server boundary.
    pub damage_stage_integer_rounding_authority: bool,
    /// Exact current-pack lifecycle/recipient closure for the promoted
    /// class-11, observed tier-0 transition only.
    pub class_11_tier_0_current_pack_lifecycle_authority: bool,
    /// Authority for proportional allocation of observed integer damage from
    /// the exact packet-proven primary -> Attack stage body. This deliberately
    /// does not claim the server's hidden counterfactual integer boundary.
    pub class_11_tier_0_exact_rational_attribution_authority: bool,
    pub replay: MechanicalPowerReplayProof,
    /// The observed-final-damage rDPS accounting policy used for the exact
    /// class-11 +750 transition. This remains distinct from any claim about
    /// the hidden server implementation.
    accounting_method: String,
    pub server_integer_counterfactual_authority: bool,
    rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    /// Production transfer may use the exact-rational observed-damage contract
    /// for only the explicitly listed classes and packet-observed magnitudes.
    pub runtime_transfer_enabled: bool,
    pub runtime_recipient_class_ids: Vec<i32>,
    pub runtime_primary_percent_raw_deltas: Vec<i64>,
}

impl MechanicalPowerRuntimeConfig {
    pub(crate) fn production_primary_percent_raw_delta(
        &self,
        recipient_class_id: i32,
    ) -> Option<i64> {
        (self.runtime_transfer_enabled
            && self
                .runtime_recipient_class_ids
                .contains(&recipient_class_id)
            && self.runtime_primary_percent_raw_deltas.len() == 1)
            .then(|| self.runtime_primary_percent_raw_deltas[0])
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HarmonyGraceDirectReplayProof {
    pub candidate_content_sha256: String,
    pub candidate_content_bytes: u64,
    pub production_content_sha256: String,
    pub production_content_bytes: u64,
    pub replay_schema_version: u16,
    pub exact_build_rlogs: u32,
    pub total_canonical_events: u64,
    pub runtime_target_match_rlogs: u32,
    pub candidate_target_match_rlogs: u32,
    pub conserved_rlogs: u32,
    pub rational_projection_overflow_count: u64,
    pub paired_output_baseline_events: u64,
    pub paired_output_baseline_rdmg: i64,
    pub production_harmony_events: u64,
    pub production_harmony_rdmg: i64,
    pub direct_increment_events: u64,
    pub direct_increment_sessions: u32,
    pub direct_increment_observed_damage: u64,
    pub direct_increment_rdmg: i64,
    pub direct_relationship_rows: u32,
    pub direct_provider_entity_uuids: u32,
    pub direct_recipient_entity_uuids: u32,
    pub direct_affected_ability_ids: u32,
    pub all_damage_context_complete: bool,
    pub ordinary_damage: u64,
    pub production_contribution_given: i64,
    pub production_contribution_received: i64,
}

impl HarmonyGraceDirectReplayProof {
    fn is_current_authority(&self) -> bool {
        self.candidate_content_sha256
            == "b3bcc7e78a327d09953ea61baed553d42f81ec83f7a1829890bc5589fa8dc76d"
            && self.candidate_content_bytes == 125_956_982
            && self.production_content_sha256
                == "0dc5899f99589795a78a345c1dd24c049a36e0da85fe0c3c1db370d5770d4803"
            && self.production_content_bytes == 150_655_567
            && self.replay_schema_version == 27
            && self.exact_build_rlogs == 26
            && self.total_canonical_events == 6_411_565
            && self.runtime_target_match_rlogs == 26
            && self.candidate_target_match_rlogs == 26
            && self.conserved_rlogs == 26
            && self.rational_projection_overflow_count == 0
            && self.paired_output_baseline_events == 57
            && self.paired_output_baseline_rdmg == 99_124
            && self.production_harmony_events == 70
            && self.production_harmony_rdmg == 106_089
            && self.direct_increment_events == 13
            && self.direct_increment_sessions == 2
            && self.direct_increment_observed_damage == 597_972
            && self.direct_increment_rdmg == 6_965
            && self.direct_relationship_rows == 6
            && self.direct_provider_entity_uuids == 1
            && self.direct_recipient_entity_uuids == 1
            && self.direct_affected_ability_ids == 6
            && self.all_damage_context_complete
            && self.ordinary_damage == 96_705_532_690
            && self.production_contribution_given == 1_207_303_316
            && self.production_contribution_received == 1_207_303_316
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HarmonyGraceRuntimeConfig {
    pub effect_id: i64,
    pub source_terminal_effect_id: i64,
    pub source_config_must_be_absent: bool,
    pub source_type_id: Option<i32>,
    pub source_config_id: Option<i64>,
    pub primary_family_rounding: AttributeFamilyRounding,
    /// Exact current-pack provider/lifecycle/recipient closure for the only
    /// production class enabled by this runtime revision.
    pub class_11_current_pack_lifecycle_authority: bool,
    /// Authority for proportional allocation of observed integer damage from
    /// the exact packet-proven Attack-stage body. This is deliberately not a
    /// claim about the server's hidden counterfactual integer boundary.
    pub class_11_exact_rational_attribution_authority: bool,
    /// Exact 26-log candidate/production delta proving that the direct route
    /// adds only rows not already owned by the paired-output route.
    pub direct_replay: HarmonyGraceDirectReplayProof,
    /// Names the rDPS accounting contract independently from the server's
    /// hidden per-hit counterfactual implementation. The observed final
    /// damage remains authoritative and the provider owns only its adjacent,
    /// packet-proven stage marginal.
    accounting_method: String,
    pub server_integer_counterfactual_authority: bool,
    rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub recipient_rules: Vec<PrimaryStatRecipientRule>,
    /// Production may use the exact-rational observed-damage contract when its
    /// lifecycle, class, overlap, and projection authorities are all closed.
    pub runtime_transfer_enabled: bool,
    /// Only recipient classes with exact behavioral closure may be enabled.
    /// Candidate rules for other classes remain available to offline replay.
    pub runtime_recipient_class_ids: Vec<i32>,
    /// Exact paired final-damage observations may authorize remote recipients
    /// without a character snapshot. The learner remains build-scoped and
    /// emits only the active rows that have a same-context inactive witness.
    pub remote_paired_output_runtime_transfer_enabled: bool,
    pub remote_paired_output_ignored_effect_ids: Vec<i64>,
    pub remote_paired_output_formula_effect_ids: Vec<i64>,
    pub remote_paired_output_max_pair_gap_micros: u64,
    /// Exact +2% primary-stat magnitude is an upper envelope for a same-stage
    /// damage marginal when every other packet formula input is unchanged.
    pub remote_paired_output_max_provider_share_basis_points: i64,
    pub remote_paired_output_min_distinct_targets: u32,
}

impl HarmonyGraceRuntimeConfig {
    fn has_valid_source_origin_rule(&self) -> bool {
        match (
            self.source_config_must_be_absent,
            self.source_type_id,
            self.source_config_id,
        ) {
            (true, None, None) => true,
            (false, Some(source_type_id), Some(source_config_id)) => {
                source_type_id > 0 && source_config_id > 0
            }
            _ => false,
        }
    }

    pub fn matches_source_origin(&self, origin: Option<(i32, i64)>) -> bool {
        match (
            self.source_config_must_be_absent,
            self.source_type_id,
            self.source_config_id,
            origin,
        ) {
            (true, None, None, None) => true,
            (false, Some(expected_type), Some(expected_config), Some(actual)) => {
                actual == (expected_type, expected_config)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatResonanceRuntimeConfig {
    pub effect_id: i64,
    pub source_type_id: i32,
    pub source_config_id: i64,
    pub current_build_external_lifecycle_authority: bool,
    pub exact_same_wire_final_attack_marginal_authority: bool,
    accounting_method: String,
    pub server_integer_counterfactual_authority: bool,
    rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub runtime_transfer_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieryBattleWillRuntimeConfig {
    pub effect_id: i64,
    pub source_type_id: i32,
    pub source_config_id: i64,
    pub provider_raw_percent_delta: i64,
    pub current_build_external_lifecycle_authority: bool,
    pub current_build_provider_ownership_authority: bool,
    pub exact_mirrored_attack_raw_percent_transition_authority: bool,
    pub local_recipient_only: bool,
    accounting_method: String,
    pub server_integer_counterfactual_authority: bool,
    rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub runtime_transfer_enabled: bool,
}

/// Exact standalone output owned by Encore (55333).
///
/// The English name "Encore" is verified in build 24252055. Numeric effect ID
/// 55333 is observed in current build 24687926; the current-build English
/// locale has not been independently extracted, so numeric identity remains
/// authoritative at runtime.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EncoreDirectOutputRuntimeConfig {
    pub effect_id: i64,
    pub damage_action_ids: [i64; 2],
    pub excluded_healing_action_id: i64,
    current_build_lifecycle_authority: bool,
    current_build_provider_ownership_authority: bool,
    exact_packet_final_integer_authority: bool,
    same_provider_instances_coalesced: bool,
    external_provider_only: bool,
    ordinary_damage_unchanged: bool,
    accounting_method: String,
    proof_content_sha256: String,
    proof_exact_build_rlogs: u32,
    proof_attributed_events: u64,
    proof_attributed_rdmg: i64,
    locale_evidence: String,
    pub runtime_transfer_enabled: bool,
}

impl EncoreDirectOutputRuntimeConfig {
    pub(crate) fn is_damage_action_id(&self, action_id: i64) -> bool {
        self.damage_action_ids.contains(&action_id)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThunderwindVectorRuntimeConfig {
    pub source_level: i64,
    pub critical_chance_raw_delta: i64,
    pub critical_damage_raw_delta: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThunderwindRuntimeConfig {
    pub effect_id: i64,
    pub child_effect_id: i64,
    pub child_source_config_id: i64,
    pub summon_entity_type_id: i64,
    pub summon_config_attribute_id: i32,
    pub summon_config_id: i64,
    pub summon_owner_attribute_ids: [i32; 2],
    pub critical_chance_attribute_id: i32,
    pub critical_damage_attribute_id: i32,
    pub packet_proven_vectors: Vec<ThunderwindVectorRuntimeConfig>,
    pub recipient_scope: String,
    pub runtime_transfer_enabled: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationVectorRuntimeConfig {
    pub primary_raw_add_delta: i64,
    pub secondary_raw_add_delta: i64,
    pub external_damage_delta: i64,
    pub property_damage_raw_delta: i64,
    pub provider_full_bloom: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationChanceProofRuntimeConfig {
    pub content_sha256: String,
    pub critical_factor_proof_sha256: String,
    pub exact_build_rlogs: u32,
    pub exact_single_provider_events: u64,
    pub emitted_critical_events: u64,
    pub emitted_lucky_events: u64,
    pub retained_combined_events: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationChanceReplayRuntimeConfig {
    pub content_sha256: String,
    pub content_bytes: u64,
    pub exact_build_rlogs: u32,
    pub total_canonical_events: u64,
    pub emitted_contribution_events: u64,
    pub sessions_with_emissions: u32,
    pub projected_credit: u64,
    pub all_runtime_target_match: bool,
    pub all_conserved: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationChanceMagnitudeRuntimeConfig {
    pub effect_level: i32,
    pub chance_raw_delta: i64,
    pub exact_removal_instances: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationRecipientDependencyMagnitudeRuntimeConfig {
    pub critical_chance_raw_delta: i64,
    pub critical_damage_raw_delta: i64,
    pub exact_isolated_transition_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationRecipientDependencyReplayRuntimeConfig {
    pub content_sha256: String,
    pub content_bytes: u64,
    pub exact_build_rlogs: u32,
    pub total_canonical_events: u64,
    pub emitted_contribution_events: u64,
    pub sessions_with_emissions: u32,
    pub dependency_affected_sessions: u32,
    pub projected_credit: u64,
    pub dependency_increment: u64,
    pub all_runtime_target_match: bool,
    pub all_conserved: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationRecipientDependencyRuntimeConfig {
    pub effect_id: i64,
    pub talent_id: i64,
    pub recipient_class_id: i32,
    pub required_status_level: i32,
    pub critical_chance_to_critical_damage_numerator: i64,
    pub critical_chance_to_critical_damage_denominator: i64,
    pub exact_build_status_lifecycle_authority: bool,
    pub exact_build_formula_authority: bool,
    pub proof_content_sha256: String,
    pub proof_exact_build_rlogs: u32,
    pub exact_isolated_transition_count: u32,
    pub magnitudes: Vec<InspirationRecipientDependencyMagnitudeRuntimeConfig>,
    pub replay: InspirationRecipientDependencyReplayRuntimeConfig,
}

impl InspirationRecipientDependencyRuntimeConfig {
    fn is_current_authority(&self) -> bool {
        self.effect_id == 2_203_220
            && self.talent_id == 1_122
            && self.recipient_class_id == 11
            && self.required_status_level == 1
            && self.critical_chance_to_critical_damage_numerator == 1
            && self.critical_chance_to_critical_damage_denominator == 2
            && self.exact_build_status_lifecycle_authority
            && self.exact_build_formula_authority
            && self.proof_content_sha256
                == "bfb1b112ccf848aa6b8dbc98980c1d937522dbe07b909abfd5a1e457de4d6dff"
            && self.proof_exact_build_rlogs == 26
            && self.exact_isolated_transition_count == 4
            && self.magnitudes
                == [
                    InspirationRecipientDependencyMagnitudeRuntimeConfig {
                        critical_chance_raw_delta: 150,
                        critical_damage_raw_delta: 75,
                        exact_isolated_transition_count: 3,
                    },
                    InspirationRecipientDependencyMagnitudeRuntimeConfig {
                        critical_chance_raw_delta: 300,
                        critical_damage_raw_delta: 150,
                        exact_isolated_transition_count: 1,
                    },
                ]
            && self.replay.content_sha256
                == "b90669032e38b323cfc961a4b630c3d2bb0cbf4d2de5f695a741344054c2fe1e"
            && self.replay.content_bytes == 134_021_194
            && self.replay.exact_build_rlogs == 26
            && self.replay.total_canonical_events == 6_411_565
            && self.replay.emitted_contribution_events == 13_618
            && self.replay.sessions_with_emissions == 6
            && self.replay.dependency_affected_sessions == 6
            && self.replay.projected_credit == 40_569_478
            && self.replay.dependency_increment == 7_546_368
            && self.replay.all_runtime_target_match
            && self.replay.all_conserved
    }

    pub(crate) fn critical_damage_raw_delta(&self, critical_chance_raw_delta: i64) -> Option<i64> {
        let numerator = critical_chance_raw_delta
            .checked_mul(self.critical_chance_to_critical_damage_numerator)?;
        (numerator % self.critical_chance_to_critical_damage_denominator == 0)
            .then(|| numerator / self.critical_chance_to_critical_damage_denominator)
            .filter(|delta| *delta > 0)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationBaseLuckyDamageReplayRuntimeConfig {
    pub content_sha256: String,
    pub content_bytes: u64,
    pub exact_build_rlogs: u32,
    pub total_canonical_events: u64,
    pub emitted_contribution_events: u64,
    pub sessions_with_emissions: u32,
    pub dependency_affected_sessions: u32,
    pub projected_credit: u64,
    pub dependency_increment: u64,
    pub all_runtime_target_match: bool,
    pub all_conserved: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationBaseLuckyDamageDependencyRuntimeConfig {
    pub lucky_chance_attribute_id: i32,
    pub lucky_damage_attribute_id: i32,
    pub chance_to_lucky_damage_numerator: i64,
    pub chance_to_lucky_damage_denominator: i64,
    pub rounding: String,
    pub exact_build_game_file_semantic_authority: bool,
    pub exact_build_packet_formula_authority: bool,
    pub proof_content_sha256: String,
    pub proof_content_bytes: u64,
    pub proof_exact_build_rlogs: u32,
    pub packet_lucky_damage_updates_evaluated: u32,
    pub informative_additive_strata: u32,
    pub informative_strata_observations: u32,
    pub exact_marginal_comparisons: u32,
    pub marginal_mismatches: u32,
    pub replay: InspirationBaseLuckyDamageReplayRuntimeConfig,
}

impl InspirationBaseLuckyDamageDependencyRuntimeConfig {
    fn is_current_authority(&self) -> bool {
        self.lucky_chance_attribute_id == 11_780
            && self.lucky_damage_attribute_id == 12_530
            && self.chance_to_lucky_damage_numerator == 1
            && self.chance_to_lucky_damage_denominator == 4
            && self.rounding == "mathematical_floor"
            && self.exact_build_game_file_semantic_authority
            && self.exact_build_packet_formula_authority
            && self.proof_content_sha256
                == "da51a9c254ed4acb23844c2ae445b70d0742c1f54d066d59894b7af422dab63e"
            && self.proof_content_bytes == 12_177
            && self.proof_exact_build_rlogs == 26
            && self.packet_lucky_damage_updates_evaluated == 348
            && self.informative_additive_strata == 2
            && self.informative_strata_observations == 278
            && self.exact_marginal_comparisons == 11
            && self.marginal_mismatches == 0
            && self.replay.content_sha256
                == "f4243f9f877f998980f2cccddb63569b73622e64d1197aa4bdadbf5ce1d3168d"
            && self.replay.content_bytes == 134_022_165
            && self.replay.exact_build_rlogs == 26
            && self.replay.total_canonical_events == 6_411_565
            && self.replay.emitted_contribution_events == 13_618
            && self.replay.sessions_with_emissions == 6
            && self.replay.dependency_affected_sessions == 5
            && self.replay.projected_credit == 40_570_593
            && self.replay.dependency_increment == 1_115
            && self.replay.all_runtime_target_match
            && self.replay.all_conserved
    }

    pub(crate) fn lucky_damage_raw_delta(
        &self,
        current_lucky_chance_raw: i64,
        provider_lucky_chance_raw_delta: i64,
    ) -> Option<i64> {
        if current_lucky_chance_raw <= 0
            || provider_lucky_chance_raw_delta <= 0
            || provider_lucky_chance_raw_delta > current_lucky_chance_raw
        {
            return None;
        }
        let provider_removed_chance =
            current_lucky_chance_raw.checked_sub(provider_lucky_chance_raw_delta)?;
        let active = current_lucky_chance_raw
            .checked_mul(self.chance_to_lucky_damage_numerator)?
            .div_euclid(self.chance_to_lucky_damage_denominator);
        let provider_removed = provider_removed_chance
            .checked_mul(self.chance_to_lucky_damage_numerator)?
            .div_euclid(self.chance_to_lucky_damage_denominator);
        active
            .checked_sub(provider_removed)
            .filter(|delta| *delta > 0)
    }
}

impl InspirationChanceProofRuntimeConfig {
    fn is_current_authority(&self) -> bool {
        self.content_sha256 == "924f8945ca37963e0726358d7385d7e1ffe19e8caeba15b40d5ba644885aba26"
            && self.critical_factor_proof_sha256
                == "efd4a6b61f3cbd757725a2a65b75982641487b457b1f9eee6c606a030111b938"
            && self.exact_build_rlogs == 26
            && self.exact_single_provider_events == 10_682
            && self.emitted_critical_events == 10_615
            && self.emitted_lucky_events == 6
            && self.retained_combined_events == 10_518
    }
}

impl InspirationChanceReplayRuntimeConfig {
    fn is_current_authority(&self) -> bool {
        self.content_sha256 == "94e7d3fc0037259aa41dee081d0dd6147b9408350e20b763d992ca31592a5400"
            && self.content_bytes == 133_969_924
            && self.exact_build_rlogs == 26
            && self.total_canonical_events == 6_411_565
            && self.emitted_contribution_events == 13_618
            && self.sessions_with_emissions == 6
            && self.projected_credit == 33_023_110
            && self.all_runtime_target_match
            && self.all_conserved
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationCombinedReconciliationRuntimeConfig {
    content_sha256: String,
    content_bytes: u64,
    source_authority_sha256: String,
    schema_version: u16,
    exact_build_rlogs: u64,
    total_canonical_events: u64,
    authorized_event_count: u64,
    newly_authorized_event_count: u64,
    extended_authorized_event_count: u64,
    new_route_decision_count: u64,
    general_route_decision_count: u64,
    decision_event_count: u64,
    emitted_event_count: u64,
    suppressed_event_count: u64,
    formula_trace_complete_count: u64,
    formula_trace_mismatch_count: u64,
    all_route_formula_trace_complete_count: u64,
    runtime_target_match_rlogs: u64,
    conserved_rlogs: u64,
    rational_projection_overflow_count: u64,
    runtime_authority: bool,
}

impl InspirationCombinedReconciliationRuntimeConfig {
    fn is_current_receipt(&self) -> bool {
        self.content_sha256 == "ff509ddd07df0ff80806a318d253b6ecea2eb2ffafec1c65d5544ae3f646093a"
            && self.content_bytes == 151_351_524
            && self.source_authority_sha256
                == "924f8945ca37963e0726358d7385d7e1ffe19e8caeba15b40d5ba644885aba26"
            && self.schema_version == 1
            && self.exact_build_rlogs == 26
            && self.total_canonical_events == 6_411_565
            && self.authorized_event_count == 61
            && self.newly_authorized_event_count == 51
            && self.extended_authorized_event_count == 112
            && self.new_route_decision_count == 265
            && self.general_route_decision_count == 326
            && self.decision_event_count == 61
            && self.emitted_event_count == 53
            && self.suppressed_event_count == 273
            && self.formula_trace_complete_count == 61
            && self.formula_trace_mismatch_count == 0
            && self.all_route_formula_trace_complete_count == 326
            && self.runtime_target_match_rlogs == 26
            && self.conserved_rlogs == 26
            && self.rational_projection_overflow_count == 0
            && self.runtime_authority
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationRuntimeConfig {
    pub effect_id: i64,
    pub source_config_id: i64,
    pub full_bloom_effect_id: i64,
    pub full_bloom_full_name: String,
    pub full_bloom_source_type_id: i32,
    pub full_bloom_source_config_id: i64,
    pub full_bloom_required_level: i32,
    pub full_bloom_required_stacks: u32,
    pub full_bloom_duration_millis: u64,
    pub full_bloom_potency_numerator: i64,
    pub full_bloom_potency_denominator: i64,
    pub full_bloom_external_windows: u64,
    pub full_bloom_exact_build_rlogs: u32,
    pub full_bloom_receipt_sha256: String,
    pub full_bloom_receipt_bytes: u64,
    pub full_bloom_emitted_contribution_events: u64,
    pub full_bloom_projected_credit: u64,
    pub full_bloom_increment_runtime_transfer_enabled: bool,
    pub primary_raw_add_attribute_ids: [i32; 4],
    pub critical_chance_attribute_id: i32,
    pub critical_chance_raw_add_attribute_id: i32,
    pub lucky_chance_attribute_id: i32,
    pub lucky_chance_raw_add_attribute_id: i32,
    pub mastery_attribute_id: i32,
    pub mastery_raw_add_attribute_id: i32,
    pub versatility_attribute_id: i32,
    pub versatility_raw_add_attribute_id: i32,
    pub external_damage_attribute_id: i32,
    pub property_damage_attribute_id: i32,
    pub property_damage_property: i32,
    pub haste_attribute_id: i32,
    pub packet_proven_vectors: Vec<InspirationVectorRuntimeConfig>,
    damage_scripts: Vec<String>,
    pub current_build_lifecycle_authority: bool,
    pub current_build_magnitude_authority: bool,
    pub exact_rational_chance_attribution_authority: bool,
    pub protocol_pack_migration_authority: bool,
    pub authorized_protocol_pack_digests: Vec<String>,
    pub formula_authority_basis: String,
    pub accounting_method: String,
    pub server_integer_counterfactual_authority: bool,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub canonical_conservation_replay_authority: bool,
    pub chance_proof: InspirationChanceProofRuntimeConfig,
    pub chance_replay: InspirationChanceReplayRuntimeConfig,
    pub chance_magnitudes: Vec<InspirationChanceMagnitudeRuntimeConfig>,
    pub combined_critical_lucky_route: InspirationCombinedCriticalLuckyRouteRuntimeConfig,
    pub combined_reconciliation: InspirationCombinedReconciliationRuntimeConfig,
    pub recipient_dependency: InspirationRecipientDependencyRuntimeConfig,
    pub base_lucky_damage_dependency: InspirationBaseLuckyDamageDependencyRuntimeConfig,
    pub critical_chance_runtime_transfer_enabled: bool,
    pub lucky_chance_runtime_transfer_enabled: bool,
    pub combined_critical_lucky_runtime_transfer_enabled: bool,
    /// Independent gate for recipient talent/passive/Imagine conversions
    /// observed as downstream Crit-DMG or Lucky-DMG packet transitions.
    pub recipient_dependency_runtime_transfer_enabled: bool,
    /// Independent gate for the universal base Luck -> Lucky-DMG stage. Class,
    /// talent, and Imagine-specific terms remain separate dependency rules.
    pub base_lucky_damage_dependency_runtime_transfer_enabled: bool,
    /// Base Attack/Mastery/External/Property/Haste composition remains a
    /// separate frontier from the proven chance component.
    pub runtime_transfer_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspirationCombinedCriticalLuckyRouteRuntimeConfig {
    pub ability_id: i64,
    pub hit_event_id: i32,
    pub damage_source: i32,
    pub packet_type_flags: i32,
    pub reported_critical_must_be_absent: bool,
    pub normal_value_must_be_absent: bool,
    pub lucky_value_must_equal_observed_damage: bool,
    pub provider_origin_source_type_id: i32,
    pub formula_identity: String,
    pub exact_game_file_level_magnitude_authority: bool,
    pub event_time_recipient_factor_authority: bool,
    pub packet_final_rational_counterfactual: bool,
}

impl InspirationRuntimeConfig {
    pub(crate) fn chance_raw_delta_for_effect_level(&self, effect_level: i32) -> Option<i64> {
        self.chance_magnitudes
            .iter()
            .find(|entry| entry.effect_level == effect_level)
            .map(|entry| entry.chance_raw_delta)
    }

    pub(crate) fn chance_raw_delta_for_effect_level_and_mode(
        &self,
        effect_level: i32,
        provider_full_bloom: bool,
    ) -> Option<i64> {
        let base = self.chance_raw_delta_for_effect_level(effect_level)?;
        if !provider_full_bloom {
            return Some(base);
        }
        let scaled = base.checked_mul(self.full_bloom_potency_numerator)?;
        (scaled % self.full_bloom_potency_denominator == 0)
            .then(|| scaled / self.full_bloom_potency_denominator)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InspireSpeedLaneRuntimeConfig {
    Normal,
    Guide,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspireActionRouteRuntimeConfig {
    pub ability_id: i64,
    pub owner_stage: i32,
    pub lane: InspireSpeedLaneRuntimeConfig,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspireRecipientModeRuntimeConfig {
    pub class_id: i32,
    pub specialization_id: i32,
    pub normal_speed_delta: i64,
    pub guide_speed_delta: i64,
}

const INSPIRE_CURRENT_ACTION_ROUTES: [InspireActionRouteRuntimeConfig; 36] = [
    InspireActionRouteRuntimeConfig {
        ability_id: 1_419,
        owner_stage: 2,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_424,
        owner_stage: 2,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_424,
        owner_stage: 4,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_433,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_501,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_541,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_901,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_902,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_907,
        owner_stage: 1,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_922,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_927,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_932,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_942,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_233,
        owner_stage: 1,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_294,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_295,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_312,
        owner_stage: 1,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_313,
        owner_stage: 4,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_332,
        owner_stage: 1,
        lane: InspireSpeedLaneRuntimeConfig::Guide,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_352,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_352,
        owner_stage: 1,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_362,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 7_997,
        owner_stage: 10,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 7_998,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 150_101,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 199_902,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 220_301,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 230_801,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 230_901,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 231_001,
        owner_stage: 0,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_121_508,
        owner_stage: 1,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_121_508,
        owner_stage: 2,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 1_121_508,
        owner_stage: 3,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_900_840,
        owner_stage: 1,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_900_840,
        owner_stage: 2,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
    InspireActionRouteRuntimeConfig {
        ability_id: 2_900_840,
        owner_stage: 3,
        lane: InspireSpeedLaneRuntimeConfig::Normal,
    },
];

const INSPIRE_CURRENT_RECIPIENT_MODES: [InspireRecipientModeRuntimeConfig; 4] = [
    InspireRecipientModeRuntimeConfig {
        class_id: 5,
        specialization_id: 110,
        normal_speed_delta: 200,
        guide_speed_delta: 2_000,
    },
    InspireRecipientModeRuntimeConfig {
        class_id: 9,
        specialization_id: 113,
        normal_speed_delta: 600,
        guide_speed_delta: 1_000,
    },
    InspireRecipientModeRuntimeConfig {
        class_id: 11,
        specialization_id: 117,
        normal_speed_delta: 600,
        guide_speed_delta: 1_000,
    },
    InspireRecipientModeRuntimeConfig {
        class_id: 13,
        specialization_id: 120,
        normal_speed_delta: 600,
        guide_speed_delta: 2_000,
    },
];

/// Exact-build packet-final accounting for party status 31602 (Inspire).
///
/// Remote action-start packets are not observable on the reviewed transport.
/// This component therefore uses the packet-observed damage-resolution
/// window as its explicit throughput accounting boundary. It never claims a
/// hidden server damage integer or rewrites ordinary damage.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspireRuntimeConfig {
    pub effect_id: i64,
    pub source_type_id: i32,
    pub source_config_id: i64,
    pub required_stacks: i32,
    pub duration_millis: u64,
    pub normal_speed_attribute_id: i32,
    pub guide_speed_attribute_id: i32,
    pub action_routes: Vec<InspireActionRouteRuntimeConfig>,
    pub recipient_modes: Vec<InspireRecipientModeRuntimeConfig>,
    pub current_build_lifecycle_authority: bool,
    pub current_build_provider_ownership_authority: bool,
    pub observed_single_stack_no_overlap_authority: bool,
    pub exact_recipient_speed_transition_authority: bool,
    pub exact_native_speed_formula_authority: bool,
    pub exact_action_route_authority: bool,
    pub temporary_speed_term_zero_authority: bool,
    pub damage_resolution_window_accounting_authority: bool,
    pub server_integer_counterfactual_authority: bool,
    pub ordinary_damage_unchanged: bool,
    pub unresolved_overlap_fails_closed: bool,
    pub accounting_method: String,
    pub allocation_order: String,
    pub rational_integer_projection: String,
    pub provider_ownership_proof_sha256: String,
    pub stacking_proof_sha256: String,
    pub damage_time_state_proof_sha256: String,
    pub temporary_lane_proof_sha256: String,
    pub action_timing_ancestry_proof_sha256: String,
    pub action_route_proof_sha256: String,
    pub capacity_proof_sha256: String,
    pub recipient_mode_proof_sha256: String,
    pub exact_build_rlogs: u32,
    pub observed_status_events: u32,
    pub observed_complete_windows: u32,
    pub observed_damage_memberships: u32,
    pub observed_external_damage_memberships: u32,
    pub observed_self_provider_exclusions: u32,
    pub runtime_transfer_enabled: bool,
}

impl InspireRuntimeConfig {
    pub(crate) fn action_lane(
        &self,
        ability_id: i64,
        owner_stage: i32,
    ) -> Option<InspireSpeedLaneRuntimeConfig> {
        self.action_routes
            .binary_search_by_key(&(ability_id, owner_stage), |route| {
                (route.ability_id, route.owner_stage)
            })
            .ok()
            .map(|index| self.action_routes[index].lane)
    }

    pub(crate) fn provider_speed_delta(
        &self,
        class_id: i32,
        specialization_id: i32,
        lane: InspireSpeedLaneRuntimeConfig,
    ) -> Option<i64> {
        let mode = self.recipient_modes.iter().find(|mode| {
            mode.class_id == class_id && mode.specialization_id == specialization_id
        })?;
        Some(match lane {
            InspireSpeedLaneRuntimeConfig::Normal => mode.normal_speed_delta,
            InspireSpeedLaneRuntimeConfig::Guide => mode.guide_speed_delta,
        })
        .filter(|delta| *delta > 0)
    }

    pub(crate) fn speed_attribute_id(&self, lane: InspireSpeedLaneRuntimeConfig) -> i32 {
        match lane {
            InspireSpeedLaneRuntimeConfig::Normal => self.normal_speed_attribute_id,
            InspireSpeedLaneRuntimeConfig::Guide => self.guide_speed_attribute_id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CriticalColdReplayProof {
    pub content_sha256: String,
    pub content_hash_method: String,
    pub content_bytes: u64,
    pub replay_schema_version: u16,
    pub exact_build_rlogs: u32,
    pub total_canonical_events: u64,
    pub runtime_target_match_rlogs: u32,
    pub conserved_rlogs: u32,
    pub rational_projection_overflow_count: u64,
    pub direct_ready_events: u64,
    pub direct_ready_projected_rdmg: i64,
    pub gap_safe_lightfall_conversion_events: u64,
    pub post_tcp_gap_no_conversion_events: u64,
    pub emitted_contribution_events: u64,
    pub emitted_projected_rdmg: i64,
    pub joint_team_luck_emitted_events: u64,
    pub standalone_emitted_events: u64,
    pub deliberate_encore_exclusion_events: u64,
    pub unresolved_stat_resonance_highland_critical_stage_order_events: u64,
    pub team_luck_emitted_events: u64,
    pub team_luck_projected_rdmg: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CriticalColdOfflineOracleProof {
    pub content_sha256: String,
    pub exact_build_rlogs: u32,
    pub eligible_critical_only_samples: u64,
    pub projected_rdmg: i64,
    pub runtime_authority: bool,
    pub authority_boundary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CriticalColdRuntimeConfig {
    pub effect_id: i64,
    pub root_effect_id: i64,
    pub source_talent_id: i64,
    pub official_en_us_source_name: String,
    pub child_en_us_name: Option<String>,
    pub child_design_name: String,
    pub source_type_id: i32,
    pub source_config_id: i64,
    pub required_level: i32,
    pub required_stacks: u32,
    pub critical_chance_attribute_id: i32,
    pub critical_damage_attribute_id: i32,
    pub critical_chance_raw_delta: i64,
    pub critical_chance_cap_raw: i64,
    pub recipient_dependency_effect_id: i64,
    pub recipient_dependency_talent_id: i64,
    pub recipient_dependency_critical_damage_raw_delta: i64,
    pub current_build_lifecycle_authority: bool,
    pub current_build_provider_ownership_authority: bool,
    pub current_build_magnitude_authority: bool,
    pub reuses_inspiration_critical_stage_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub proof_content_sha256: String,
    pub proof_exact_build_rlogs: u32,
    pub proof_status_events: u64,
    pub proof_windows: u64,
    pub proof_external_player_windows: u64,
    pub proof_packet_final_samples: u64,
    pub proof_eligible_critical_only_samples: u64,
    pub replay: CriticalColdReplayProof,
    pub offline_oracle: CriticalColdOfflineOracleProof,
    pub runtime_transfer_enabled: bool,
}

/// Rogue entry 209, Synergy Crit Field. The visible five-second aura refreshes
/// child effect 997538 on recipients inside the field. Runtime occurrence and
/// ownership come only from that observed child status; the installed-game
/// description supplies the exact +3% Crit DMG magnitude and party scope.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SynergyCritFieldRuntimeConfig {
    pub effect_id: i64,
    pub root_effect_id: i64,
    pub aura_effect_id: i64,
    pub source_rogue_entry_id: i64,
    pub description_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub aura_duration_millis: u64,
    pub child_refresh_duration_millis: u64,
    pub aura_radius_meters: u32,
    pub critical_damage_attribute_id: i32,
    pub critical_damage_raw_delta: i64,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_child_status_identity_authority: bool,
    pub additive_critical_stage_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub runtime_transfer_enabled: bool,
}

/// Rogue entry 196, Element Sharing. The owner marker is effect 997512 and
/// the exact ten-second recipient child is 997513. The installed description
/// supplies +20% Elemental Damage and party scope; runtime ownership and
/// occurrence still require the observed child status.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ElementSharingRuntimeConfig {
    pub effect_id: i64,
    pub root_effect_id: i64,
    pub source_rogue_entry_id: i64,
    pub description_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub element_damage_raw_delta: i64,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_child_status_identity_authority: bool,
    pub additive_all_plus_property_stage_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub runtime_transfer_enabled: bool,
}

/// Rogue entry 199, Enhanced Synergy. Effect 997517 is the owner marker and
/// 997518 is the exact three-second recipient child. The installed description
/// supplies +10% PHY Boost and +10% MAG Boost; the packet-synced final boost
/// attributes select the recipient's physical or magical damage bucket.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnhancedSynergyRuntimeConfig {
    pub effect_id: i64,
    pub root_effect_id: i64,
    pub source_rogue_entry_id: i64,
    pub description_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub physical_boost_attribute_id: i32,
    pub magical_boost_attribute_id: i32,
    pub boost_raw_delta: i64,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_child_status_identity_authority: bool,
    pub packet_final_boost_attributes_authority: bool,
    pub corrected_calculator_multiplicative_boost_stage_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub runtime_transfer_enabled: bool,
}

/// Verdant Oracle skill 3401, Blessing. Effect 2100154 is the exact
/// recipient-held party status. The installed description supplies +30%
/// damage for ten seconds; the corrected calculator places it in the
/// additive General Damage bucket, whose packet-final current value is
/// `AttrOtherDamInc` (12670) in ten-thousandths.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlessingRuntimeConfig {
    pub effect_id: i64,
    pub source_skill_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub general_damage_attribute_id: i32,
    pub general_damage_raw_delta: i64,
    current_build_buff_row_fingerprint_sha256: String,
    current_build_fight_attribute_row_fingerprint_sha256: String,
    pinned_calculator_calc_skill_sha256: String,
    pinned_calculator_general_damage_stage_expression: String,
    pub exact_current_build_buff_row_authority: bool,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_current_build_fight_attribute_row_authority: bool,
    pub packet_final_general_damage_attribute_route_authority: bool,
    pub corrected_calculator_additive_general_damage_stage_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_stacking_fails_closed: bool,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub runtime_transfer_enabled: bool,
}

/// Rogue entry 208, Synergy Luck Field. Effect 997533 is the owner marker and
/// 997534 is the exact ten-second recipient aura. The aura grants the
/// Lizardman Hunter Imagine passive; its uniquely linked produced-damage
/// action 3210081 is wholly provider-created only when an exact recipient
/// loadout proves that Imagine was not already equipped.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SynergyLuckFieldRuntimeConfig {
    pub effect_id: i64,
    pub root_effect_id: i64,
    pub source_rogue_entry_id: i64,
    pub description_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub trigger_cooldown_millis: u64,
    pub granted_imagine_ability_id: i64,
    pub granted_imagine_item_id: i64,
    pub granted_passive_effect_id: i64,
    pub produced_damage_ability_id: i64,
    pub produced_damage_attr_id: i64,
    pub base_passive_coefficient_basis_points: i64,
    pub game_description_trigger_and_party_scope_authority: bool,
    pub exact_child_status_identity_authority: bool,
    pub exact_imagine_passive_family_authority: bool,
    pub exact_produced_damage_action_authority: bool,
    pub exact_recipient_loadout_absence_required: bool,
    pub observed_final_direct_output_authority: bool,
    pub accounting_method: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub runtime_transfer_enabled: bool,
}

/// Rogue entry 195, Coordinated Strike. Effect 997510 is the owner marker and
/// 997511 is the exact three-second recipient child. The installed description
/// supplies the +15% Attack magnitude, party scope, trigger cooldown, and
/// duration; attribution still requires the observed child status and the
/// recipient's complete packet-observed Attack family.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CoordinatedStrikeRuntimeConfig {
    pub effect_id: i64,
    pub root_effect_id: i64,
    pub source_rogue_entry_id: i64,
    pub description_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub trigger_cooldown_millis: u64,
    pub attack_raw_percent_delta: i64,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_child_status_identity_authority: bool,
    pub additive_attack_percent_stage_authority: bool,
    pub same_stage_provider_conservation_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub runtime_transfer_enabled: bool,
}

/// Rogue entry 103, All-Class Aura. Effect 998542 is the continuous level-42
/// aura status. Its installed description defines a +5% Attack base plus +5%
/// for each distinct role represented by the observed aura cohort, capped at
/// +20%. Runtime never infers missing cohort members or actor roles.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AllClassAuraRuntimeConfig {
    pub effect_id: i64,
    pub source_rogue_entry_id: i64,
    pub description_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub base_attack_raw_percent_delta: i64,
    pub per_distinct_role_raw_percent_delta: i64,
    pub maximum_attack_raw_percent_delta: i64,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_observed_aura_status_authority: bool,
    pub observed_aura_cohort_role_selector_authority: bool,
    pub additive_attack_percent_stage_authority: bool,
    pub same_stage_provider_conservation_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub runtime_transfer_enabled: bool,
}

/// Rogue entry 197, Attribute Transfer. Effect 997514 is the owner marker and
/// 997515 is the shared ten-second recipient child for Crit, Luck, Haste,
/// Mastery, and Versatility. Because all five branches share one child ID,
/// runtime lane identity must be established by the recipient's exact adjacent
/// final-substat transition; the status alone never guesses a lane.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttributeTransferRuntimeConfig {
    pub effect_id: i64,
    pub root_effect_id: i64,
    pub source_rogue_entry_id: i64,
    pub description_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub substat_raw_delta: i64,
    pub versatility_to_external_damage_numerator: i64,
    pub versatility_to_external_damage_denominator: i64,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_child_status_identity_authority: bool,
    pub exact_adjacent_lane_transition_authority: bool,
    pub corrected_calculator_final_substat_stage_authority: bool,
    pub corrected_calculator_versatility_stage_authority: bool,
    pub reuses_inspiration_chance_stage_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub critical_chance_runtime_transfer_enabled: bool,
    pub lucky_chance_runtime_transfer_enabled: bool,
    pub versatility_runtime_transfer_enabled: bool,
    pub mastery_runtime_transfer_enabled: bool,
    pub haste_runtime_transfer_enabled: bool,
    pub runtime_transfer_enabled: bool,
}

/// Module part 2404, Life Wave. The provider is the exact HP/max-HP changer
/// paired with child 2302421; the recipient's adjacent attribute vector proves
/// both the calculator-selected lane and any build-specific derived factor.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LifeWaveRuntimeConfig {
    pub effect_id: i64,
    pub source_type_id: i32,
    pub source_config_id: i64,
    pub module_effect_id: i32,
    pub duration_millis: u64,
    pub level_five_bonus_basis_points: i64,
    pub level_six_bonus_basis_points: i64,
    pub versatility_to_external_damage_numerator: i64,
    pub versatility_to_external_damage_denominator: i64,
    pub calculator_lane_selection_authority: bool,
    pub exact_module_profile_magnitude_authority: bool,
    pub exact_child_status_identity_authority: bool,
    pub cross_vantage_trigger_ownership_authority: bool,
    pub exact_adjacent_lane_transition_authority: bool,
    pub packet_final_counterfactual_authority: bool,
    pub reviewed_action_route_required: bool,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub runtime_transfer_enabled: bool,
}

/// Rogue entry 349, Tactical Blessing. Root effect 997557 makes Pulse Beam
/// grant the exact ten-second recipient child 997570 to friendly units. The
/// installed description supplies simultaneous +10% Crit and +10% Luck;
/// packet-final chance and damage attributes close the occurrence-stage
/// counterfactual without requiring a newly observed session.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalBlessingRuntimeConfig {
    pub effect_id: i64,
    pub root_effect_id: i64,
    pub source_rogue_entry_id: i64,
    pub description_id: i64,
    pub official_en_us_name: String,
    pub required_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub critical_chance_raw_delta: i64,
    pub lucky_chance_raw_delta: i64,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_child_status_identity_authority: bool,
    pub exact_static_lifecycle_authority: bool,
    pub corrected_calculator_final_substat_stage_authority: bool,
    pub reuses_inspiration_chance_stage_authority: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub runtime_transfer_enabled: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FixedPointFamilyRuntimeConfig {
    pub current_attribute_id: i32,
    pub total_attribute_id: i32,
    pub add_attribute_id: i32,
    pub extra_add_attribute_id: i32,
    pub percent_attribute_id: i32,
    pub extra_percent_attribute_id: i32,
}

impl FixedPointFamilyRuntimeConfig {
    fn is_valid(&self) -> bool {
        let ids = [
            self.current_attribute_id,
            self.total_attribute_id,
            self.add_attribute_id,
            self.extra_add_attribute_id,
            self.percent_attribute_id,
            self.extra_percent_attribute_id,
        ];
        ids.iter().all(|value| *value > 0)
            && ids
                .iter()
                .enumerate()
                .all(|(index, value)| !ids[..index].contains(value))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HighlandBloodRuntimeConfig {
    pub effect_id: i64,
    pub provider_imagine_ability_id: i64,
    pub provider_marker_effect_id: i64,
    pub companion_lockout_effect_id: i64,
    pub all_element_family: FixedPointFamilyRuntimeConfig,
    pub duration_millis: u64,
    pub lockout_duration_millis: u64,
    pub packet_proven_raw_deltas: Vec<i64>,
    pub excluded_provider_owned_damage_ids: Vec<i64>,
    pub requires_recipient_packet_transition: bool,
    pub runtime_transfer_enabled: bool,
    pub remote_paired_output_runtime_transfer_enabled: bool,
    pub remote_paired_output_ignored_effect_ids: Vec<i64>,
    pub remote_paired_output_max_pair_gap_micros: u64,
    pub remote_paired_output_min_distinct_targets: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndlessMindRuntimeConfig {
    pub effect_id: i64,
    pub source_type_id: i32,
    pub source_config_id: i64,
    pub required_level: i32,
    pub minimum_stacks: u32,
    pub maximum_stacks: u32,
    pub mastery_attribute_id: i32,
    pub mastery_basis_points_per_stack: i64,
    pub shattered_illusion_ability_id: i64,
    pub shattered_illusion_hit_event_id: i32,
    pub shattered_illusion_damage_attr_id: i64,
    pub mastery_to_element_numerator: i64,
    pub mastery_to_element_denominator: i64,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_action_identity_authority: bool,
    pub observed_final_proportional_authority: bool,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub runtime_transfer_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArcaneTimeDecreeRuntimeConfig {
    pub effect_id: i64,
    pub provider_imagine_ability_id: i64,
    pub provider_imagine_item_id: i64,
    pub required_effect_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub cooldown_acceleration_basis_points_by_tier: Vec<i64>,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub canonical_cooldown_acceleration_field_authority: bool,
    pub exact_cooldown_action_identity_required: bool,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub runtime_transfer_enabled: bool,
}

impl ArcaneTimeDecreeRuntimeConfig {
    pub(crate) fn basis_points_for_tier(&self, tier: u32) -> Option<i64> {
        usize::try_from(tier.checked_sub(1)?)
            .ok()
            .and_then(|index| self.cooldown_acceleration_basis_points_by_tier.get(index))
            .copied()
    }
}

/// Whole packet-final produced damage created by Arcane! Thunder Roar's
/// recipient-held Electro Shield (2110096).
///
/// The game description identifies the 0.5-second recipient-triggered
/// Thunderstrike, while DamageAttrTable binds that output to action
/// 2110096:3 / row 2211009603. Because the action would not exist without the
/// external provider, its complete observed integer is the provider marginal;
/// the tier coefficient is retained for audit/versioning but is not used to
/// reverse an already-final packet result.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThunderRoarRuntimeConfig {
    pub effect_id: i64,
    pub required_effect_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub trigger_cooldown_millis: u64,
    pub thunderstrike_ability_id: i64,
    pub thunderstrike_hit_event_id: i32,
    pub thunderstrike_damage_attr_id: i64,
    pub thunderstrike_coefficient_basis_points_by_tier: Vec<i64>,
    pub thunderstrike_fixed_parameter: i64,
    pub excluded_placeholder_damage_attr_id: i64,
    pub excluded_direct_cast_damage_attr_id: i64,
    pub game_description_trigger_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_damage_attr_link_authority: bool,
    pub source_owner_ancestry_required: bool,
    pub observed_final_direct_output_authority: bool,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub accounting_method: String,
    pub runtime_transfer_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PoisonExplosionVulnerabilityRuntimeConfig {
    pub effect_id: i64,
    pub provider_imagine_ability_id: i64,
    pub provider_imagine_item_id: i64,
    pub required_effect_level: i32,
    pub minimum_stacks: u32,
    pub maximum_stacks: u32,
    pub duration_millis: u64,
    pub vulnerability_basis_points_per_stack_by_tier: Vec<i64>,
    pub conflicting_target_effect_ids: Vec<i64>,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_target_status_identity_authority: bool,
    pub exact_static_lifecycle_authority: bool,
    pub provider_loadout_tier_authority: bool,
    pub additive_vulnerability_stage_authority: bool,
    pub same_stage_provider_conservation_authority: bool,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub runtime_transfer_enabled: bool,
}

impl PoisonExplosionVulnerabilityRuntimeConfig {
    pub(crate) fn basis_points_per_stack_for_tier(&self, tier: u32) -> Option<i64> {
        usize::try_from(tier.checked_sub(1)?)
            .ok()
            .and_then(|index| self.vulnerability_basis_points_per_stack_by_tier.get(index))
            .copied()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CelestialGuardianVulnerabilityRuntimeConfig {
    pub effect_id: i64,
    pub provider_imagine_ability_id: i64,
    pub provider_imagine_item_id: i64,
    pub required_effect_level: i32,
    pub required_stacks: u32,
    pub duration_millis: u64,
    pub vulnerability_basis_points_by_tier: Vec<i64>,
    pub conflicting_target_effect_ids: Vec<i64>,
    pub game_description_formula_authority: bool,
    pub game_description_party_scope_authority: bool,
    pub exact_target_status_identity_authority: bool,
    pub exact_static_lifecycle_authority: bool,
    pub provider_loadout_tier_authority: bool,
    pub additive_vulnerability_stage_authority: bool,
    pub later_stage_cancellation_authority: bool,
    pub element_resistance_component_transfer_enabled: bool,
    pub same_stage_provider_conservation_authority: bool,
    pub unresolved_overlap_fails_closed: bool,
    pub ordinary_damage_unchanged: bool,
    pub accounting_method: String,
    pub rational_integer_projection: String,
    pub runtime_transfer_enabled: bool,
}

impl CelestialGuardianVulnerabilityRuntimeConfig {
    pub(crate) fn basis_points_for_tier(&self, tier: u32) -> Option<i64> {
        usize::try_from(tier.checked_sub(1)?)
            .ok()
            .and_then(|index| self.vulnerability_basis_points_by_tier.get(index))
            .copied()
    }
}

impl ThunderRoarRuntimeConfig {
    pub(crate) fn is_thunderstrike_action(
        &self,
        ability_id: Option<i64>,
        hit_event_id: Option<i32>,
    ) -> bool {
        ability_id == Some(self.thunderstrike_ability_id)
            && hit_event_id == Some(self.thunderstrike_hit_event_id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetVulnerabilityRuntimeConfig {
    current_build_lifecycle_authority: bool,
    current_build_formula_authority: bool,
    server_integer_counterfactual_authority: bool,
    formula_specific_conservation_authority: bool,
    unresolved_overlap_fails_closed: bool,
    runtime_transfer_effect_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsPromotionInventoryPolicy {
    ordinary_damage_and_dps_unchanged: bool,
    unknown_and_unresolved_events_retained: bool,
    candidate_effects_grant_provider_credit: bool,
    production_effect_ids_are_sorted_and_unique: bool,
    complete_localized_names_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsPromotionReviewCoverage {
    consolidated_unique_effect_ids: usize,
    exact_id_route_rows: usize,
    exact_id_route_unique_ids: usize,
    zero_effect_rows_without_disposition: bool,
    zero_exact_id_route_rows_without_disposition: bool,
    exhaustive_ledger_content_sha256: String,
    ledger_production_effect_ids: usize,
    post_ledger_production_effect_ids: Vec<i64>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RdpsPromotionEffect {
    effect_id: i64,
    full_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsRemainingCandidate {
    effect_id: i64,
    full_name: String,
    disposition: String,
    remaining_proof_obligation: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsPromotionInventory {
    schema_version: u16,
    deployment_id: String,
    game_build: String,
    policy: RdpsPromotionInventoryPolicy,
    review_coverage: RdpsPromotionReviewCoverage,
    production_effects: Vec<RdpsPromotionEffect>,
    remaining_candidates: Vec<RdpsRemainingCandidate>,
}

#[derive(Debug, Deserialize)]
struct RdpsAttributionEffectPresentation {
    schema_version: u16,
    deployment_id: String,
    game_build: String,
    locale: String,
    effects: Vec<RdpsAttributionPresentedEffect>,
}

#[derive(Debug, Deserialize)]
struct RdpsAttributionPresentedEffect {
    effect_id: i64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ExternalStateRdpsInventory {
    schema_version: u16,
    game_build: String,
    rules: Vec<ExternalStateRdpsInventoryRule>,
}

#[derive(Debug, Deserialize)]
struct ExternalStateRdpsInventoryRule {
    effect_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RdpsRuntimeConfig {
    schema_version: u16,
    pub deployment_id: String,
    pub game_build: String,
    /// Exact decoder/protocol identity. A build number alone is insufficient:
    /// two packs for the same client build can decode different event fields.
    pub protocol_pack_digest: String,
    promotion_state: String,
    promotion_blockers: Vec<String>,
    policy: RdpsRuntimePolicy,
    pub attack_families: AttackFamiliesRuntimeConfig,
    pub damage_stage_catalog: DamageStageCatalogRuntimeConfig,
    pub critical_damage_factor_interpretation: CriticalDamageFactorInterpretation,
    /// Independently reviewed target-vulnerability authority. This remains
    /// component-scoped so one exact effect/action can ship without granting
    /// authority to unrelated rDPS candidates.
    target_vulnerability: TargetVulnerabilityRuntimeConfig,
    pub team_luck: TeamLuckRuntimeConfig,
    pub functional_amp: FunctionalAmpRuntimeConfig,
    pub mechanical_power: MechanicalPowerRuntimeConfig,
    pub harmony_grace: HarmonyGraceRuntimeConfig,
    pub stat_resonance: StatResonanceRuntimeConfig,
    pub fiery_battle_will: FieryBattleWillRuntimeConfig,
    pub encore: EncoreDirectOutputRuntimeConfig,
    pub thunderwind: ThunderwindRuntimeConfig,
    pub inspiration: InspirationRuntimeConfig,
    pub inspire: InspireRuntimeConfig,
    pub critical_cold: CriticalColdRuntimeConfig,
    pub synergy_crit_field: SynergyCritFieldRuntimeConfig,
    pub element_sharing: ElementSharingRuntimeConfig,
    pub enhanced_synergy: EnhancedSynergyRuntimeConfig,
    pub blessing: BlessingRuntimeConfig,
    pub synergy_luck_field: SynergyLuckFieldRuntimeConfig,
    pub coordinated_strike: CoordinatedStrikeRuntimeConfig,
    pub all_class_aura: AllClassAuraRuntimeConfig,
    pub attribute_transfer: AttributeTransferRuntimeConfig,
    pub life_wave: LifeWaveRuntimeConfig,
    pub tactical_blessing: TacticalBlessingRuntimeConfig,
    pub endless_mind: EndlessMindRuntimeConfig,
    pub arcane_time_decree: ArcaneTimeDecreeRuntimeConfig,
    pub thunder_roar: ThunderRoarRuntimeConfig,
    pub poison_explosion_vulnerability: PoisonExplosionVulnerabilityRuntimeConfig,
    pub celestial_guardian_vulnerability: CelestialGuardianVulnerabilityRuntimeConfig,
    pub highland_blood: HighlandBloodRuntimeConfig,
}

impl RdpsRuntimeConfig {
    pub(crate) fn runtime_promotion_allowed(&self) -> bool {
        self.policy.runtime_promotion_allowed
    }

    pub(crate) fn encore_runtime_transfer_enabled(&self) -> bool {
        self.encore.runtime_transfer_enabled && self.game_build == "24687926"
    }

    /// Whether this exact build has at least one production attribution path.
    /// The global gate remains the all-frontier approval; component gates let
    /// independently closed effects ship one at a time.
    pub(crate) fn has_any_runtime_transfer_enabled(&self) -> bool {
        self.runtime_promotion_allowed()
            || !self
                .target_vulnerability
                .runtime_transfer_effect_ids
                .is_empty()
            || self.functional_amp.attack_magic_runtime_transfer_enabled
            || self.team_luck.critical_damage_runtime_transfer_enabled
            || self.team_luck.lucky_damage_runtime_transfer_enabled
            || self.mechanical_power.runtime_transfer_enabled
            || self.harmony_grace.runtime_transfer_enabled
            || self
                .harmony_grace
                .remote_paired_output_runtime_transfer_enabled
            || self.stat_resonance.runtime_transfer_enabled
            || self.fiery_battle_will.runtime_transfer_enabled
            || self.encore_runtime_transfer_enabled()
            || self.highland_blood.runtime_transfer_enabled
            || self
                .highland_blood
                .remote_paired_output_runtime_transfer_enabled
            || self.inspiration.critical_chance_runtime_transfer_enabled
            || self.inspiration.lucky_chance_runtime_transfer_enabled
            || self
                .inspiration
                .full_bloom_increment_runtime_transfer_enabled
            || self.inspire.runtime_transfer_enabled
            || self.critical_cold.runtime_transfer_enabled
            || self.synergy_crit_field.runtime_transfer_enabled
            || self.element_sharing.runtime_transfer_enabled
            || self.enhanced_synergy.runtime_transfer_enabled
            || self.blessing.runtime_transfer_enabled
            || self.synergy_luck_field.runtime_transfer_enabled
            || self.coordinated_strike.runtime_transfer_enabled
            || self.all_class_aura.runtime_transfer_enabled
            || self.attribute_transfer.runtime_transfer_enabled
            || self.life_wave.runtime_transfer_enabled
            || self.tactical_blessing.runtime_transfer_enabled
            || self.endless_mind.runtime_transfer_enabled
            || self.arcane_time_decree.runtime_transfer_enabled
            || self.thunder_roar.runtime_transfer_enabled
            || self.poison_explosion_vulnerability.runtime_transfer_enabled
            || self
                .celestial_guardian_vulnerability
                .runtime_transfer_enabled
    }

    /// Effect-scoped production authority. A false result never hides the
    /// canonical event or ordinary damage; it only blocks provider transfer.
    pub(crate) fn effect_runtime_transfer_enabled(&self, effect_id: i64) -> bool {
        if effect_id == self.thunderwind.effect_id || effect_id == self.thunderwind.child_effect_id
        {
            return false;
        }
        self.runtime_promotion_allowed()
            || self
                .target_vulnerability
                .runtime_transfer_effect_ids
                .contains(&effect_id)
            || (effect_id == self.functional_amp.effect_id
                && self.functional_amp.attack_magic_runtime_transfer_enabled)
            || (effect_id == self.team_luck.effect_id
                && (self.team_luck.critical_damage_runtime_transfer_enabled
                    || self.team_luck.lucky_damage_runtime_transfer_enabled))
            || (effect_id == self.mechanical_power.effect_id
                && self.mechanical_power.runtime_transfer_enabled)
            || (effect_id == self.harmony_grace.effect_id
                && (self.harmony_grace.runtime_transfer_enabled
                    || self
                        .harmony_grace
                        .remote_paired_output_runtime_transfer_enabled))
            || (effect_id == self.stat_resonance.effect_id
                && self.stat_resonance.runtime_transfer_enabled)
            || (effect_id == self.fiery_battle_will.effect_id
                && self.fiery_battle_will.runtime_transfer_enabled)
            || (effect_id == self.encore.effect_id && self.encore_runtime_transfer_enabled())
            || (effect_id == self.highland_blood.effect_id
                && (self.highland_blood.runtime_transfer_enabled
                    || self
                        .highland_blood
                        .remote_paired_output_runtime_transfer_enabled))
            || (effect_id == self.inspiration.effect_id
                && (self.inspiration.critical_chance_runtime_transfer_enabled
                    || self.inspiration.lucky_chance_runtime_transfer_enabled))
            || (effect_id == self.inspiration.full_bloom_effect_id
                && self
                    .inspiration
                    .full_bloom_increment_runtime_transfer_enabled)
            || (effect_id == self.inspire.effect_id && self.inspire.runtime_transfer_enabled)
            || (effect_id == self.critical_cold.effect_id
                && self.critical_cold.runtime_transfer_enabled)
            || (effect_id == self.synergy_crit_field.effect_id
                && self.synergy_crit_field.runtime_transfer_enabled)
            || (effect_id == self.element_sharing.effect_id
                && self.element_sharing.runtime_transfer_enabled)
            || (effect_id == self.enhanced_synergy.effect_id
                && self.enhanced_synergy.runtime_transfer_enabled)
            || (effect_id == self.blessing.effect_id && self.blessing.runtime_transfer_enabled)
            || (effect_id == self.synergy_luck_field.effect_id
                && self.synergy_luck_field.runtime_transfer_enabled)
            || (effect_id == self.coordinated_strike.effect_id
                && self.coordinated_strike.runtime_transfer_enabled)
            || (effect_id == self.all_class_aura.effect_id
                && self.all_class_aura.runtime_transfer_enabled)
            || (effect_id == self.attribute_transfer.effect_id
                && self.attribute_transfer.runtime_transfer_enabled)
            || (effect_id == self.life_wave.effect_id && self.life_wave.runtime_transfer_enabled)
            || (effect_id == self.tactical_blessing.effect_id
                && self.tactical_blessing.runtime_transfer_enabled)
            || (effect_id == self.endless_mind.effect_id
                && self.endless_mind.runtime_transfer_enabled)
            || (effect_id == self.arcane_time_decree.effect_id
                && self.arcane_time_decree.runtime_transfer_enabled)
            || (effect_id == self.thunder_roar.effect_id
                && self.thunder_roar.runtime_transfer_enabled)
            || (effect_id == self.poison_explosion_vulnerability.effect_id
                && self.poison_explosion_vulnerability.runtime_transfer_enabled)
            || (effect_id == self.celestial_guardian_vulnerability.effect_id
                && self
                    .celestial_guardian_vulnerability
                    .runtime_transfer_enabled)
    }

    pub(crate) fn target_vulnerability_runtime_transfer_effect_ids(&self) -> &[i64] {
        &self.target_vulnerability.runtime_transfer_effect_ids
    }

    fn requires_exact_build(&self) -> bool {
        self.policy.same_deployment_build_mismatch == "exact-build-only"
    }

    pub(crate) fn warns_on_build_mismatch(&self) -> bool {
        self.policy.warn_on_build_mismatch
    }

    pub(crate) fn promotion_blockers(&self) -> &[String] {
        &self.promotion_blockers
    }

    pub(crate) fn promotion_blocker_status_detail(&self) -> String {
        self.promotion_blockers().join(",")
    }

    fn validate(&self) -> Result<(), String> {
        let promotion_state_is_consistent = match self.promotion_state.as_str() {
            "approved" => self.runtime_promotion_allowed(),
            "blocked-current-build-proof-gates-open" => !self.runtime_promotion_allowed(),
            _ => false,
        };
        let critical_damage_interpretation_is_consistent = match (
            self.policy.critical_damage_factor_interpretation_authority,
            self.critical_damage_factor_interpretation,
        ) {
            (false, CriticalDamageFactorInterpretation::Unresolved) => true,
            (true, interpretation) => interpretation.is_resolved(),
            _ => false,
        };
        let promotion_blockers_are_consistent =
            self.promotion_blockers
                .iter()
                .enumerate()
                .all(|(index, blocker)| {
                    KNOWN_PROMOTION_BLOCKERS.contains(&blocker.as_str())
                        && !self.promotion_blockers[..index].contains(blocker)
                })
                && (self.runtime_promotion_allowed() == self.promotion_blockers.is_empty())
                && (self.policy.critical_damage_factor_interpretation_authority
                    != self.promotion_blockers.iter().any(|blocker| {
                        blocker == "critical-damage-factor-interpretation-authority"
                    }))
                && (self.policy.party_support_formula_frontier_complete
                    != self
                        .promotion_blockers
                        .iter()
                        .any(|blocker| blocker == "party-support-formula-frontier"));
        let target_vulnerability_runtime_effects_are_valid = self
            .target_vulnerability
            .runtime_transfer_effect_ids
            .iter()
            .enumerate()
            .all(|(index, effect_id)| {
                *effect_id > 0
                    && !self.target_vulnerability.runtime_transfer_effect_ids[..index]
                        .contains(effect_id)
            });
        let target_vulnerability_runtime_authority =
            self.target_vulnerability.current_build_lifecycle_authority
                && self.target_vulnerability.current_build_formula_authority
                && self
                    .target_vulnerability
                    .server_integer_counterfactual_authority
                && self
                    .target_vulnerability
                    .formula_specific_conservation_authority
                && self.target_vulnerability.unresolved_overlap_fails_closed;
        if self.schema_version != RDPS_RUNTIME_SCHEMA_VERSION
            || self.deployment_id != "global"
            || self.game_build.is_empty()
            || numeric_build(&self.game_build) == 0
            || !is_prefixed_sha256(&self.protocol_pack_digest)
            || !promotion_state_is_consistent
            || !self.policy.canonical_events_retained
            || self.policy.unresolved_events_hidden
            || self.policy.candidate_rules_enabled
            || !critical_damage_interpretation_is_consistent
            || !promotion_blockers_are_consistent
            || (self.runtime_promotion_allowed()
                && !self.policy.critical_damage_factor_interpretation_authority)
            || !self.policy.promotion_requires_exact_conservation
            || !self
                .policy
                .game_files_are_identity_and_coefficient_evidence_not_packet_occurrence_evidence
            || !self.requires_exact_build()
            || !self.warns_on_build_mismatch()
            || !target_vulnerability_runtime_effects_are_valid
            || (!self
                .target_vulnerability
                .runtime_transfer_effect_ids
                .is_empty()
                && !target_vulnerability_runtime_authority)
        {
            return Err("bundled BPSR rDPS formula policy is not approved and fail closed".into());
        }

        let attack_families = &self.attack_families;
        if !attack_families.physical.is_valid()
            || !attack_families.magical.is_valid()
            || attack_families
                .physical
                .attribute_ids()
                .iter()
                .any(|value| attack_families.magical.attribute_ids().contains(value))
        {
            return Err("bundled BPSR attack attribute families are invalid".into());
        }

        let catalog = &self.damage_stage_catalog;
        if catalog.authored_game_build.is_empty()
            || numeric_build(&catalog.authored_game_build) == 0
            || catalog.source_table != "DamageAttrTable.ctb"
            || catalog.source_table_hash == 0
            || catalog.source_row_count == 0
            || catalog.unique_lookup_keys == 0
            || catalog.unique_lookup_keys + catalog.ambiguous_lookup_keys > catalog.source_row_count
            || catalog.standard_attack_rules + catalog.standard_magic_attack_rules
                != catalog.standard_rules
            || catalog.standard_rules == 0
        {
            return Err("bundled BPSR damage-stage catalog identity is invalid".into());
        }

        let team_luck = &self.team_luck;
        let team_luck_routes_are_valid =
            team_luck
                .lucky_damage_routes
                .iter()
                .enumerate()
                .all(|(index, route)| {
                    route.ability_id > 0
                        && route.hit_event_id >= 0
                        && !team_luck.lucky_damage_routes[..index].contains(route)
                });
        let team_luck_critical_runtime_authority = self
            .policy
            .critical_damage_factor_interpretation_authority
            && self.critical_damage_factor_interpretation
                == CriticalDamageFactorInterpretation::AdditiveBonus
            && team_luck.critical_damage_current_build_lifecycle_authority
            && team_luck.critical_damage_current_build_executor_authority
            && team_luck.critical_damage_exact_rational_attribution_authority
            && team_luck.critical_damage_protocol_pack_migration_authority
            && team_luck.critical_damage_authorized_protocol_pack_digests
                == [
                    "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
                    "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395",
                    "sha256:f975b4acade288bc87392bfeaae464873f7af1d3060be56023ff69d176905a3e",
                    "sha256:f4eb9db52ee232ecc7845119cb7fd909fb0f2c2d4fee33fe587b4235b656773c",
                    "sha256:58c849d0264261efe8220b7dd5ce50fd7e3f8fa31980941e823a18306f30c7d1",
                    "sha256:9de9c7eccc5309686ad4e982968aef67c1d6cf6f59e71762c457ce8ce8f23ac3",
                    "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
                ]
            && team_luck
                .critical_damage_authorized_protocol_pack_digests
                .contains(&self.protocol_pack_digest)
            && team_luck.critical_damage_formula_authority_basis
                == "current-build-strict-normal-vs-critical-ratio-replay-plus-target-pack-decoder-contract-migration"
            && team_luck.critical_damage_ratio_proof.is_valid()
            && team_luck.accounting_method == "observed-final-damage-proportional-stage-share"
            && !team_luck.server_integer_counterfactual_authority
            && team_luck.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && team_luck.unresolved_overlap_fails_closed;
        let team_luck_lucky_runtime_authority = team_luck
            .lucky_damage_current_build_lifecycle_authority
            && team_luck.lucky_damage_current_build_executor_authority
            && team_luck.lucky_damage_exact_rational_attribution_authority
            && team_luck.lucky_damage_protocol_pack_migration_authority
            && team_luck.lucky_damage_authorized_protocol_pack_digests
                == [
                    "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
                    "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395",
                    "sha256:f975b4acade288bc87392bfeaae464873f7af1d3060be56023ff69d176905a3e",
                    "sha256:f4eb9db52ee232ecc7845119cb7fd909fb0f2c2d4fee33fe587b4235b656773c",
                    "sha256:58c849d0264261efe8220b7dd5ce50fd7e3f8fa31980941e823a18306f30c7d1",
                    "sha256:9de9c7eccc5309686ad4e982968aef67c1d6cf6f59e71762c457ce8ce8f23ac3",
                    "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
                ]
            && team_luck
                .lucky_damage_authorized_protocol_pack_digests
                .contains(&self.protocol_pack_digest)
            && team_luck.formula_authority_basis
                == "current-build-prior-pack-replay-plus-target-pack-decoder-contract-migration"
            && team_luck.accounting_method == "observed-final-damage-proportional-stage-share"
            && !team_luck.server_integer_counterfactual_authority
            && team_luck.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && team_luck.unresolved_overlap_fails_closed;
        let team_luck_combined_runtime_authority = team_luck_critical_runtime_authority
            && team_luck_lucky_runtime_authority
            && team_luck.combined_damage_current_build_packet_component_authority
            && team_luck.combined_damage_exact_rational_cross_term_authority
            && team_luck.combined_damage_protocol_pack_migration_authority
            && team_luck.combined_damage_formula_authority_basis
                == "current-build-dedicated-lucky-component-plus-observed-final-rational-cross-term"
            && team_luck.combined_damage_proof.is_valid();
        if team_luck.effect_id <= 0
            || team_luck.source_type_id <= 0
            || team_luck.source_config_id <= 0
            || team_luck.critical_damage_attribute_id <= 0
            || team_luck.lucky_damage_attribute_id <= 0
            || team_luck.critical_damage_attribute_id == team_luck.lucky_damage_attribute_id
            || team_luck.critical_raw_delta <= 0
            || team_luck.lucky_raw_delta <= 0
            || team_luck.combined_critical_lucky_enabled != team_luck_combined_runtime_authority
            || !team_luck_routes_are_valid
            || (team_luck.critical_damage_runtime_transfer_enabled
                && !team_luck_critical_runtime_authority)
            || (!team_luck.critical_damage_runtime_transfer_enabled
                && (team_luck.critical_damage_current_build_lifecycle_authority
                    || team_luck.critical_damage_current_build_executor_authority
                    || team_luck.critical_damage_exact_rational_attribution_authority
                    || team_luck.critical_damage_protocol_pack_migration_authority
                    || !team_luck
                        .critical_damage_authorized_protocol_pack_digests
                        .is_empty()))
            || (team_luck.lucky_damage_runtime_transfer_enabled
                && (!team_luck_lucky_runtime_authority
                    || team_luck.lucky_damage_routes != TEAM_LUCK_CURRENT_LUCKY_DAMAGE_ROUTES))
            || (!team_luck.lucky_damage_runtime_transfer_enabled
                && !team_luck.lucky_damage_routes.is_empty())
        {
            return Err("bundled BPSR Team Luck formula is invalid".into());
        }

        let amp = &self.functional_amp;
        let amp_effects = [
            amp.effect_id,
            amp.source_config_id,
            amp.self_multiplier_effect_id,
            amp.passive_damage_effect_id,
            amp.passive_stack_effect_id,
        ];
        if amp_effects.iter().any(|value| *value <= 0)
            || amp_effects
                .iter()
                .enumerate()
                .any(|(index, value)| amp_effects[..index].contains(value))
            || amp.attack_percent_raw_delta <= 0
            || amp.damage_scripts != ["Attack", "MAttack"]
            || !amp.attack_magic_historical_formula_authority
            || !amp.attack_magic_target_build_lifecycle_schema_authority
            || !amp.attack_magic_target_build_formula_authority
            || !amp.dormant_activation_requires_observed_lifecycle
            || amp.accounting_method != "observed-final-damage-proportional-stage-share"
            || amp.server_integer_counterfactual_authority
            || amp.rational_integer_projection
                != "sum-exact-then-half-up-per-effect-provider-recipient"
            || !amp.unresolved_overlap_fails_closed
            || amp.speed_runtime_transfer_enabled
            || (amp.formula_authority_basis == "target-build-direct-packet-replay"
                && (!amp.attack_magic_target_build_effect_occurrence_observed
                    || !amp.attack_magic_target_build_damage_replay_observed))
            || (amp.formula_authority_basis != "target-build-direct-packet-replay"
                && amp.formula_authority_basis
                    != "historical-replay-plus-target-static-and-protocol-schema-migration")
            || (amp.attack_magic_runtime_transfer_enabled
                && (!amp.attack_magic_target_build_lifecycle_schema_authority
                    || !amp.attack_magic_target_build_formula_authority
                    || !amp.dormant_activation_requires_observed_lifecycle))
        {
            return Err("bundled BPSR Functional Amp formula is invalid".into());
        }

        let mechanical = &self.mechanical_power;
        let mechanical_runtime_authority = mechanical
            .class_11_tier_0_current_pack_lifecycle_authority
            && mechanical.class_11_tier_0_exact_rational_attribution_authority
            && mechanical.replay.is_current_authority()
            && mechanical.accounting_method == "observed-final-damage-proportional-stage-share"
            && !mechanical.damage_stage_operation_order_authority
            && !mechanical.damage_stage_integer_rounding_authority
            && !mechanical.server_integer_counterfactual_authority
            && mechanical.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && mechanical.unresolved_overlap_fails_closed;
        let mechanical_runtime_classes_are_valid = mechanical
            .runtime_recipient_class_ids
            .iter()
            .enumerate()
            .all(|(index, class_id)| {
                *class_id > 0
                    && !mechanical.runtime_recipient_class_ids[..index].contains(class_id)
                    && mechanical
                        .recipient_rules
                        .iter()
                        .any(|rule| rule.recipient_class_id == *class_id)
            });
        let mechanical_runtime_deltas_are_valid = mechanical
            .runtime_primary_percent_raw_deltas
            .iter()
            .enumerate()
            .all(|(index, delta)| {
                *delta > 0
                    && !mechanical.runtime_primary_percent_raw_deltas[..index].contains(delta)
            });
        if mechanical.effect_id <= 0
            || mechanical.source_config_id <= 0
            || !mechanical.source_config_must_be_absent
            || mechanical.duration_millis == 0
            || mechanical.haste_attribute_id <= 0
            || !rules_are_valid_and_unique(&mechanical.recipient_rules)
            || !mechanical.requires_recipient_packet_transition
            || !mechanical.haste_is_action_opportunity
            || mechanical.universal_tier_formula_enabled
            || mechanical.accounting_method != "observed-final-damage-proportional-stage-share"
            || mechanical.damage_stage_operation_order_authority
            || mechanical.damage_stage_integer_rounding_authority
            || mechanical.server_integer_counterfactual_authority
            || mechanical.rational_integer_projection
                != "sum-exact-then-half-up-per-effect-provider-recipient"
            || !mechanical.unresolved_overlap_fails_closed
            || !mechanical_runtime_classes_are_valid
            || !mechanical_runtime_deltas_are_valid
            || (mechanical.runtime_transfer_enabled
                && (!mechanical_runtime_authority
                    || mechanical.runtime_recipient_class_ids != [11]
                    || mechanical.runtime_primary_percent_raw_deltas != [750]))
            || (!mechanical.runtime_transfer_enabled
                && (!mechanical.runtime_recipient_class_ids.is_empty()
                    || !mechanical.runtime_primary_percent_raw_deltas.is_empty()))
        {
            return Err(
                "bundled BPSR Mechanical Power formula is not ready for runtime transfer".into(),
            );
        }

        let harmony = &self.harmony_grace;
        let harmony_runtime_authority = harmony.class_11_current_pack_lifecycle_authority
            && harmony.class_11_exact_rational_attribution_authority
            && harmony.direct_replay.is_current_authority()
            && harmony.accounting_method == "observed-final-damage-proportional-stage-share"
            && !harmony.server_integer_counterfactual_authority
            && harmony.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && harmony.unresolved_overlap_fails_closed;
        let harmony_runtime_classes_are_valid = harmony
            .runtime_recipient_class_ids
            .iter()
            .enumerate()
            .all(|(index, class_id)| {
                *class_id > 0
                    && !harmony.runtime_recipient_class_ids[..index].contains(class_id)
                    && harmony
                        .recipient_rules
                        .iter()
                        .any(|rule| rule.recipient_class_id == *class_id)
            });
        let harmony_remote_paired_output_evidence_is_valid = harmony.remote_paired_output_ignored_effect_ids == [27_016, 55_301, 3_002_011]
                // The sealed receipt covered 583 current-build formula
                // effects. Coordinated Strike, Element Sharing, Attribute
                // Transfer, Enhanced Synergy, Synergy Luck Field, and Tactical
                // Blessing are appended as
                // stronger context dimensions:
                // pair matching now fails when either lifecycle differs, while
                // the checksum below continues to authenticate the original set.
                && harmony.remote_paired_output_formula_effect_ids.len() == 589
                && harmony
                    .remote_paired_output_formula_effect_ids
                    .binary_search(&self.coordinated_strike.effect_id)
                    .is_ok()
                && harmony
                    .remote_paired_output_formula_effect_ids
                    .binary_search(&self.element_sharing.effect_id)
                    .is_ok()
                && harmony
                    .remote_paired_output_formula_effect_ids
                    .binary_search(&self.attribute_transfer.effect_id)
                    .is_ok()
                && harmony
                    .remote_paired_output_formula_effect_ids
                    .binary_search(&self.enhanced_synergy.effect_id)
                    .is_ok()
                && harmony
                    .remote_paired_output_formula_effect_ids
                    .binary_search(&self.synergy_luck_field.effect_id)
                    .is_ok()
                && harmony
                    .remote_paired_output_formula_effect_ids
                    .binary_search(&self.tactical_blessing.effect_id)
                    .is_ok()
                && harmony
                    .remote_paired_output_formula_effect_ids
                    .binary_search(&self.all_class_aura.effect_id)
                    .is_ok()
                && harmony
                    .remote_paired_output_formula_effect_ids
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
                && {
                    let mut hasher = Sha256::new();
                    for effect_id in &harmony.remote_paired_output_formula_effect_ids {
                        if *effect_id != self.coordinated_strike.effect_id
                            && *effect_id != self.element_sharing.effect_id
                            && *effect_id != self.attribute_transfer.effect_id
                            && *effect_id != self.enhanced_synergy.effect_id
                            && *effect_id != self.synergy_luck_field.effect_id
                            && *effect_id != self.tactical_blessing.effect_id
                        {
                            hasher.update(effect_id.to_le_bytes());
                        }
                    }
                    format!("{:x}", hasher.finalize())
                        == "da6bc3f98a3c8c38105bc9ff17eb8fd3be50f905481046c919969d34c86a44de"
                }
                && harmony.remote_paired_output_max_pair_gap_micros == 30_000_000
                && harmony.remote_paired_output_max_provider_share_basis_points == 200
                && harmony.remote_paired_output_min_distinct_targets >= 2;
        if harmony.effect_id <= 0
            || harmony.source_terminal_effect_id <= 0
            || harmony.effect_id == harmony.source_terminal_effect_id
            || !harmony.has_valid_source_origin_rule()
            || !rules_are_valid_and_unique(&harmony.recipient_rules)
            || harmony.accounting_method != "observed-final-damage-proportional-stage-share"
            || harmony.server_integer_counterfactual_authority
            || harmony.rational_integer_projection
                != "sum-exact-then-half-up-per-effect-provider-recipient"
            || !harmony.unresolved_overlap_fails_closed
            || !harmony_runtime_classes_are_valid
            || !harmony_remote_paired_output_evidence_is_valid
            || (harmony.runtime_transfer_enabled
                && (!harmony_runtime_authority || harmony.runtime_recipient_class_ids != [11]))
            || (!harmony.runtime_transfer_enabled
                && !harmony.runtime_recipient_class_ids.is_empty())
        {
            return Err(
                "bundled BPSR Harmony Grace formula is not ready for runtime transfer".into(),
            );
        }

        let stat_resonance = &self.stat_resonance;
        let stat_resonance_runtime_authority = stat_resonance
            .current_build_external_lifecycle_authority
            && stat_resonance.exact_same_wire_final_attack_marginal_authority
            && stat_resonance.accounting_method == "observed-final-damage-proportional-stage-share"
            && !stat_resonance.server_integer_counterfactual_authority
            && stat_resonance.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && stat_resonance.unresolved_overlap_fails_closed;
        if stat_resonance.effect_id != 2_207_252
            || stat_resonance.source_type_id != 1
            || stat_resonance.source_config_id != 2_207_251
            || (stat_resonance.runtime_transfer_enabled
                && (!stat_resonance_runtime_authority || self.game_build != "24687926"))
        {
            return Err(
                "bundled BPSR Stat Resonance formula is not ready for runtime transfer".into(),
            );
        }

        let fiery = &self.fiery_battle_will;
        let fiery_runtime_authority = fiery.current_build_external_lifecycle_authority
            && fiery.current_build_provider_ownership_authority
            && fiery.exact_mirrored_attack_raw_percent_transition_authority
            && fiery.local_recipient_only
            && fiery.accounting_method == "observed-final-damage-proportional-stage-share"
            && !fiery.server_integer_counterfactual_authority
            && fiery.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && fiery.unresolved_overlap_fails_closed;
        if fiery.effect_id != 2_110_065
            || fiery.source_type_id != 1
            || fiery.source_config_id != 2_110_064
            || fiery.provider_raw_percent_delta != 1_000
            || (fiery.runtime_transfer_enabled
                && (!fiery_runtime_authority || self.game_build != "24687926"))
        {
            return Err(
                "bundled BPSR Fiery Battle Will formula is not ready for runtime transfer".into(),
            );
        }

        let encore = &self.encore;
        let encore_runtime_authority = encore.current_build_lifecycle_authority
            && encore.current_build_provider_ownership_authority
            && encore.exact_packet_final_integer_authority
            && encore.same_provider_instances_coalesced
            && encore.external_provider_only
            && encore.ordinary_damage_unchanged
            && encore.accounting_method == "standalone-generated-output-whole-packet-final-integer"
            && encore.proof_content_sha256
                == "6dd132454bbef3f56f800bdc3aae9b2455bb65c24db48a09c38e99eaf8f4137f"
            && encore.proof_exact_build_rlogs == 26
            && encore.proof_attributed_events == 2_746
            && encore.proof_attributed_rdmg == 55_685_346
            && encore.locale_evidence
                == "English Encore verified in build 24252055; numeric effect ID 55333 observed in build 24687926; current-build English locale not independently extracted";
        if encore.effect_id != 55_333
            || encore.damage_action_ids != [230_401, 230_501]
            || encore.excluded_healing_action_id != 55_314
            || (self.encore_runtime_transfer_enabled() && !encore_runtime_authority)
        {
            return Err(
                "bundled BPSR Encore (55333) direct-output rule is not ready for runtime transfer"
                    .into(),
            );
        }

        let thunderwind = &self.thunderwind;
        let thunderwind_ids = [
            thunderwind.effect_id,
            thunderwind.child_effect_id,
            thunderwind.child_source_config_id,
            thunderwind.summon_entity_type_id,
            i64::from(thunderwind.summon_config_attribute_id),
            thunderwind.summon_config_id,
            i64::from(thunderwind.summon_owner_attribute_ids[0]),
            i64::from(thunderwind.summon_owner_attribute_ids[1]),
            i64::from(thunderwind.critical_chance_attribute_id),
            i64::from(thunderwind.critical_damage_attribute_id),
        ];
        if thunderwind_ids.iter().any(|value| *value <= 0)
            || thunderwind.recipient_scope != "summon-owner-only"
            || thunderwind.runtime_transfer_enabled
            || thunderwind.packet_proven_vectors.is_empty()
            || thunderwind.packet_proven_vectors.iter().any(|vector| {
                vector.source_level <= 0
                    || vector.critical_chance_raw_delta <= 0
                    || vector.critical_damage_raw_delta <= 0
            })
            || thunderwind
                .packet_proven_vectors
                .iter()
                .enumerate()
                .any(|(index, vector)| {
                    thunderwind.packet_proven_vectors[..index]
                        .iter()
                        .any(|other| other.source_level == vector.source_level)
                })
        {
            return Err("bundled BPSR Thunderwind formula is invalid".into());
        }

        let inspiration = &self.inspiration;
        let inspiration_chance_shared_runtime_authority = inspiration
            .current_build_lifecycle_authority
            && inspiration.current_build_magnitude_authority
            && inspiration.exact_rational_chance_attribution_authority
            && inspiration.protocol_pack_migration_authority
            && inspiration.authorized_protocol_pack_digests
                == [
                    "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
                    "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395",
                    "sha256:f975b4acade288bc87392bfeaae464873f7af1d3060be56023ff69d176905a3e",
                    "sha256:f4eb9db52ee232ecc7845119cb7fd909fb0f2c2d4fee33fe587b4235b656773c",
                    "sha256:58c849d0264261efe8220b7dd5ce50fd7e3f8fa31980941e823a18306f30c7d1",
                    "sha256:9de9c7eccc5309686ad4e982968aef67c1d6cf6f59e71762c457ce8ce8f23ac3",
                    "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
                ]
            && inspiration
                .authorized_protocol_pack_digests
                .contains(&self.protocol_pack_digest)
            && inspiration.formula_authority_basis
                == "current-build-exact-lifecycle-removal-magnitude-and-critical-factor-replay-plus-target-pack-decoder-contract-migration"
            && inspiration.accounting_method == "observed-final-damage-proportional-stage-share"
            && !inspiration.server_integer_counterfactual_authority
            && inspiration.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && inspiration.unresolved_overlap_fails_closed
            && inspiration.canonical_conservation_replay_authority
            && inspiration.chance_proof.is_current_authority()
            && inspiration.chance_replay.is_current_authority();
        let inspiration_critical_runtime_authority = inspiration_chance_shared_runtime_authority
            && self.policy.critical_damage_factor_interpretation_authority
            && self.critical_damage_factor_interpretation
                == CriticalDamageFactorInterpretation::AdditiveBonus;
        let inspiration_chance_runtime_enabled = inspiration
            .critical_chance_runtime_transfer_enabled
            || inspiration.lucky_chance_runtime_transfer_enabled;
        let inspiration_chance_magnitudes_are_current_authority = inspiration.chance_magnitudes
            == [
                InspirationChanceMagnitudeRuntimeConfig {
                    effect_level: 2,
                    chance_raw_delta: 150,
                    exact_removal_instances: 10,
                },
                InspirationChanceMagnitudeRuntimeConfig {
                    effect_level: 5,
                    chance_raw_delta: 300,
                    exact_removal_instances: 5,
                },
            ];
        let inspiration_combined_route = &inspiration.combined_critical_lucky_route;
        let inspiration_combined_route_is_current_authority = inspiration_combined_route.ability_id
            == 2_031_109
            && inspiration_combined_route.hit_event_id == 3
            && inspiration_combined_route.damage_source == 2
            && inspiration_combined_route.packet_type_flags == 1
            && inspiration_combined_route.reported_critical_must_be_absent
            && inspiration_combined_route.normal_value_must_be_absent
            && inspiration_combined_route.lucky_value_must_equal_observed_damage
            && inspiration_combined_route.provider_origin_source_type_id == 1
            && inspiration_combined_route.formula_identity
                == "inspiration-2202041-ability-2031109-packet-final-critical-lucky-rational-v1"
            && inspiration_combined_route.exact_game_file_level_magnitude_authority
            && inspiration_combined_route.event_time_recipient_factor_authority
            && inspiration_combined_route.packet_final_rational_counterfactual;
        let mut inspiration_attribute_ids = inspiration.primary_raw_add_attribute_ids.to_vec();
        inspiration_attribute_ids.extend([
            inspiration.critical_chance_attribute_id,
            inspiration.critical_chance_raw_add_attribute_id,
            inspiration.lucky_chance_attribute_id,
            inspiration.lucky_chance_raw_add_attribute_id,
            inspiration.mastery_attribute_id,
            inspiration.mastery_raw_add_attribute_id,
            inspiration.versatility_attribute_id,
            inspiration.versatility_raw_add_attribute_id,
            inspiration.external_damage_attribute_id,
            inspiration.property_damage_attribute_id,
            inspiration.haste_attribute_id,
        ]);
        if inspiration.effect_id <= 0
            || inspiration.source_config_id <= 0
            || inspiration.full_bloom_effect_id <= 0
            || inspiration.full_bloom_full_name != "Full Bloom"
            || inspiration.full_bloom_source_type_id != 1
            || inspiration.full_bloom_source_config_id <= 0
            || inspiration.full_bloom_required_level != 1
            || inspiration.full_bloom_required_stacks != 1
            || inspiration.full_bloom_duration_millis != 10_000
            || inspiration.full_bloom_potency_numerator != 6
            || inspiration.full_bloom_potency_denominator != 5
            || inspiration.full_bloom_external_windows != 83
            || inspiration.full_bloom_exact_build_rlogs != 26
            || inspiration.full_bloom_receipt_sha256
                != "cad8b61eab03086e12a0ed0963303eb10eb82c7d611aba378b7d669d2c470176"
            || inspiration.full_bloom_receipt_bytes != 2_866
            || inspiration.full_bloom_emitted_contribution_events != 229
            || inspiration.full_bloom_projected_credit != 217_407
            || inspiration.effect_id == inspiration.full_bloom_effect_id
            || inspiration_attribute_ids.iter().any(|value| *value <= 0)
            || inspiration_attribute_ids
                .iter()
                .enumerate()
                .any(|(index, value)| inspiration_attribute_ids[..index].contains(value))
            || inspiration.packet_proven_vectors.len() != 2
            || inspiration.packet_proven_vectors.iter().any(|vector| {
                vector.primary_raw_add_delta <= 0
                    || vector.secondary_raw_add_delta <= 0
                    || vector.external_damage_delta <= 0
                    || vector.external_damage_delta
                        != vector.secondary_raw_add_delta.saturating_mul(35) / 100
                    || vector.property_damage_raw_delta <= 0
                    || vector.property_damage_raw_delta
                        != vector.secondary_raw_add_delta.saturating_mul(60) / 100
            })
            || inspiration
                .packet_proven_vectors
                .iter()
                .filter(|vector| vector.provider_full_bloom)
                .count()
                != 1
            || inspiration.damage_scripts != ["Attack", "MAttack"]
            || inspiration.property_damage_property != 7
            || (inspiration_chance_runtime_enabled
                && (!inspiration_combined_route_is_current_authority
                    || !inspiration.combined_reconciliation.is_current_receipt()
                    || inspiration.combined_critical_lucky_runtime_transfer_enabled
                        != inspiration.combined_reconciliation.runtime_authority))
            || (!inspiration_chance_runtime_enabled
                && inspiration.combined_critical_lucky_runtime_transfer_enabled)
            || inspiration.runtime_transfer_enabled
            || (inspiration.full_bloom_increment_runtime_transfer_enabled
                && (!inspiration_chance_runtime_enabled
                    || !inspiration.current_build_lifecycle_authority
                    || !inspiration.current_build_magnitude_authority
                    || !inspiration.exact_rational_chance_attribution_authority
                    || !inspiration.canonical_conservation_replay_authority))
            || (inspiration.recipient_dependency_runtime_transfer_enabled
                && (!inspiration_critical_runtime_authority
                    || !inspiration.recipient_dependency.is_current_authority()))
            || (inspiration.base_lucky_damage_dependency_runtime_transfer_enabled
                && (!inspiration_chance_shared_runtime_authority
                    || !inspiration.lucky_chance_runtime_transfer_enabled
                    || inspiration
                        .base_lucky_damage_dependency
                        .lucky_chance_attribute_id
                        != inspiration.lucky_chance_attribute_id
                    || inspiration
                        .base_lucky_damage_dependency
                        .lucky_damage_attribute_id
                        != self.team_luck.lucky_damage_attribute_id
                    || !inspiration
                        .base_lucky_damage_dependency
                        .is_current_authority()))
            || (inspiration.critical_chance_runtime_transfer_enabled
                && (!inspiration_critical_runtime_authority
                    || !inspiration_chance_magnitudes_are_current_authority))
            || (inspiration.lucky_chance_runtime_transfer_enabled
                && (!inspiration_chance_shared_runtime_authority
                    || !inspiration_chance_magnitudes_are_current_authority))
            || (!inspiration_chance_runtime_enabled
                && (inspiration.current_build_lifecycle_authority
                    || inspiration.current_build_magnitude_authority
                    || inspiration.exact_rational_chance_attribution_authority
                    || inspiration.protocol_pack_migration_authority
                    || !inspiration.authorized_protocol_pack_digests.is_empty()
                    || inspiration.canonical_conservation_replay_authority
                    || !inspiration.chance_magnitudes.is_empty()))
        {
            return Err(
                "bundled BPSR Inspiration formula is not ready for runtime transfer".into(),
            );
        }

        let inspire = &self.inspire;
        let inspire_current_authority = inspire.current_build_lifecycle_authority
            && inspire.current_build_provider_ownership_authority
            && inspire.observed_single_stack_no_overlap_authority
            && inspire.exact_recipient_speed_transition_authority
            && inspire.exact_native_speed_formula_authority
            && inspire.exact_action_route_authority
            && inspire.temporary_speed_term_zero_authority
            && inspire.damage_resolution_window_accounting_authority
            && !inspire.server_integer_counterfactual_authority
            && inspire.ordinary_damage_unchanged
            && inspire.unresolved_overlap_fails_closed
            && inspire.accounting_method
                == "packet-final-damage-resolution-window-speed-capacity-share"
            && inspire.allocation_order == "speed-opportunity-before-per-hit-damage-modifiers"
            && inspire.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && inspire.provider_ownership_proof_sha256
                == "964c9b36de0ffae6cc47a85bbf06195219827225c5f04e278ae2fe88df521c6e"
            && inspire.stacking_proof_sha256
                == "37c38b31f52bd21bf071b4a0ef39f000059ecb0292a0a9a655515d163c454bca"
            && inspire.damage_time_state_proof_sha256
                == "1084c1a735bd39b4845b755c39b24c138c72f8d9f57774c650a18a4cce701518"
            && inspire.temporary_lane_proof_sha256
                == "18563c17c24aa36a342752f541ca8df48b88363076e6fb18b77dabb1de014049"
            && inspire.action_timing_ancestry_proof_sha256
                == "3922b8b3b340b7d9f53f3024d70eeb448df0612e8f19cdd3a284981284b77de0"
            && inspire.action_route_proof_sha256
                == "604601814ee87387c0433eef10ade28160a2f69653854cffcccfbbf8f343a502"
            && inspire.capacity_proof_sha256
                == "19acedc12102db945bdd6153fd363c11feb5548bd5b1e011b72533a4748a0a9b"
            && inspire.recipient_mode_proof_sha256
                == "36c4024d9b750eca634eddb367ca9f353e6b875f0dae0cff22cba283f3a962f9"
            && inspire.exact_build_rlogs == 26
            && inspire.observed_status_events == 130
            && inspire.observed_complete_windows == 65
            && inspire.observed_damage_memberships == 3_713
            && inspire.observed_external_damage_memberships == 3_185
            && inspire.observed_self_provider_exclusions == 528;
        let inspire_historical_authority_is_clear = !inspire.current_build_lifecycle_authority
            && !inspire.current_build_provider_ownership_authority
            && !inspire.observed_single_stack_no_overlap_authority
            && !inspire.exact_recipient_speed_transition_authority
            && !inspire.exact_native_speed_formula_authority
            && !inspire.exact_action_route_authority
            && !inspire.temporary_speed_term_zero_authority
            && !inspire.damage_resolution_window_accounting_authority
            && inspire.provider_ownership_proof_sha256.is_empty()
            && inspire.stacking_proof_sha256.is_empty()
            && inspire.damage_time_state_proof_sha256.is_empty()
            && inspire.temporary_lane_proof_sha256.is_empty()
            && inspire.action_timing_ancestry_proof_sha256.is_empty()
            && inspire.action_route_proof_sha256.is_empty()
            && inspire.capacity_proof_sha256.is_empty()
            && inspire.recipient_mode_proof_sha256.is_empty()
            && inspire.exact_build_rlogs == 0
            && inspire.observed_status_events == 0
            && inspire.observed_complete_windows == 0
            && inspire.observed_damage_memberships == 0
            && inspire.observed_external_damage_memberships == 0
            && inspire.observed_self_provider_exclusions == 0;
        if inspire.effect_id != 31_602
            || inspire.source_type_id != 1
            || inspire.source_config_id != 31_601
            || inspire.required_stacks != 1
            || inspire.duration_millis != 10_000
            || inspire.normal_speed_attribute_id != 11_720
            || inspire.guide_speed_attribute_id != 11_730
            || inspire.action_routes != INSPIRE_CURRENT_ACTION_ROUTES
            || inspire.recipient_modes != INSPIRE_CURRENT_RECIPIENT_MODES
            || (self.game_build == "24687926" && !inspire_current_authority)
            || (self.game_build != "24687926" && !inspire_historical_authority_is_clear)
            || inspire.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Inspire (effect 31602) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let critical_cold = &self.critical_cold;
        let critical_cold_replay = &critical_cold.replay;
        let critical_cold_replay_authority = critical_cold_replay.content_sha256
            == "0c66f83d449898a0323bdbf55e21815c65c78fb83ee7b0f0e47ae8ee36e2b32f"
            && critical_cold_replay.content_hash_method
                == "sha256-concatenated-sorted-shard-file-bytes"
            && critical_cold_replay.content_bytes == 151_717_449
            && critical_cold_replay.replay_schema_version == 27
            && critical_cold_replay.exact_build_rlogs == 26
            && critical_cold_replay.total_canonical_events == 6_411_565
            && critical_cold_replay.runtime_target_match_rlogs == 26
            && critical_cold_replay.conserved_rlogs == 26
            && critical_cold_replay.rational_projection_overflow_count == 0
            && critical_cold_replay.direct_ready_events == 39_822
            && critical_cold_replay.direct_ready_projected_rdmg == 145_988_935
            && critical_cold_replay.gap_safe_lightfall_conversion_events == 15_011
            && critical_cold_replay.post_tcp_gap_no_conversion_events == 24_811
            && critical_cold_replay.emitted_contribution_events == 39_560
            && critical_cold_replay.emitted_projected_rdmg == 142_729_617
            && critical_cold_replay.joint_team_luck_emitted_events == 39_036
            && critical_cold_replay.standalone_emitted_events == 524
            && critical_cold_replay.deliberate_encore_exclusion_events == 194
            && critical_cold_replay.unresolved_stat_resonance_highland_critical_stage_order_events
                == 68
            && critical_cold_replay.team_luck_emitted_events == 202_740
            && critical_cold_replay.team_luck_projected_rdmg == 676_489_800
            && critical_cold_replay
                .gap_safe_lightfall_conversion_events
                .checked_add(critical_cold_replay.post_tcp_gap_no_conversion_events)
                == Some(critical_cold_replay.direct_ready_events)
            && critical_cold_replay
                .joint_team_luck_emitted_events
                .checked_add(critical_cold_replay.standalone_emitted_events)
                == Some(critical_cold_replay.emitted_contribution_events)
            && critical_cold_replay
                .emitted_contribution_events
                .checked_add(critical_cold_replay.deliberate_encore_exclusion_events)
                .and_then(|value| {
                    value.checked_add(
                        critical_cold_replay
                            .unresolved_stat_resonance_highland_critical_stage_order_events,
                    )
                })
                == Some(critical_cold_replay.direct_ready_events);
        let critical_cold_offline_oracle_is_diagnostic_only =
            critical_cold.offline_oracle.content_sha256
                == "cdcb81de166ef2651b401984a16f2c57398c8b0d6fd8e1cf2c4e31e314b34bfa"
                && critical_cold.offline_oracle.exact_build_rlogs == 26
                && critical_cold.offline_oracle.eligible_critical_only_samples == 39_125
                && critical_cold.offline_oracle.projected_rdmg == 165_939_233
                && !critical_cold.offline_oracle.runtime_authority
                && critical_cold.offline_oracle.authority_boundary
                    == "diagnostic-only-no-tcp-gap-or-unresolved-overlap-runtime-gates";
        let critical_cold_runtime_authority = critical_cold.current_build_lifecycle_authority
            && critical_cold.current_build_provider_ownership_authority
            && critical_cold.current_build_magnitude_authority
            && critical_cold.reuses_inspiration_critical_stage_authority
            && inspiration_critical_runtime_authority
            && inspiration.critical_chance_runtime_transfer_enabled
            && inspiration.recipient_dependency_runtime_transfer_enabled
            && inspiration.recipient_dependency.is_current_authority()
            && critical_cold.accounting_method == "observed-final-damage-proportional-stage-share"
            && critical_cold.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && critical_cold.unresolved_overlap_fails_closed
            && critical_cold.ordinary_damage_unchanged
            && critical_cold.proof_content_sha256
                == "c03da85aab6de7211fd2a0046ed13b7d80b0d26a37bf4258bb2d4a2ecc59ac7e"
            && critical_cold.proof_exact_build_rlogs == 26
            && critical_cold.proof_status_events == 5_500
            && critical_cold.proof_windows == 2_775
            && critical_cold.proof_external_player_windows == 2_203
            && critical_cold.proof_packet_final_samples == 41_078
            && critical_cold.proof_eligible_critical_only_samples == 39_125
            && critical_cold_replay_authority
            && critical_cold_offline_oracle_is_diagnostic_only;
        if critical_cold.effect_id != 2_204_471
            || critical_cold.root_effect_id != 2_204_470
            || critical_cold.source_talent_id != 250
            || critical_cold.official_en_us_source_name != "Critical Cold"
            || critical_cold.child_en_us_name.is_some()
            || critical_cold.child_design_name != "暴击之寒_队友暴击"
            || critical_cold.source_type_id != 1
            || critical_cold.source_config_id != critical_cold.root_effect_id
            || critical_cold.required_level != 1
            || critical_cold.required_stacks != 1
            || critical_cold.critical_chance_attribute_id
                != inspiration.critical_chance_attribute_id
            || critical_cold.critical_damage_attribute_id
                != self.team_luck.critical_damage_attribute_id
            || critical_cold.critical_chance_raw_delta != 300
            || critical_cold.critical_chance_cap_raw != 10_000
            || critical_cold.recipient_dependency_effect_id
                != inspiration.recipient_dependency.effect_id
            || critical_cold.recipient_dependency_talent_id
                != inspiration.recipient_dependency.talent_id
            || critical_cold.recipient_dependency_critical_damage_raw_delta
                != inspiration
                    .recipient_dependency
                    .critical_damage_raw_delta(critical_cold.critical_chance_raw_delta)
                    .unwrap_or_default()
            || (critical_cold.runtime_transfer_enabled && !critical_cold_runtime_authority)
            || critical_cold.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Critical Cold (talent 250; child effect 2204471, design name 暴击之寒_队友暴击) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let synergy_crit = &self.synergy_crit_field;
        let synergy_crit_current_authority = synergy_crit.game_description_formula_authority
            && synergy_crit.game_description_party_scope_authority
            && synergy_crit.exact_child_status_identity_authority
            && synergy_crit.additive_critical_stage_authority
            && synergy_crit.accounting_method
                == "observed-final-damage-proportional-critical-damage-stage-share"
            && synergy_crit.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && synergy_crit.unresolved_overlap_fails_closed
            && synergy_crit.ordinary_damage_unchanged;
        if synergy_crit.effect_id != 997_538
            || synergy_crit.root_effect_id != 997_536
            || synergy_crit.aura_effect_id != 997_537
            || synergy_crit.source_rogue_entry_id != 209
            || synergy_crit.description_id != 110_901
            || synergy_crit.official_en_us_name != "Synergy Crit Field"
            || synergy_crit.required_level != 38
            || synergy_crit.required_stacks != 1
            || synergy_crit.aura_duration_millis != 5_000
            || synergy_crit.child_refresh_duration_millis != 1_100
            || synergy_crit.aura_radius_meters != 15
            || synergy_crit.critical_damage_attribute_id
                != self.team_luck.critical_damage_attribute_id
            || synergy_crit.critical_damage_raw_delta != 300
            || (self.game_build == "24687926" && !synergy_crit_current_authority)
            || synergy_crit.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Synergy Crit Field (Rogue entry 209; recipient effect 997538) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let element_sharing = &self.element_sharing;
        let element_sharing_current_authority = element_sharing.game_description_formula_authority
            && element_sharing.game_description_party_scope_authority
            && element_sharing.exact_child_status_identity_authority
            && element_sharing.additive_all_plus_property_stage_authority
            && element_sharing.accounting_method
                == "observed-final-damage-proportional-all-plus-property-element-stage-share"
            && element_sharing.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && element_sharing.unresolved_overlap_fails_closed
            && element_sharing.ordinary_damage_unchanged;
        if element_sharing.effect_id != 997_513
            || element_sharing.root_effect_id != 997_512
            || element_sharing.source_rogue_entry_id != 196
            || element_sharing.description_id != 109_601
            || element_sharing.official_en_us_name != "Element Sharing"
            || element_sharing.required_level != 13
            || element_sharing.required_stacks != 1
            || element_sharing.duration_millis != 10_000
            || element_sharing.element_damage_raw_delta != 2_000
            || (self.game_build == "24687926" && !element_sharing_current_authority)
            || element_sharing.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Element Sharing (Rogue entry 196; recipient effect 997513) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let enhanced_synergy = &self.enhanced_synergy;
        let enhanced_synergy_current_authority = enhanced_synergy
            .game_description_formula_authority
            && enhanced_synergy.game_description_party_scope_authority
            && enhanced_synergy.exact_child_status_identity_authority
            && enhanced_synergy.packet_final_boost_attributes_authority
            && enhanced_synergy.corrected_calculator_multiplicative_boost_stage_authority
            && enhanced_synergy.accounting_method
                == "observed-final-damage-proportional-phy-mag-boost-stage-share"
            && enhanced_synergy.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && enhanced_synergy.unresolved_overlap_fails_closed
            && enhanced_synergy.ordinary_damage_unchanged;
        if enhanced_synergy.effect_id != 997_518
            || enhanced_synergy.root_effect_id != 997_517
            || enhanced_synergy.source_rogue_entry_id != 199
            || enhanced_synergy.description_id != 109_901
            || enhanced_synergy.official_en_us_name != "Enhanced Synergy"
            || enhanced_synergy.required_level != 18
            || enhanced_synergy.required_stacks != 1
            || enhanced_synergy.duration_millis != 3_000
            || enhanced_synergy.physical_boost_attribute_id != 12_550
            || enhanced_synergy.magical_boost_attribute_id != 12_570
            || enhanced_synergy.boost_raw_delta != 1_000
            || (self.game_build == "24687926" && !enhanced_synergy_current_authority)
            || enhanced_synergy.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Enhanced Synergy (Rogue entry 199; recipient effect 997518) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let blessing = &self.blessing;
        let blessing_current_authority = blessing.exact_current_build_buff_row_authority
            && blessing.game_description_formula_authority
            && blessing.game_description_party_scope_authority
            && blessing.exact_current_build_fight_attribute_row_authority
            && blessing.packet_final_general_damage_attribute_route_authority
            && blessing.corrected_calculator_additive_general_damage_stage_authority
            && blessing.accounting_method
                == "observed-final-damage-proportional-additive-general-damage-stage-share"
            && blessing.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && blessing.unresolved_stacking_fails_closed
            && blessing.unresolved_overlap_fails_closed
            && blessing.ordinary_damage_unchanged;
        if blessing.effect_id != 2_100_154
            || blessing.source_skill_id != 3_401
            || blessing.official_en_us_name != "Blessing"
            || blessing.required_level != 1
            || blessing.required_stacks != 1
            || blessing.duration_millis != 10_000
            || blessing.general_damage_attribute_id != 12_670
            || blessing.general_damage_raw_delta != 3_000
            || blessing.current_build_buff_row_fingerprint_sha256
                != "sha256:6cc38de4edc95d37b153196aa65e6db1ebf09aa13f5500f4e4933fdea56bc464"
            || blessing.current_build_fight_attribute_row_fingerprint_sha256
                != "sha256:3a67b30072f160536002ebb8d3b41fba6cacfa76c1424640daa23284300ee0a1"
            || blessing.pinned_calculator_calc_skill_sha256
                != "sha256:7c9a29e0ee817e8b6439e130c3e4f68443e28faf402f3775b75feae5ed269fc2"
            || blessing.pinned_calculator_general_damage_stage_expression
                != "stdMult includes (1 + totalGen) after (1 + finalElemDmgPct) and before (1 + finalDreamDmgPct)"
            || (self.game_build == "24687926" && !blessing_current_authority)
            || blessing.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Blessing (skill 3401; recipient effect 2100154) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let synergy_luck_field = &self.synergy_luck_field;
        let synergy_luck_field_current_authority = synergy_luck_field
            .game_description_trigger_and_party_scope_authority
            && synergy_luck_field.exact_child_status_identity_authority
            && synergy_luck_field.exact_imagine_passive_family_authority
            && synergy_luck_field.exact_produced_damage_action_authority
            && synergy_luck_field.exact_recipient_loadout_absence_required
            && synergy_luck_field.observed_final_direct_output_authority
            && synergy_luck_field.accounting_method == "whole-observed-final-produced-damage"
            && synergy_luck_field.unresolved_overlap_fails_closed
            && synergy_luck_field.ordinary_damage_unchanged;
        if synergy_luck_field.effect_id != 997_534
            || synergy_luck_field.root_effect_id != 997_533
            || synergy_luck_field.source_rogue_entry_id != 208
            || synergy_luck_field.description_id != 110_801
            || synergy_luck_field.official_en_us_name != "Synergy Luck Field"
            || synergy_luck_field.required_level != 34
            || synergy_luck_field.required_stacks != 1
            || synergy_luck_field.duration_millis != 10_000
            || synergy_luck_field.trigger_cooldown_millis != 30_000
            || synergy_luck_field.granted_imagine_ability_id != 3_937
            || synergy_luck_field.granted_imagine_item_id != 3_000_016
            || synergy_luck_field.granted_passive_effect_id != 3_210_080
            || synergy_luck_field.produced_damage_ability_id != 3_210_081
            || synergy_luck_field.produced_damage_attr_id != 2_321_008_101
            || synergy_luck_field.base_passive_coefficient_basis_points != 1_120
            || (self.game_build == "24687926" && !synergy_luck_field_current_authority)
            || synergy_luck_field.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Synergy Luck Field (Rogue entry 208; recipient effect 997534) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let coordinated_strike = &self.coordinated_strike;
        let coordinated_strike_current_authority = coordinated_strike
            .game_description_formula_authority
            && coordinated_strike.game_description_party_scope_authority
            && coordinated_strike.exact_child_status_identity_authority
            && coordinated_strike.additive_attack_percent_stage_authority
            && coordinated_strike.same_stage_provider_conservation_authority
            && coordinated_strike.accounting_method
                == "observed-final-damage-proportional-additive-attack-percent-stage-share"
            && coordinated_strike.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && coordinated_strike.unresolved_overlap_fails_closed
            && coordinated_strike.ordinary_damage_unchanged;
        if coordinated_strike.effect_id != 997_511
            || coordinated_strike.root_effect_id != 997_510
            || coordinated_strike.source_rogue_entry_id != 195
            || coordinated_strike.description_id != 109_501
            || coordinated_strike.official_en_us_name != "Coordinated Strike"
            || coordinated_strike.required_level != 11
            || coordinated_strike.required_stacks != 1
            || coordinated_strike.duration_millis != 3_000
            || coordinated_strike.trigger_cooldown_millis != 300
            || coordinated_strike.attack_raw_percent_delta != 1_500
            || (self.game_build == "24687926" && !coordinated_strike_current_authority)
            || coordinated_strike.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Coordinated Strike (Rogue entry 195; recipient effect 997511) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let all_class_aura = &self.all_class_aura;
        let all_class_aura_current_authority = all_class_aura.game_description_formula_authority
            && all_class_aura.game_description_party_scope_authority
            && all_class_aura.exact_observed_aura_status_authority
            && all_class_aura.observed_aura_cohort_role_selector_authority
            && all_class_aura.additive_attack_percent_stage_authority
            && all_class_aura.same_stage_provider_conservation_authority
            && all_class_aura.accounting_method
                == "observed-final-damage-proportional-additive-attack-percent-stage-share"
            && all_class_aura.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && all_class_aura.unresolved_overlap_fails_closed
            && all_class_aura.ordinary_damage_unchanged;
        if all_class_aura.effect_id != 998_542
            || all_class_aura.source_rogue_entry_id != 103
            || all_class_aura.description_id != 100_301
            || all_class_aura.official_en_us_name != "All-Class Aura"
            || all_class_aura.required_level != 42
            || all_class_aura.required_stacks != 1
            || all_class_aura.base_attack_raw_percent_delta != 500
            || all_class_aura.per_distinct_role_raw_percent_delta != 500
            || all_class_aura.maximum_attack_raw_percent_delta != 2_000
            || (self.game_build == "24687926" && !all_class_aura_current_authority)
            || all_class_aura.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR All-Class Aura (Rogue entry 103; effect 998542) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let attribute_transfer = &self.attribute_transfer;
        let attribute_transfer_current_authority = attribute_transfer
            .game_description_formula_authority
            && attribute_transfer.game_description_party_scope_authority
            && attribute_transfer.exact_child_status_identity_authority
            && attribute_transfer.exact_adjacent_lane_transition_authority
            && attribute_transfer.corrected_calculator_final_substat_stage_authority
            && attribute_transfer.corrected_calculator_versatility_stage_authority
            && attribute_transfer.reuses_inspiration_chance_stage_authority
            && attribute_transfer.accounting_method
                == "observed-final-damage-proportional-final-substat-stage-share"
            && attribute_transfer.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && attribute_transfer.unresolved_overlap_fails_closed
            && attribute_transfer.ordinary_damage_unchanged;
        if attribute_transfer.effect_id != 997_515
            || attribute_transfer.root_effect_id != 997_514
            || attribute_transfer.source_rogue_entry_id != 197
            || attribute_transfer.description_id != 109_701
            || attribute_transfer.official_en_us_name != "Attribute Transfer"
            || attribute_transfer.required_level != 15
            || attribute_transfer.required_stacks != 1
            || attribute_transfer.duration_millis != 10_000
            || attribute_transfer.substat_raw_delta != 1_000
            || attribute_transfer.versatility_to_external_damage_numerator != 35
            || attribute_transfer.versatility_to_external_damage_denominator != 100
            || !attribute_transfer.critical_chance_runtime_transfer_enabled
            || !attribute_transfer.lucky_chance_runtime_transfer_enabled
            || !attribute_transfer.versatility_runtime_transfer_enabled
            || attribute_transfer.mastery_runtime_transfer_enabled
            || attribute_transfer.haste_runtime_transfer_enabled
            || (self.game_build == "24687926" && !attribute_transfer_current_authority)
            || attribute_transfer.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Attribute Transfer (Rogue entry 197; recipient effect 997515) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let life_wave = &self.life_wave;
        let life_wave_current_authority = life_wave.calculator_lane_selection_authority
            && life_wave.exact_module_profile_magnitude_authority
            && life_wave.exact_child_status_identity_authority
            && life_wave.cross_vantage_trigger_ownership_authority
            && life_wave.exact_adjacent_lane_transition_authority
            && life_wave.packet_final_counterfactual_authority
            && life_wave.reviewed_action_route_required
            && life_wave.unresolved_overlap_fails_closed
            && life_wave.ordinary_damage_unchanged
            && life_wave.accounting_method
                == "observed-final-damage-selected-secondary-lane-counterfactual"
            && life_wave.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient";
        if life_wave.effect_id != 2_302_421
            || life_wave.source_type_id != 1
            || life_wave.source_config_id != 2_302_420
            || life_wave.module_effect_id != 2_404
            || life_wave.duration_millis != 5_000
            || life_wave.level_five_bonus_basis_points != 600
            || life_wave.level_six_bonus_basis_points != 1_000
            || life_wave.versatility_to_external_damage_numerator != 35
            || life_wave.versatility_to_external_damage_denominator != 100
            || (self.game_build == "24687926" && !life_wave_current_authority)
            || life_wave.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Life Wave (module part 2404; recipient effect 2302421) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let tactical_blessing = &self.tactical_blessing;
        let tactical_blessing_current_authority = tactical_blessing
            .game_description_formula_authority
            && tactical_blessing.game_description_party_scope_authority
            && tactical_blessing.exact_child_status_identity_authority
            && tactical_blessing.exact_static_lifecycle_authority
            && tactical_blessing.corrected_calculator_final_substat_stage_authority
            && tactical_blessing.reuses_inspiration_chance_stage_authority
            && tactical_blessing.accounting_method
                == "observed-final-damage-proportional-critical-and-lucky-chance-stage-share"
            && tactical_blessing.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient"
            && tactical_blessing.unresolved_overlap_fails_closed
            && tactical_blessing.ordinary_damage_unchanged;
        if tactical_blessing.effect_id != 997_570
            || tactical_blessing.root_effect_id != 997_557
            || tactical_blessing.source_rogue_entry_id != 349
            || tactical_blessing.description_id != 124_901
            || tactical_blessing.official_en_us_name != "Tactical Blessing"
            || tactical_blessing.required_level != 70
            || tactical_blessing.required_stacks != 1
            || tactical_blessing.duration_millis != 10_000
            || tactical_blessing.critical_chance_raw_delta != 1_000
            || tactical_blessing.lucky_chance_raw_delta != 1_000
            || tactical_blessing.critical_chance_raw_delta
                != tactical_blessing.lucky_chance_raw_delta
            || (self.game_build == "24687926" && !tactical_blessing_current_authority)
            || tactical_blessing.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Tactical Blessing (Rogue entry 349; recipient effect 997570) formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let endless_mind = &self.endless_mind;
        let endless_mind_current_authority = endless_mind.game_description_formula_authority
            && endless_mind.game_description_party_scope_authority
            && endless_mind.exact_action_identity_authority
            && endless_mind.observed_final_proportional_authority
            && endless_mind.unresolved_overlap_fails_closed
            && endless_mind.ordinary_damage_unchanged
            && endless_mind.accounting_method
                == "observed-final-damage-proportional-derived-element-stage-share"
            && endless_mind.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient";
        if endless_mind.effect_id != 3_003_411
            || endless_mind.source_type_id != 1
            || endless_mind.source_config_id != 3_003_410
            || endless_mind.required_level != 11
            || endless_mind.minimum_stacks != 1
            || endless_mind.maximum_stacks != 3
            || endless_mind.mastery_attribute_id != self.inspiration.mastery_attribute_id
            || endless_mind.mastery_basis_points_per_stack != 200
            || endless_mind.shattered_illusion_ability_id != 3_003_213
            || endless_mind.shattered_illusion_hit_event_id != 1
            || endless_mind.shattered_illusion_damage_attr_id != 2_300_321_301
            || endless_mind.mastery_to_element_numerator != 65
            || endless_mind.mastery_to_element_denominator != 100
            || (self.game_build == "24687926" && !endless_mind_current_authority)
            || endless_mind.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Endless Mind (effect 3003411) Shattered Illusion formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let arcane = &self.arcane_time_decree;
        let arcane_current_authority = arcane.game_description_formula_authority
            && arcane.game_description_party_scope_authority
            && arcane.canonical_cooldown_acceleration_field_authority
            && arcane.exact_cooldown_action_identity_required
            && arcane.unresolved_overlap_fails_closed
            && arcane.ordinary_damage_unchanged
            && arcane.accounting_method
                == "observed-final-damage-proportional-cooldown-opportunity-capacity"
            && arcane.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient";
        if arcane.effect_id != 2_110_034
            || arcane.provider_imagine_ability_id != 3_921
            || arcane.provider_imagine_item_id != 3_000_011
            || arcane.required_effect_level != 34
            || arcane.required_stacks != 1
            || arcane.duration_millis != 20_000
            || arcane.cooldown_acceleration_basis_points_by_tier
                != [1_000, 2_000, 3_000, 4_000, 5_000]
            || (self.game_build == "24687926" && !arcane_current_authority)
            || arcane.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Arcane! Time Decree (effect 2110034) cooldown-opportunity formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let thunder_roar = &self.thunder_roar;
        let thunder_roar_current_authority = thunder_roar.game_description_trigger_authority
            && thunder_roar.game_description_party_scope_authority
            && thunder_roar.exact_damage_attr_link_authority
            && thunder_roar.source_owner_ancestry_required
            && thunder_roar.observed_final_direct_output_authority
            && thunder_roar.unresolved_overlap_fails_closed
            && thunder_roar.ordinary_damage_unchanged
            && thunder_roar.accounting_method == "whole-observed-final-produced-damage";
        if thunder_roar.effect_id != 2_110_096
            || thunder_roar.required_effect_level != 96
            || thunder_roar.required_stacks != 1
            || thunder_roar.duration_millis != 15_000
            || thunder_roar.trigger_cooldown_millis != 500
            || thunder_roar.thunderstrike_ability_id != 2_110_096
            || thunder_roar.thunderstrike_hit_event_id != 3
            || thunder_roar.thunderstrike_damage_attr_id != 2_211_009_603
            || thunder_roar.thunderstrike_coefficient_basis_points_by_tier
                != [5_800, 6_660, 7_540, 8_410, 9_280, 10_150]
            || thunder_roar.thunderstrike_fixed_parameter != 5
            || thunder_roar.excluded_placeholder_damage_attr_id != 2_211_009_601
            || thunder_roar.excluded_direct_cast_damage_attr_id != 2_211_009_604
            || (self.game_build == "24687926" && !thunder_roar_current_authority)
            || thunder_roar.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Arcane! Thunder Roar — Electro Shield (Thunderstrike; effect 2110096) produced-damage formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let poison = &self.poison_explosion_vulnerability;
        let poison_current_authority = poison.game_description_formula_authority
            && poison.game_description_party_scope_authority
            && poison.exact_target_status_identity_authority
            && poison.exact_static_lifecycle_authority
            && poison.provider_loadout_tier_authority
            && poison.additive_vulnerability_stage_authority
            && poison.same_stage_provider_conservation_authority
            && poison.unresolved_overlap_fails_closed
            && poison.ordinary_damage_unchanged
            && poison.accounting_method
                == "observed-final-damage-proportional-additive-vulnerability-stage-share"
            && poison.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient";
        if poison.effect_id != 2_110_099
            || poison.provider_imagine_ability_id != 3_942
            || poison.provider_imagine_item_id != 3_000_041
            || poison.required_effect_level != 99
            || poison.minimum_stacks != 1
            || poison.maximum_stacks != 5
            || poison.duration_millis != 8_000
            || poison.vulnerability_basis_points_per_stack_by_tier != [80, 160, 240, 320, 400]
            || poison.conflicting_target_effect_ids != [55_228, 2_100_107, 2_110_078, 2_110_092]
            || (self.game_build == "24687926" && !poison_current_authority)
            || poison.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Arcane! Poison Explosion (effect 2110099) stacked Vulnerability formula is not ready for runtime transfer"
                    .into(),
            );
        }

        let celestial = &self.celestial_guardian_vulnerability;
        let celestial_current_authority = celestial.game_description_formula_authority
            && celestial.game_description_party_scope_authority
            && celestial.exact_target_status_identity_authority
            && celestial.exact_static_lifecycle_authority
            && celestial.provider_loadout_tier_authority
            && celestial.additive_vulnerability_stage_authority
            && celestial.later_stage_cancellation_authority
            && !celestial.element_resistance_component_transfer_enabled
            && celestial.same_stage_provider_conservation_authority
            && celestial.unresolved_overlap_fails_closed
            && celestial.ordinary_damage_unchanged
            && celestial.accounting_method
                == "observed-final-damage-proportional-separated-vulnerability-component-share"
            && celestial.rational_integer_projection
                == "sum-exact-then-half-up-per-effect-provider-recipient";
        if celestial.effect_id != 2_110_167
            || celestial.provider_imagine_ability_id != 3_982
            || celestial.provider_imagine_item_id != 3_001_001
            || celestial.required_effect_level != 67
            || celestial.required_stacks != 1
            || celestial.duration_millis != 10_000
            || celestial.vulnerability_basis_points_by_tier != [60, 120, 180, 240, 300]
            || celestial.conflicting_target_effect_ids != [55_228, 2_100_107, 2_110_078, 2_110_092]
            || (self.game_build == "24687926" && !celestial_current_authority)
            || celestial.runtime_transfer_enabled != (self.game_build == "24687926")
        {
            return Err(
                "bundled BPSR Arcane! Celestial Spirit Mage (effect 2110167) separated Vulnerability component is not ready for runtime transfer"
                    .into(),
            );
        }

        let highland = &self.highland_blood;
        let highland_effect_ids = [
            highland.effect_id,
            highland.provider_marker_effect_id,
            highland.companion_lockout_effect_id,
        ];
        if highland_effect_ids.iter().any(|value| *value <= 0)
            || highland_effect_ids
                .iter()
                .enumerate()
                .any(|(index, value)| highland_effect_ids[..index].contains(value))
            || !highland.all_element_family.is_valid()
            || highland.duration_millis == 0
            || highland.lockout_duration_millis <= highland.duration_millis
            || highland.packet_proven_raw_deltas != [600, 700, 800, 900, 1_000]
            || highland.provider_imagine_ability_id != 3_957
            || highland.excluded_provider_owned_damage_ids.is_empty()
            || highland
                .excluded_provider_owned_damage_ids
                .iter()
                .any(|value| *value <= 0)
            || highland
                .excluded_provider_owned_damage_ids
                .iter()
                .enumerate()
                .any(|(index, value)| {
                    highland.excluded_provider_owned_damage_ids[..index].contains(value)
                })
            || !highland.requires_recipient_packet_transition
            || highland.runtime_transfer_enabled != (self.game_build == "24687926")
            || highland.remote_paired_output_runtime_transfer_enabled
            || highland.remote_paired_output_ignored_effect_ids != [55_301, 55_304]
            || highland.remote_paired_output_max_pair_gap_micros != 30_000_000
            || highland.remote_paired_output_min_distinct_targets != 2
        {
            return Err(
                "bundled BPSR Highland Blood formula is not ready for runtime transfer".into(),
            );
        }

        Ok(())
    }
}

fn rules_are_valid_and_unique(rules: &[PrimaryStatRecipientRule]) -> bool {
    !rules.is_empty()
        && rules.iter().all(PrimaryStatRecipientRule::is_valid)
        && rules.iter().enumerate().all(|(index, rule)| {
            !rules[..index]
                .iter()
                .any(|other| other.recipient_class_id == rule.recipient_class_id)
        })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsRuntimeBuildOverride {
    game_build: String,
    protocol_pack_digest: String,
    patch: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsRuntimeBuildOverrides {
    schema_version: u16,
    deployment_id: String,
    builds: Vec<RdpsRuntimeBuildOverride>,
}

fn validate_rdps_promotion_inventory(runtime: &RdpsRuntimeConfig) -> Result<(), String> {
    let inventory: RdpsPromotionInventory = serde_json::from_str(include_str!(
        "../game-data/runtime/rdps-promotion-inventory.v1.json"
    ))
    .map_err(|error| format!("bundled BPSR rDPS promotion inventory is invalid: {error}"))?;
    let presentation: RdpsAttributionEffectPresentation = serde_json::from_str(include_str!(
        "../game-data/runtime/rdps-attribution-effect-presentation.v1.json"
    ))
    .map_err(|error| format!("bundled BPSR rDPS attribution presentation is invalid: {error}"))?;
    let external_state: ExternalStateRdpsInventory = serde_json::from_str(include_str!(
        "../game-data/runtime/external-state-rdps.v1.json"
    ))
    .map_err(|error| format!("bundled BPSR external-state rDPS inventory is invalid: {error}"))?;

    let production_ids = inventory
        .production_effects
        .iter()
        .map(|effect| effect.effect_id)
        .collect::<Vec<_>>();
    let candidate_ids = inventory
        .remaining_candidates
        .iter()
        .map(|effect| effect.effect_id)
        .collect::<Vec<_>>();
    let external_state_ids = external_state
        .rules
        .iter()
        .map(|rule| rule.effect_id)
        .collect::<Vec<_>>();
    let presentation_effects = presentation
        .effects
        .into_iter()
        .map(|effect| RdpsPromotionEffect {
            effect_id: effect.effect_id,
            full_name: effect.name,
        })
        .collect::<Vec<_>>();

    let sorted_unique_positive = |values: &[i64]| {
        values.iter().all(|value| *value > 0) && values.windows(2).all(|pair| pair[0] < pair[1])
    };
    let coverage = &inventory.review_coverage;
    let policy = &inventory.policy;
    let inventory_identity_is_exact = inventory.schema_version
        == RDPS_PROMOTION_INVENTORY_SCHEMA_VERSION
        && inventory.deployment_id == runtime.deployment_id
        && inventory.game_build == runtime.game_build;
    let presentation_identity_is_exact = presentation.schema_version == 1
        && presentation.deployment_id == runtime.deployment_id
        && presentation.game_build == runtime.game_build
        && presentation.locale == "en-US";
    let external_state_identity_is_exact =
        external_state.schema_version == 1 && external_state.game_build == runtime.game_build;
    let policy_is_fail_closed = policy.ordinary_damage_and_dps_unchanged
        && policy.unknown_and_unresolved_events_retained
        && !policy.candidate_effects_grant_provider_credit
        && policy.production_effect_ids_are_sorted_and_unique
        && policy.complete_localized_names_required;
    let review_coverage_is_exact = coverage.consolidated_unique_effect_ids == 513
        && coverage.exact_id_route_rows == 1_586
        && coverage.exact_id_route_unique_ids == 660
        && coverage.zero_effect_rows_without_disposition
        && coverage.zero_exact_id_route_rows_without_disposition
        && coverage.exhaustive_ledger_content_sha256
            == "5451c9b8d274f4e65db98e8afa0c2b1367522b810c92144a4051479f7664ae67"
        && coverage.ledger_production_effect_ids == 29
        && coverage.post_ledger_production_effect_ids.is_empty()
        && coverage.ledger_production_effect_ids + coverage.post_ledger_production_effect_ids.len()
            == inventory.production_effects.len();
    let production_inventory_is_exact = production_ids.len() == 29
        && sorted_unique_positive(&production_ids)
        && inventory.production_effects == presentation_effects
        && inventory
            .production_effects
            .iter()
            .all(|effect| !effect.full_name.trim().is_empty())
        && production_ids.iter().all(|effect_id| {
            runtime.effect_runtime_transfer_enabled(*effect_id)
                || external_state_ids.contains(effect_id)
        });
    let candidate_inventory_is_exact = candidate_ids == [997_520, 2_110_060, 2_110_078, 2_110_092]
        && sorted_unique_positive(&candidate_ids)
        && inventory.remaining_candidates.iter().all(|candidate| {
            candidate.disposition == "candidate-fail-closed"
                && !candidate.full_name.trim().is_empty()
                && !candidate.remaining_proof_obligation.trim().is_empty()
                && !production_ids.contains(&candidate.effect_id)
                && !runtime.effect_runtime_transfer_enabled(candidate.effect_id)
                && !external_state_ids.contains(&candidate.effect_id)
        });
    let external_state_inventory_is_exact = external_state_ids == [2_404_261]
        && production_ids
            .iter()
            .filter(|effect_id| !runtime.effect_runtime_transfer_enabled(**effect_id))
            .copied()
            .collect::<Vec<_>>()
            == external_state_ids;

    if !inventory_identity_is_exact
        || !presentation_identity_is_exact
        || !external_state_identity_is_exact
        || !policy_is_fail_closed
        || !review_coverage_is_exact
        || !production_inventory_is_exact
        || !candidate_inventory_is_exact
        || !external_state_inventory_is_exact
    {
        return Err("bundled BPSR rDPS promotion inventory drifted from runtime authority".into());
    }
    Ok(())
}

#[derive(Debug)]
struct RdpsRuntimeRegistry {
    deployment_id: String,
    default_identity: (String, String),
    default_identity_by_build: HashMap<String, (String, String)>,
    by_identity: HashMap<(String, String), RdpsRuntimeConfig>,
}

static RDPS_RUNTIME_REGISTRY: OnceLock<Result<RdpsRuntimeRegistry, String>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromotedRemoteEffectMagnitudeModel {
    CounterfactualReplay,
}

pub(crate) fn rdps_runtime_config() -> Result<&'static RdpsRuntimeConfig, String> {
    let registry = rdps_runtime_registry()?;
    registry
        .by_identity
        .get(&registry.default_identity)
        .ok_or_else(|| "bundled BPSR rDPS registry has no default formula pack".into())
}

pub(crate) fn rdps_runtime_config_for(
    deployment_id: &str,
    game_build: &str,
) -> Result<Option<&'static RdpsRuntimeConfig>, String> {
    let registry = rdps_runtime_registry()?;
    if deployment_id != registry.deployment_id {
        return Ok(None);
    }
    Ok(registry
        .default_identity_by_build
        .get(game_build)
        .and_then(|identity| registry.by_identity.get(identity)))
}

pub(crate) fn rdps_runtime_config_for_identity(
    deployment_id: &str,
    game_build: &str,
    protocol_pack_digest: &str,
) -> Result<Option<&'static RdpsRuntimeConfig>, String> {
    let registry = rdps_runtime_registry()?;
    if deployment_id != registry.deployment_id {
        return Ok(None);
    }
    Ok(registry
        .by_identity
        .get(&(game_build.to_owned(), protocol_pack_digest.to_owned())))
}

fn rdps_runtime_registry() -> Result<&'static RdpsRuntimeRegistry, String> {
    RDPS_RUNTIME_REGISTRY
        .get_or_init(|| {
            let base_value: serde_json::Value = serde_json::from_str(include_str!(
                "../game-data/runtime/rdps-formula-runtime.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR rDPS formula pack is invalid: {error}"))?;
            let overrides: RdpsRuntimeBuildOverrides = serde_json::from_str(include_str!(
                "../game-data/runtime/rdps-formula-runtime-overrides.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR rDPS formula overrides are invalid: {error}"))?;
            let base: RdpsRuntimeConfig = serde_json::from_value(base_value.clone())
                .map_err(|error| format!("bundled BPSR rDPS formula pack is invalid: {error}"))?;
            base.validate()?;
            validate_rdps_promotion_inventory(&base)?;
            if overrides.schema_version != RDPS_RUNTIME_SCHEMA_VERSION
                || overrides.deployment_id != base.deployment_id
            {
                return Err(
                    "bundled BPSR rDPS formula overrides have an unsupported identity".into(),
                );
            }

            let deployment_id = base.deployment_id.clone();
            let default_identity = (base.game_build.clone(), base.protocol_pack_digest.clone());
            let mut default_identity_by_build =
                HashMap::from([(base.game_build.clone(), default_identity.clone())]);
            let mut by_identity = HashMap::from([(default_identity.clone(), base)]);
            for build_override in overrides.builds {
                if build_override.game_build.is_empty()
                    || !is_prefixed_sha256(&build_override.protocol_pack_digest)
                    || by_identity.contains_key(&(
                        build_override.game_build.clone(),
                        build_override.protocol_pack_digest.clone(),
                    ))
                {
                    return Err(
                        "bundled BPSR rDPS formula overrides contain a duplicate or invalid identity"
                            .into(),
                    );
                }
                let mut value = base_value.clone();
                merge_json_object(&mut value, build_override.patch);
                let config: RdpsRuntimeConfig = serde_json::from_value(value).map_err(|error| {
                    format!("bundled BPSR rDPS formula override is invalid: {error}")
                })?;
                if config.deployment_id != deployment_id
                    || config.game_build != build_override.game_build
                    || config.protocol_pack_digest != build_override.protocol_pack_digest
                {
                    return Err(
                        "bundled BPSR rDPS formula override changed its declared identity".into(),
                    );
                }
                config.validate()?;
                let identity = (config.game_build.clone(), config.protocol_pack_digest.clone());
                default_identity_by_build
                    .entry(config.game_build.clone())
                    .or_insert_with(|| identity.clone());
                by_identity.insert(identity, config);
            }

            Ok(RdpsRuntimeRegistry {
                deployment_id,
                default_identity,
                default_identity_by_build,
                by_identity,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn merge_json_object(base: &mut serde_json::Value, patch: serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base), serde_json::Value::Object(patch)) => {
            for (key, value) in patch {
                merge_json_object(base.entry(key).or_insert(serde_json::Value::Null), value);
            }
        }
        (base, patch) => *base = patch,
    }
}

fn numeric_build(build: &str) -> u64 {
    build.parse().unwrap_or_default()
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

pub(crate) fn promoted_remote_effect_magnitude_model(
    effect_id: i64,
) -> Result<Option<PromotedRemoteEffectMagnitudeModel>, String> {
    let runtime = rdps_runtime_config()?;
    // This selects how validation preserves the remote status magnitude; it
    // does not authorize provider credit. Production transfer is gated by the
    // runtime promotion policy and the effect-specific projector allowlist.
    Ok((effect_id == runtime.harmony_grace.effect_id)
        .then_some(PromotedRemoteEffectMagnitudeModel::CounterfactualReplay))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_from_value(value: serde_json::Value) -> RdpsRuntimeConfig {
        serde_json::from_value(value).expect("test runtime config should deserialize")
    }

    fn bundled_runtime_value() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../game-data/runtime/rdps-formula-runtime.v1.json"
        ))
        .expect("bundled runtime config should parse")
    }

    #[test]
    fn critical_damage_interpretation_and_authority_must_advance_together() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert_eq!(
            current.critical_damage_factor_interpretation,
            CriticalDamageFactorInterpretation::AdditiveBonus,
        );
        assert!(
            current
                .policy
                .critical_damage_factor_interpretation_authority
        );
        assert!(current.validate().is_ok());

        let mut interpretation_without_authority = base.clone();
        interpretation_without_authority["policy"]["critical_damage_factor_interpretation_authority"] =
            serde_json::Value::Bool(false);
        assert!(
            runtime_from_value(interpretation_without_authority)
                .validate()
                .is_err(),
            "a resolved interpretation without exact-build authority must fail",
        );

        let mut authority_without_interpretation = base.clone();
        authority_without_interpretation["critical_damage_factor_interpretation"] =
            serde_json::Value::String("unresolved".into());
        assert!(
            runtime_from_value(authority_without_interpretation)
                .validate()
                .is_err(),
            "authority without a resolved exact-build interpretation must fail",
        );

        let mut wrong_resolved_interpretation = base.clone();
        wrong_resolved_interpretation["critical_damage_factor_interpretation"] =
            serde_json::Value::String("direct_total".into());
        assert!(
            runtime_from_value(wrong_resolved_interpretation)
                .validate()
                .is_err(),
            "Team Luck critical authority is exact to additive_bonus, not any resolved candidate",
        );

        let mut altered_proof = base;
        altered_proof["team_luck"]["critical_damage_ratio_proof"]["authority_pairs"] =
            serde_json::json!(20);
        assert!(
            runtime_from_value(altered_proof).validate().is_err(),
            "the sealed ratio-proof receipt must not be weakened",
        );
    }

    #[test]
    fn promotion_blockers_are_known_unique_and_match_runtime_authority() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(
            current
                .inspiration
                .base_lucky_damage_dependency
                .is_current_authority(),
            "the sealed base Luck dependency receipt must validate"
        );
        assert!(current.validate().is_ok());
        assert_eq!(
            current.promotion_blockers(),
            ["party-support-formula-frontier"]
        );

        let mut duplicate = base.clone();
        duplicate["promotion_blockers"]
            .as_array_mut()
            .expect("promotion blockers should be an array")
            .push(serde_json::Value::String(
                "party-support-formula-frontier".into(),
            ));
        assert!(runtime_from_value(duplicate).validate().is_err());

        let mut unknown = base.clone();
        unknown["promotion_blockers"][0] =
            serde_json::Value::String("future-unreviewed-gate".into());
        assert!(runtime_from_value(unknown).validate().is_err());

        let mut empty_while_blocked = base;
        empty_while_blocked["promotion_blockers"] = serde_json::Value::Array(Vec::new());
        assert!(runtime_from_value(empty_while_blocked).validate().is_err());
    }

    #[test]
    fn protocol_pack_digest_is_part_of_the_runtime_identity() {
        let mut missing_prefix = bundled_runtime_value();
        missing_prefix["protocol_pack_digest"] = serde_json::Value::String(
            "f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395".into(),
        );
        assert!(runtime_from_value(missing_prefix).validate().is_err());

        let mut wrong_length = bundled_runtime_value();
        wrong_length["protocol_pack_digest"] = serde_json::Value::String("sha256:abcd".into());
        assert!(runtime_from_value(wrong_length).validate().is_err());
    }

    #[test]
    fn target_vulnerability_component_gate_requires_its_own_complete_authority() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(
            current.target_vulnerability_runtime_transfer_effect_ids(),
            &[55_228]
        );
        assert!(current.effect_runtime_transfer_enabled(55_228));
        assert!(!current.runtime_promotion_allowed());

        let mut premature = base.clone();
        premature["target_vulnerability"]["formula_specific_conservation_authority"] =
            serde_json::Value::Bool(false);
        assert!(
            runtime_from_value(premature).validate().is_err(),
            "an effect ID cannot remain enabled after any component authority is removed",
        );

        let mut independently_proven = base;
        for field in [
            "current_build_formula_authority",
            "server_integer_counterfactual_authority",
            "formula_specific_conservation_authority",
        ] {
            independently_proven["target_vulnerability"][field] = serde_json::Value::Bool(true);
        }
        independently_proven["target_vulnerability"]["runtime_transfer_effect_ids"] =
            serde_json::json!([55_228]);
        let independently_proven = runtime_from_value(independently_proven);
        assert!(independently_proven.validate().is_ok());
        assert!(!independently_proven.runtime_promotion_allowed());
        assert!(independently_proven.has_any_runtime_transfer_enabled());
        assert!(independently_proven.effect_runtime_transfer_enabled(55_228));
        assert!(!independently_proven.effect_runtime_transfer_enabled(55_229));
    }

    #[test]
    fn team_luck_promotes_single_and_combined_current_build_components() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(!current.runtime_promotion_allowed());
        assert!(current.team_luck.critical_damage_runtime_transfer_enabled);
        assert!(current.team_luck.lucky_damage_runtime_transfer_enabled);
        assert!(current.team_luck.combined_critical_lucky_enabled);
        assert!(
            current
                .team_luck
                .combined_damage_current_build_packet_component_authority
        );
        assert!(
            current
                .team_luck
                .combined_damage_exact_rational_cross_term_authority
        );
        assert!(
            current
                .team_luck
                .combined_damage_protocol_pack_migration_authority
        );
        assert!(current.team_luck.combined_damage_proof.is_valid());
        assert!(
            current
                .team_luck
                .critical_damage_current_build_lifecycle_authority
        );
        assert!(
            current
                .team_luck
                .critical_damage_current_build_executor_authority
        );
        assert!(
            current
                .team_luck
                .critical_damage_exact_rational_attribution_authority
        );
        assert!(
            current
                .team_luck
                .critical_damage_protocol_pack_migration_authority
        );
        assert!(current.team_luck.critical_damage_ratio_proof.is_valid());
        assert_eq!(
            current
                .team_luck
                .critical_damage_authorized_protocol_pack_digests,
            [
                "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
                "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395",
                "sha256:f975b4acade288bc87392bfeaae464873f7af1d3060be56023ff69d176905a3e",
                "sha256:f4eb9db52ee232ecc7845119cb7fd909fb0f2c2d4fee33fe587b4235b656773c",
                "sha256:58c849d0264261efe8220b7dd5ce50fd7e3f8fa31980941e823a18306f30c7d1",
                "sha256:9de9c7eccc5309686ad4e982968aef67c1d6cf6f59e71762c457ce8ce8f23ac3",
                "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
            ]
        );
        assert!(
            current
                .team_luck
                .lucky_damage_current_build_lifecycle_authority
        );
        assert!(
            current
                .team_luck
                .lucky_damage_current_build_executor_authority
        );
        assert!(
            current
                .team_luck
                .lucky_damage_exact_rational_attribution_authority
        );
        assert!(
            current
                .team_luck
                .lucky_damage_protocol_pack_migration_authority
        );
        assert_eq!(
            current
                .team_luck
                .lucky_damage_authorized_protocol_pack_digests,
            [
                "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
                "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395",
                "sha256:f975b4acade288bc87392bfeaae464873f7af1d3060be56023ff69d176905a3e",
                "sha256:f4eb9db52ee232ecc7845119cb7fd909fb0f2c2d4fee33fe587b4235b656773c",
                "sha256:58c849d0264261efe8220b7dd5ce50fd7e3f8fa31980941e823a18306f30c7d1",
                "sha256:9de9c7eccc5309686ad4e982968aef67c1d6cf6f59e71762c457ce8ce8f23ac3",
                "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
            ]
        );
        assert!(!current.team_luck.server_integer_counterfactual_authority);
        assert!(current.team_luck.unresolved_overlap_fails_closed);
        assert_eq!(
            current.team_luck.lucky_damage_routes,
            TEAM_LUCK_CURRENT_LUCKY_DAMAGE_ROUTES
        );
        assert!(
            current
                .team_luck
                .is_lucky_damage_route(Some(2_031_101), Some(3))
        );
        assert!(
            !current
                .team_luck
                .is_lucky_damage_route(Some(2_031_101), Some(4))
        );
        assert!(current.effect_runtime_transfer_enabled(current.team_luck.effect_id));

        for field in [
            "critical_damage_current_build_lifecycle_authority",
            "critical_damage_current_build_executor_authority",
            "critical_damage_exact_rational_attribution_authority",
            "critical_damage_protocol_pack_migration_authority",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["team_luck"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        for field in [
            "lucky_damage_current_build_lifecycle_authority",
            "lucky_damage_current_build_executor_authority",
            "lucky_damage_exact_rational_attribution_authority",
            "lucky_damage_protocol_pack_migration_authority",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["team_luck"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let mut missing_critical_digest_authority = base.clone();
        missing_critical_digest_authority["team_luck"]["critical_damage_authorized_protocol_pack_digests"] =
            serde_json::json!([]);
        assert!(
            runtime_from_value(missing_critical_digest_authority)
                .validate()
                .is_err()
        );

        let mut invented_server_authority = base.clone();
        invented_server_authority["team_luck"]["server_integer_counterfactual_authority"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(invented_server_authority)
                .validate()
                .is_err()
        );

        let mut disabled_combined = base.clone();
        disabled_combined["team_luck"]["combined_critical_lucky_enabled"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(disabled_combined).validate().is_err());

        let mut missing_combined_authority = base.clone();
        missing_combined_authority["team_luck"]["combined_damage_exact_rational_cross_term_authority"] =
            serde_json::Value::Bool(false);
        assert!(
            runtime_from_value(missing_combined_authority)
                .validate()
                .is_err()
        );

        let mut wrong_combined_receipt = base.clone();
        wrong_combined_receipt["team_luck"]["combined_damage_proof"]["combined_emitted_events"] =
            serde_json::json!(972);
        assert!(
            runtime_from_value(wrong_combined_receipt)
                .validate()
                .is_err()
        );

        let mut wrong_route = base.clone();
        wrong_route["team_luck"]["lucky_damage_routes"][0]["hit_event_id"] = serde_json::json!(4);
        assert!(runtime_from_value(wrong_route).validate().is_err());

        let mut guessed_overlap = base.clone();
        guessed_overlap["team_luck"]["unresolved_overlap_fails_closed"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(guessed_overlap).validate().is_err());

        let mut wrong_projection = base;
        wrong_projection["team_luck"]["rational_integer_projection"] =
            serde_json::Value::String("per-hit-floor".into());
        assert!(runtime_from_value(wrong_projection).validate().is_err());
    }

    #[test]
    fn poison_explosion_vulnerability_is_exact_current_build_authority_only() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        let config = &current.poison_explosion_vulnerability;
        assert_eq!(config.effect_id, 2_110_099);
        assert_eq!(config.provider_imagine_ability_id, 3_942);
        assert_eq!(config.provider_imagine_item_id, 3_000_041);
        assert_eq!(config.required_effect_level, 99);
        assert_eq!((config.minimum_stacks, config.maximum_stacks), (1, 5));
        assert_eq!(config.duration_millis, 8_000);
        assert_eq!(
            config.vulnerability_basis_points_per_stack_by_tier,
            [80, 160, 240, 320, 400]
        );
        assert_eq!(
            config.conflicting_target_effect_ids,
            [55_228, 2_100_107, 2_110_078, 2_110_092]
        );
        assert!(config.runtime_transfer_enabled);

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(
            !historical
                .poison_explosion_vulnerability
                .runtime_transfer_enabled
        );

        let mut wrong_tier = base.clone();
        wrong_tier["poison_explosion_vulnerability"]["vulnerability_basis_points_per_stack_by_tier"]
            [4] = serde_json::json!(401);
        assert!(runtime_from_value(wrong_tier).validate().is_err());

        let mut missing_conflict = base;
        missing_conflict["poison_explosion_vulnerability"]["conflicting_target_effect_ids"] =
            serde_json::json!([55228, 2100107, 2110078]);
        assert!(runtime_from_value(missing_conflict).validate().is_err());
    }

    #[test]
    fn celestial_guardian_vulnerability_is_exact_current_build_component_authority_only() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        let config = &current.celestial_guardian_vulnerability;
        assert_eq!(config.effect_id, 2_110_167);
        assert_eq!(config.provider_imagine_ability_id, 3_982);
        assert_eq!(config.provider_imagine_item_id, 3_001_001);
        assert_eq!(config.required_effect_level, 67);
        assert_eq!(config.required_stacks, 1);
        assert_eq!(config.duration_millis, 10_000);
        assert_eq!(
            config.vulnerability_basis_points_by_tier,
            [60, 120, 180, 240, 300]
        );
        assert!(!config.element_resistance_component_transfer_enabled);
        assert!(config.runtime_transfer_enabled);

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(
            !historical
                .celestial_guardian_vulnerability
                .runtime_transfer_enabled
        );

        let mut guessed_resistance = base.clone();
        guessed_resistance["celestial_guardian_vulnerability"]["element_resistance_component_transfer_enabled"] =
            serde_json::json!(true);
        assert!(runtime_from_value(guessed_resistance).validate().is_err());

        let mut wrong_tier = base;
        wrong_tier["celestial_guardian_vulnerability"]["vulnerability_basis_points_by_tier"][4] =
            serde_json::json!(301);
        assert!(runtime_from_value(wrong_tier).validate().is_err());
    }

    #[test]
    fn inspiration_promotes_versioned_combined_route_and_exact_dependencies() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(current.inspiration.critical_chance_runtime_transfer_enabled);
        assert!(current.inspiration.lucky_chance_runtime_transfer_enabled);
        assert!(
            current
                .inspiration
                .combined_critical_lucky_runtime_transfer_enabled
        );
        assert!(
            current
                .inspiration
                .combined_reconciliation
                .is_current_receipt()
        );
        assert!(
            current
                .inspiration
                .recipient_dependency_runtime_transfer_enabled
        );
        assert!(
            current
                .inspiration
                .recipient_dependency
                .is_current_authority()
        );
        assert!(
            current
                .inspiration
                .base_lucky_damage_dependency_runtime_transfer_enabled
        );
        assert!(
            current
                .inspiration
                .base_lucky_damage_dependency
                .is_current_authority()
        );
        assert_eq!(
            current
                .inspiration
                .base_lucky_damage_dependency
                .lucky_damage_raw_delta(650, 150),
            Some(37)
        );
        assert_eq!(
            current
                .inspiration
                .base_lucky_damage_dependency
                .lucky_damage_raw_delta(800, 150),
            Some(38),
            "integer floor remainder belongs to the recipient's exact current Luck state"
        );
        assert!(!current.inspiration.runtime_transfer_enabled);
        assert!(current.inspiration.chance_proof.is_current_authority());
        assert!(current.inspiration.chance_replay.is_current_authority());
        assert!(current.effect_runtime_transfer_enabled(current.inspiration.effect_id));

        for field in [
            "current_build_lifecycle_authority",
            "current_build_magnitude_authority",
            "exact_rational_chance_attribution_authority",
            "protocol_pack_migration_authority",
            "canonical_conservation_replay_authority",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["inspiration"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let mut altered_receipt = base.clone();
        altered_receipt["inspiration"]["chance_proof"]["emitted_critical_events"] =
            serde_json::json!(10_614);
        assert!(runtime_from_value(altered_receipt).validate().is_err());

        let mut altered_replay = base.clone();
        altered_replay["inspiration"]["chance_replay"]["emitted_contribution_events"] =
            serde_json::json!(13_617);
        assert!(runtime_from_value(altered_replay).validate().is_err());

        let mut guessed_combined = base.clone();
        guessed_combined["inspiration"]["combined_critical_lucky_runtime_transfer_enabled"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(guessed_combined).validate().is_err());

        let mut altered_combined_receipt = base.clone();
        altered_combined_receipt["inspiration"]["combined_reconciliation"]["general_route_decision_count"] =
            serde_json::json!(325);
        assert!(
            runtime_from_value(altered_combined_receipt)
                .validate()
                .is_err()
        );

        let mut guessed_dependency = base.clone();
        guessed_dependency["inspiration"]["recipient_dependency"]["proof_content_sha256"] =
            serde_json::Value::String("unreviewed".into());
        assert!(runtime_from_value(guessed_dependency).validate().is_err());

        let mut altered_dependency_replay = bundled_runtime_value();
        altered_dependency_replay["inspiration"]["recipient_dependency"]["replay"]["dependency_increment"] =
            serde_json::json!(7_546_367);
        assert!(
            runtime_from_value(altered_dependency_replay)
                .validate()
                .is_err()
        );

        let mut altered_base_luck_proof = base.clone();
        altered_base_luck_proof["inspiration"]["base_lucky_damage_dependency"]["exact_marginal_comparisons"] =
            serde_json::json!(10);
        assert!(
            runtime_from_value(altered_base_luck_proof)
                .validate()
                .is_err()
        );

        let mut altered_base_luck_replay = base;
        altered_base_luck_replay["inspiration"]["base_lucky_damage_dependency"]["replay"]["dependency_increment"] =
            serde_json::json!(1_114);
        assert!(
            runtime_from_value(altered_base_luck_replay)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn critical_cold_requires_exact_identity_authority_and_stays_closed_on_historical_build() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(current.critical_cold.effect_id, 2_204_471);
        assert_eq!(current.critical_cold.root_effect_id, 2_204_470);
        assert_eq!(
            current.critical_cold.official_en_us_source_name,
            "Critical Cold"
        );
        assert!(current.critical_cold.child_en_us_name.is_none());
        assert_eq!(current.critical_cold.child_design_name, "暴击之寒_队友暴击");
        assert!(current.effect_runtime_transfer_enabled(current.critical_cold.effect_id));

        for field in [
            "current_build_lifecycle_authority",
            "current_build_provider_ownership_authority",
            "current_build_magnitude_authority",
            "reuses_inspiration_critical_stage_authority",
            "unresolved_overlap_fails_closed",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["critical_cold"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let mut altered_replay = base.clone();
        altered_replay["critical_cold"]["replay"]["emitted_contribution_events"] =
            serde_json::json!(39_561);
        assert!(runtime_from_value(altered_replay).validate().is_err());

        let mut promoted_offline_oracle = base.clone();
        promoted_offline_oracle["critical_cold"]["offline_oracle"]["runtime_authority"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(promoted_offline_oracle)
                .validate()
                .is_err()
        );

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.critical_cold.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(historical.critical_cold.effect_id));
    }

    #[test]
    fn synergy_crit_field_uses_description_authority_and_child_status_occurrence() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(current.synergy_crit_field.effect_id, 997_538);
        assert_eq!(current.synergy_crit_field.root_effect_id, 997_536);
        assert_eq!(current.synergy_crit_field.aura_effect_id, 997_537);
        assert_eq!(current.synergy_crit_field.source_rogue_entry_id, 209);
        assert_eq!(current.synergy_crit_field.description_id, 110_901);
        assert_eq!(current.synergy_crit_field.critical_damage_raw_delta, 300);
        assert!(current.effect_runtime_transfer_enabled(current.synergy_crit_field.effect_id));
        assert!(
            !current.effect_runtime_transfer_enabled(current.synergy_crit_field.root_effect_id)
        );
        assert!(
            !current.effect_runtime_transfer_enabled(current.synergy_crit_field.aura_effect_id)
        );

        for field in [
            "game_description_formula_authority",
            "game_description_party_scope_authority",
            "exact_child_status_identity_authority",
            "additive_critical_stage_authority",
            "unresolved_overlap_fails_closed",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["synergy_crit_field"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.synergy_crit_field.runtime_transfer_enabled);
        assert!(
            !historical.effect_runtime_transfer_enabled(historical.synergy_crit_field.effect_id)
        );
    }

    #[test]
    fn element_sharing_uses_description_authority_and_exact_child_status() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(current.element_sharing.effect_id, 997_513);
        assert_eq!(current.element_sharing.root_effect_id, 997_512);
        assert_eq!(current.element_sharing.source_rogue_entry_id, 196);
        assert_eq!(current.element_sharing.description_id, 109_601);
        assert_eq!(current.element_sharing.element_damage_raw_delta, 2_000);
        assert!(current.effect_runtime_transfer_enabled(current.element_sharing.effect_id));
        assert!(!current.effect_runtime_transfer_enabled(current.element_sharing.root_effect_id));
        assert!(
            current
                .harmony_grace
                .remote_paired_output_formula_effect_ids
                .binary_search(&current.element_sharing.effect_id)
                .is_ok()
        );

        for field in [
            "game_description_formula_authority",
            "game_description_party_scope_authority",
            "exact_child_status_identity_authority",
            "additive_all_plus_property_stage_authority",
            "unresolved_overlap_fails_closed",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["element_sharing"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.element_sharing.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(historical.element_sharing.effect_id));
    }

    #[test]
    fn coordinated_strike_uses_description_authority_and_exact_child_status() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(current.coordinated_strike.effect_id, 997_511);
        assert_eq!(current.coordinated_strike.root_effect_id, 997_510);
        assert_eq!(current.coordinated_strike.source_rogue_entry_id, 195);
        assert_eq!(current.coordinated_strike.description_id, 109_501);
        assert_eq!(current.coordinated_strike.attack_raw_percent_delta, 1_500);
        assert!(current.effect_runtime_transfer_enabled(current.coordinated_strike.effect_id));
        assert!(
            !current.effect_runtime_transfer_enabled(current.coordinated_strike.root_effect_id)
        );
        assert!(
            current
                .harmony_grace
                .remote_paired_output_formula_effect_ids
                .binary_search(&current.coordinated_strike.effect_id)
                .is_ok()
        );

        for field in [
            "game_description_formula_authority",
            "game_description_party_scope_authority",
            "exact_child_status_identity_authority",
            "additive_attack_percent_stage_authority",
            "same_stage_provider_conservation_authority",
            "unresolved_overlap_fails_closed",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["coordinated_strike"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.coordinated_strike.runtime_transfer_enabled);
        assert!(
            !historical.effect_runtime_transfer_enabled(historical.coordinated_strike.effect_id)
        );
    }

    #[test]
    fn all_class_aura_uses_exact_role_scaled_attack_formula() {
        let current = rdps_runtime_config().unwrap();
        assert_eq!(current.all_class_aura.effect_id, 998_542);
        assert_eq!(current.all_class_aura.source_rogue_entry_id, 103);
        assert_eq!(current.all_class_aura.description_id, 100_301);
        assert_eq!(current.all_class_aura.required_level, 42);
        assert_eq!(current.all_class_aura.base_attack_raw_percent_delta, 500);
        assert_eq!(
            current.all_class_aura.per_distinct_role_raw_percent_delta,
            500
        );
        assert_eq!(
            current.all_class_aura.maximum_attack_raw_percent_delta,
            2_000
        );
        assert!(current.effect_runtime_transfer_enabled(current.all_class_aura.effect_id));

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.all_class_aura.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(historical.all_class_aura.effect_id));
    }

    #[test]
    fn enhanced_synergy_uses_exact_phy_mag_boost_bucket() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(current.enhanced_synergy.effect_id, 997_518);
        assert_eq!(current.enhanced_synergy.root_effect_id, 997_517);
        assert_eq!(current.enhanced_synergy.source_rogue_entry_id, 199);
        assert_eq!(current.enhanced_synergy.description_id, 109_901);
        assert_eq!(current.enhanced_synergy.physical_boost_attribute_id, 12_550);
        assert_eq!(current.enhanced_synergy.magical_boost_attribute_id, 12_570);
        assert_eq!(current.enhanced_synergy.boost_raw_delta, 1_000);
        assert!(current.effect_runtime_transfer_enabled(current.enhanced_synergy.effect_id));
        assert!(!current.effect_runtime_transfer_enabled(current.enhanced_synergy.root_effect_id));
        assert!(
            current
                .harmony_grace
                .remote_paired_output_formula_effect_ids
                .binary_search(&current.enhanced_synergy.effect_id)
                .is_ok()
        );

        for field in [
            "game_description_formula_authority",
            "game_description_party_scope_authority",
            "exact_child_status_identity_authority",
            "packet_final_boost_attributes_authority",
            "corrected_calculator_multiplicative_boost_stage_authority",
            "unresolved_overlap_fails_closed",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["enhanced_synergy"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.enhanced_synergy.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(historical.enhanced_synergy.effect_id));
    }

    #[test]
    fn blessing_uses_exact_additive_general_damage_bucket() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        let config = &current.blessing;
        assert_eq!(config.effect_id, 2_100_154);
        assert_eq!(config.source_skill_id, 3_401);
        assert_eq!(config.required_level, 1);
        assert_eq!(config.required_stacks, 1);
        assert_eq!(config.duration_millis, 10_000);
        assert_eq!(config.general_damage_attribute_id, 12_670);
        assert_eq!(config.general_damage_raw_delta, 3_000);
        assert!(current.effect_runtime_transfer_enabled(config.effect_id));

        for field in [
            "exact_current_build_buff_row_authority",
            "game_description_formula_authority",
            "game_description_party_scope_authority",
            "exact_current_build_fight_attribute_row_authority",
            "packet_final_general_damage_attribute_route_authority",
            "corrected_calculator_additive_general_damage_stage_authority",
            "unresolved_stacking_fails_closed",
            "unresolved_overlap_fails_closed",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["blessing"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let mut wrong_accounting = base;
        wrong_accounting["blessing"]["accounting_method"] =
            serde_json::Value::String("standalone-times-1.30".into());
        assert!(runtime_from_value(wrong_accounting).validate().is_err());

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.blessing.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(historical.blessing.effect_id));
    }

    #[test]
    fn synergy_luck_field_uses_exact_external_imagine_proc_identity() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        let config = &current.synergy_luck_field;
        assert_eq!(config.effect_id, 997_534);
        assert_eq!(config.root_effect_id, 997_533);
        assert_eq!(config.source_rogue_entry_id, 208);
        assert_eq!(config.description_id, 110_801);
        assert_eq!(config.granted_imagine_ability_id, 3_937);
        assert_eq!(config.granted_imagine_item_id, 3_000_016);
        assert_eq!(config.granted_passive_effect_id, 3_210_080);
        assert_eq!(config.produced_damage_ability_id, 3_210_081);
        assert_eq!(config.produced_damage_attr_id, 2_321_008_101);
        assert!(current.effect_runtime_transfer_enabled(config.effect_id));
        assert!(!current.effect_runtime_transfer_enabled(config.root_effect_id));
        assert!(
            current
                .harmony_grace
                .remote_paired_output_formula_effect_ids
                .binary_search(&config.effect_id)
                .is_ok()
        );

        for field in [
            "game_description_trigger_and_party_scope_authority",
            "exact_child_status_identity_authority",
            "exact_imagine_passive_family_authority",
            "exact_produced_damage_action_authority",
            "exact_recipient_loadout_absence_required",
            "observed_final_direct_output_authority",
            "unresolved_overlap_fails_closed",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["synergy_luck_field"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.synergy_luck_field.runtime_transfer_enabled);
        assert!(
            !historical.effect_runtime_transfer_enabled(historical.synergy_luck_field.effect_id)
        );
    }

    #[test]
    fn attribute_transfer_promotes_only_formula_complete_substat_lanes() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(current.attribute_transfer.effect_id, 997_515);
        assert_eq!(current.attribute_transfer.root_effect_id, 997_514);
        assert_eq!(current.attribute_transfer.source_rogue_entry_id, 197);
        assert_eq!(current.attribute_transfer.description_id, 109_701);
        assert_eq!(current.attribute_transfer.substat_raw_delta, 1_000);
        assert!(
            current
                .attribute_transfer
                .critical_chance_runtime_transfer_enabled
        );
        assert!(
            current
                .attribute_transfer
                .lucky_chance_runtime_transfer_enabled
        );
        assert!(
            current
                .attribute_transfer
                .versatility_runtime_transfer_enabled
        );
        assert!(!current.attribute_transfer.mastery_runtime_transfer_enabled);
        assert!(!current.attribute_transfer.haste_runtime_transfer_enabled);
        assert!(current.effect_runtime_transfer_enabled(997_515));
        assert!(!current.effect_runtime_transfer_enabled(997_514));

        let historical = rdps_runtime_config_for_identity(
            "global",
            "24252055",
            "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
        )
        .unwrap()
        .unwrap();
        assert!(!historical.attribute_transfer.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(997_515));
    }

    #[test]
    fn life_wave_is_exact_current_build_selected_lane_authority_only() {
        let current = runtime_from_value(bundled_runtime_value());
        assert!(current.validate().is_ok());
        let config = &current.life_wave;
        assert_eq!(config.effect_id, 2_302_421);
        assert_eq!(config.source_config_id, 2_302_420);
        assert_eq!(config.module_effect_id, 2_404);
        assert_eq!(config.duration_millis, 5_000);
        assert_eq!(config.level_five_bonus_basis_points, 600);
        assert_eq!(config.level_six_bonus_basis_points, 1_000);
        assert!(config.runtime_transfer_enabled);
        assert!(current.effect_runtime_transfer_enabled(config.effect_id));

        let historical = rdps_runtime_config_for_identity(
            "global",
            "24252055",
            "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
        )
        .unwrap()
        .unwrap();
        assert!(!historical.life_wave.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(config.effect_id));
    }

    #[test]
    fn tactical_blessing_uses_exact_simultaneous_crit_and_luck_child() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        let config = &current.tactical_blessing;
        assert_eq!(config.effect_id, 997_570);
        assert_eq!(config.root_effect_id, 997_557);
        assert_eq!(config.source_rogue_entry_id, 349);
        assert_eq!(config.description_id, 124_901);
        assert_eq!(config.duration_millis, 10_000);
        assert_eq!(config.critical_chance_raw_delta, 1_000);
        assert_eq!(config.lucky_chance_raw_delta, 1_000);
        assert!(current.effect_runtime_transfer_enabled(config.effect_id));
        assert!(!current.effect_runtime_transfer_enabled(config.root_effect_id));
        assert!(
            current
                .harmony_grace
                .remote_paired_output_formula_effect_ids
                .binary_search(&config.effect_id)
                .is_ok()
        );

        for field in [
            "game_description_formula_authority",
            "game_description_party_scope_authority",
            "exact_child_status_identity_authority",
            "exact_static_lifecycle_authority",
            "corrected_calculator_final_substat_stage_authority",
            "reuses_inspiration_chance_stage_authority",
            "unresolved_overlap_fails_closed",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["tactical_blessing"][field] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("historical formula identity should remain registered");
        assert!(!historical.tactical_blessing.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(config.effect_id));
    }

    #[test]
    fn registry_keeps_same_build_protocol_identities_separate() {
        let prior_digest =
            "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b";
        let base_digest = "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395";
        let latest_digest =
            "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae";
        let base = rdps_runtime_config_for_identity("global", "24687926", base_digest)
            .unwrap()
            .expect("base formula protocol identity should be registered");
        let latest = rdps_runtime_config_for_identity("global", "24687926", latest_digest)
            .unwrap()
            .expect("latest exact protocol identity should be registered");
        let prior = rdps_runtime_config_for_identity("global", "24687926", prior_digest)
            .unwrap()
            .expect("current-build history protocol identity should be registered");
        assert_eq!(base.game_build, prior.game_build);
        assert_eq!(latest.game_build, prior.game_build);
        assert_ne!(base.protocol_pack_digest, prior.protocol_pack_digest);
        assert_ne!(latest.protocol_pack_digest, base.protocol_pack_digest);
        assert!(base.effect_runtime_transfer_enabled(base.team_luck.effect_id));
        assert!(latest.effect_runtime_transfer_enabled(latest.team_luck.effect_id));
        assert!(prior.effect_runtime_transfer_enabled(prior.team_luck.effect_id));
        assert!(
            rdps_runtime_config_for_identity(
                "global",
                "24687926",
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            rdps_runtime_config_for("global", "24687926")
                .unwrap()
                .expect("build-default identity should remain available")
                .protocol_pack_digest,
            base_digest,
        );
    }

    #[test]
    fn highland_blood_direct_lane_is_current_build_only_and_paired_output_stays_closed() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(current.highland_blood.runtime_transfer_enabled);
        assert!(
            !current
                .highland_blood
                .remote_paired_output_runtime_transfer_enabled
        );
        assert!(current.effect_runtime_transfer_enabled(current.highland_blood.effect_id));

        let registry = rdps_runtime_registry().expect("bundled identities should validate");
        assert!(registry.by_identity.len() >= 8);
        for digest in [
            "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395",
            "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
            "sha256:f975b4acade288bc87392bfeaae464873f7af1d3060be56023ff69d176905a3e",
            "sha256:f4eb9db52ee232ecc7845119cb7fd909fb0f2c2d4fee33fe587b4235b656773c",
            "sha256:58c849d0264261efe8220b7dd5ce50fd7e3f8fa31980941e823a18306f30c7d1",
            "sha256:9de9c7eccc5309686ad4e982968aef67c1d6cf6f59e71762c457ce8ce8f23ac3",
            "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae",
        ] {
            let runtime = rdps_runtime_config_for_identity("global", "24687926", digest)
                .expect("registry lookup should succeed")
                .expect("every exact current-build decoder identity must remain replayable");
            assert!(runtime.validate().is_ok());
            assert!(runtime.highland_blood.runtime_transfer_enabled);
            assert!(
                !runtime
                    .highland_blood
                    .remote_paired_output_runtime_transfer_enabled
            );
        }
        for runtime in registry.by_identity.values() {
            assert!(
                !runtime
                    .highland_blood
                    .remote_paired_output_runtime_transfer_enabled,
                "Highland Blood paired output must remain disabled for build {} protocol {}",
                runtime.game_build,
                runtime.protocol_pack_digest
            );
            assert_eq!(
                runtime.highland_blood.runtime_transfer_enabled,
                runtime.game_build == "24687926",
                "Highland Blood direct authority is exact-current-build only"
            );
            assert_eq!(
                runtime.effect_runtime_transfer_enabled(runtime.highland_blood.effect_id),
                runtime.game_build == "24687926"
            );
        }

        let mut incorrectly_enabled = base;
        incorrectly_enabled["highland_blood"]["remote_paired_output_runtime_transfer_enabled"] =
            serde_json::Value::Bool(true);
        assert!(runtime_from_value(incorrectly_enabled).validate().is_err());

        let mut incorrectly_disabled = bundled_runtime_value();
        incorrectly_disabled["highland_blood"]["runtime_transfer_enabled"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(incorrectly_disabled).validate().is_err());
    }

    #[test]
    fn functional_amp_migrated_rule_activates_only_from_live_exact_evidence() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(
            current
                .functional_amp
                .attack_magic_historical_formula_authority
        );
        assert!(
            current
                .functional_amp
                .attack_magic_target_build_lifecycle_schema_authority
        );
        assert!(
            current
                .functional_amp
                .attack_magic_target_build_formula_authority
        );
        assert!(current.functional_amp.unresolved_overlap_fails_closed);
        assert!(
            !current
                .functional_amp
                .attack_magic_target_build_effect_occurrence_observed
        );
        assert!(
            !current
                .functional_amp
                .attack_magic_target_build_damage_replay_observed
        );
        assert!(
            current
                .functional_amp
                .dormant_activation_requires_observed_lifecycle
        );
        assert!(
            !current
                .functional_amp
                .server_integer_counterfactual_authority
        );
        assert_eq!(current.functional_amp.attack_percent_raw_delta, 360);
        assert_eq!(
            current.functional_amp.accounting_method,
            "observed-final-damage-proportional-stage-share"
        );
        assert!(current.functional_amp.attack_magic_runtime_transfer_enabled);
        assert!(!current.functional_amp.speed_runtime_transfer_enabled);
        assert!(
            current.has_any_runtime_transfer_enabled(),
            "the independently promoted target-vulnerability rule keeps the partial runtime active"
        );
        assert!(current.effect_runtime_transfer_enabled(current.functional_amp.effect_id));
        assert!(current.effect_runtime_transfer_enabled(current.harmony_grace.effect_id));

        let mut transfer_without_migration_authority = base.clone();
        transfer_without_migration_authority["functional_amp"]["attack_magic_target_build_formula_authority"] =
            serde_json::Value::Bool(false);
        assert!(
            runtime_from_value(transfer_without_migration_authority)
                .validate()
                .is_err(),
            "historical proof cannot replace exact target-build migration authority",
        );

        let mut wrong_accounting_method = base.clone();
        wrong_accounting_method["functional_amp"]["accounting_method"] =
            serde_json::Value::String("server-counterfactual-guess".into());
        assert!(
            runtime_from_value(wrong_accounting_method)
                .validate()
                .is_err(),
            "a migrated component must retain the observed-damage proportional contract",
        );

        let mut invented_server_authority = base.clone();
        invented_server_authority["functional_amp"]["server_integer_counterfactual_authority"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(invented_server_authority)
                .validate()
                .is_err()
        );

        let mut speed_riding_on_attack_proof = base;
        speed_riding_on_attack_proof["functional_amp"]["speed_runtime_transfer_enabled"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(speed_riding_on_attack_proof)
                .validate()
                .is_err(),
            "speed action-opportunity credit needs its own proof",
        );
    }

    #[test]
    fn mechanical_power_runtime_is_bound_to_the_exact_conserving_replay() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(current.mechanical_power.runtime_transfer_enabled);
        assert!(
            current
                .mechanical_power
                .class_11_tier_0_current_pack_lifecycle_authority
        );
        assert!(
            current
                .mechanical_power
                .class_11_tier_0_exact_rational_attribution_authority
        );
        assert_eq!(
            current.mechanical_power.accounting_method,
            "observed-final-damage-proportional-stage-share"
        );
        assert!(
            !current
                .mechanical_power
                .damage_stage_operation_order_authority
        );
        assert!(
            !current
                .mechanical_power
                .damage_stage_integer_rounding_authority
        );
        assert!(
            !current
                .mechanical_power
                .server_integer_counterfactual_authority
        );
        assert!(current.mechanical_power.unresolved_overlap_fails_closed);
        assert!(current.mechanical_power.runtime_recipient_class_ids == [11]);
        assert!(current.mechanical_power.runtime_primary_percent_raw_deltas == [750]);
        assert_eq!(
            current
                .mechanical_power
                .production_primary_percent_raw_delta(11),
            Some(750)
        );
        assert_eq!(
            current
                .mechanical_power
                .production_primary_percent_raw_delta(9),
            None
        );
        assert!(current.effect_runtime_transfer_enabled(current.mechanical_power.effect_id));

        let mut missing_rational_authority = base.clone();
        missing_rational_authority["mechanical_power"]["class_11_tier_0_exact_rational_attribution_authority"] =
            serde_json::Value::Bool(false);
        assert!(
            runtime_from_value(missing_rational_authority)
                .validate()
                .is_err(),
            "runtime transfer still requires exact-rational authority",
        );

        let mut exact_rational_reenable = base.clone();
        exact_rational_reenable["mechanical_power"]["replay"]["production_content_sha256"] =
            serde_json::Value::String("0".repeat(64));
        assert!(
            runtime_from_value(exact_rational_reenable)
                .validate()
                .is_err(),
            "production transfer must remain bound to the exact conserving replay",
        );

        let mut wrong_accounting_method = base.clone();
        wrong_accounting_method["mechanical_power"]["accounting_method"] =
            serde_json::Value::String("server-counterfactual-guess".into());
        assert!(
            runtime_from_value(wrong_accounting_method)
                .validate()
                .is_err()
        );

        for field in [
            "damage_stage_operation_order_authority",
            "damage_stage_integer_rounding_authority",
            "server_integer_counterfactual_authority",
        ] {
            let mut invented_server_authority = base.clone();
            invented_server_authority["mechanical_power"][field] = serde_json::Value::Bool(true);
            assert!(
                runtime_from_value(invented_server_authority)
                    .validate()
                    .is_err()
            );
        }

        let mut wrong_projection = base.clone();
        wrong_projection["mechanical_power"]["rational_integer_projection"] =
            serde_json::Value::String("per-hit-floor".into());
        assert!(runtime_from_value(wrong_projection).validate().is_err());

        let mut guessed_overlap = base.clone();
        guessed_overlap["mechanical_power"]["unresolved_overlap_fails_closed"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(guessed_overlap).validate().is_err());

        let mut disabled_with_scope = base;
        disabled_with_scope["mechanical_power"]["runtime_transfer_enabled"] =
            serde_json::Value::Bool(false);
        disabled_with_scope["mechanical_power"]["runtime_recipient_class_ids"] =
            serde_json::json!([11]);
        assert!(runtime_from_value(disabled_with_scope).validate().is_err());
    }

    #[test]
    fn thunderwind_power_remains_owner_only_when_the_whole_pack_is_approved() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(current.thunderwind.recipient_scope, "summon-owner-only");
        assert!(!current.thunderwind.runtime_transfer_enabled);
        assert!(!current.effect_runtime_transfer_enabled(current.thunderwind.effect_id));
        assert!(!current.effect_runtime_transfer_enabled(current.thunderwind.child_effect_id));

        let mut approved = base;
        approved["promotion_state"] = serde_json::Value::String("approved".into());
        approved["promotion_blockers"] = serde_json::json!([]);
        approved["policy"]["party_support_formula_frontier_complete"] =
            serde_json::Value::Bool(true);
        approved["policy"]["runtime_promotion_allowed"] = serde_json::Value::Bool(true);
        let approved = runtime_from_value(approved);
        assert!(approved.validate().is_ok());
        assert!(approved.runtime_promotion_allowed());
        assert!(!approved.effect_runtime_transfer_enabled(approved.thunderwind.effect_id));
        assert!(!approved.effect_runtime_transfer_enabled(approved.thunderwind.child_effect_id));
    }

    #[test]
    fn stat_resonance_requires_exact_current_build_observed_delta_authority() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(current.stat_resonance.runtime_transfer_enabled);
        assert!(
            current
                .stat_resonance
                .current_build_external_lifecycle_authority
        );
        assert!(
            current
                .stat_resonance
                .exact_same_wire_final_attack_marginal_authority
        );
        assert!(
            !current
                .stat_resonance
                .server_integer_counterfactual_authority
        );
        assert!(current.stat_resonance.unresolved_overlap_fails_closed);
        assert!(current.effect_runtime_transfer_enabled(2_207_252));

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("the prior build remains replayable");
        assert!(!historical.stat_resonance.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(2_207_252));

        for authority in [
            "current_build_external_lifecycle_authority",
            "exact_same_wire_final_attack_marginal_authority",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["stat_resonance"][authority] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let mut invented_server_integer = base.clone();
        invented_server_integer["stat_resonance"]["server_integer_counterfactual_authority"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(invented_server_integer)
                .validate()
                .is_err()
        );

        let mut wrong_projection = base.clone();
        wrong_projection["stat_resonance"]["rational_integer_projection"] =
            serde_json::Value::String("per-hit-floor".into());
        assert!(runtime_from_value(wrong_projection).validate().is_err());

        let mut guessed_overlap = base;
        guessed_overlap["stat_resonance"]["unresolved_overlap_fails_closed"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(guessed_overlap).validate().is_err());
    }

    #[test]
    fn encore_55333_direct_output_requires_the_sealed_current_build_receipt() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(current.encore.effect_id, 55_333);
        assert_eq!(current.encore.damage_action_ids, [230_401, 230_501]);
        assert_eq!(current.encore.excluded_healing_action_id, 55_314);
        assert!(current.encore_runtime_transfer_enabled());
        assert!(current.effect_runtime_transfer_enabled(55_333));

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("the prior build remains replayable");
        assert!(!historical.encore_runtime_transfer_enabled());
        assert!(!historical.effect_runtime_transfer_enabled(55_333));

        for authority in [
            "current_build_lifecycle_authority",
            "current_build_provider_ownership_authority",
            "exact_packet_final_integer_authority",
            "same_provider_instances_coalesced",
            "external_provider_only",
            "ordinary_damage_unchanged",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["encore"][authority] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        for (field, value) in [
            ("proof_exact_build_rlogs", serde_json::json!(25)),
            ("proof_attributed_events", serde_json::json!(2_745)),
            ("proof_attributed_rdmg", serde_json::json!(55_685_345)),
        ] {
            let mut altered_receipt = base.clone();
            altered_receipt["encore"][field] = value;
            assert!(runtime_from_value(altered_receipt).validate().is_err());
        }

        let mut healing_promoted_as_damage = base.clone();
        healing_promoted_as_damage["encore"]["damage_action_ids"] =
            serde_json::json!([55_314, 230_401]);
        assert!(
            runtime_from_value(healing_promoted_as_damage)
                .validate()
                .is_err()
        );

        let mut locale_overclaimed = base;
        locale_overclaimed["encore"]["locale_evidence"] = serde_json::Value::String(
            "English Encore independently verified in current build 24687926".into(),
        );
        assert!(runtime_from_value(locale_overclaimed).validate().is_err());
    }

    #[test]
    fn encore_55333_current_build_replay_receipt_stays_conserved_and_external() {
        let receipt: serde_json::Value = serde_json::from_str(include_str!(
            "../protocol-packs/global/steam-24687926/observations/encore-55333-provider-recipient-replay-002.json"
        ))
        .expect("the build-locked Encore replay receipt must remain valid JSON");

        assert_eq!(receipt["deployment_id"], "global");
        assert_eq!(receipt["client_build"], "24687926");
        assert_eq!(receipt["effect_identity"]["effect_id"], 55_333);
        assert_eq!(
            receipt["effect_identity"]["damage_action_ids"],
            serde_json::json!([230_401, 230_501])
        );
        assert_eq!(
            receipt["effect_identity"]["excluded_healing_action_id"],
            55_314
        );
        assert_eq!(receipt["packet_lifecycle"]["unique_player_providers"], 1);
        assert_eq!(receipt["packet_lifecycle"]["unique_targets"], 4);
        assert_eq!(receipt["production_replay"]["unique_providers"], 1);
        assert_eq!(receipt["production_replay"]["unique_recipients"], 4);
        assert_eq!(receipt["production_replay"]["attributed_events"], 74);
        assert_eq!(receipt["production_replay"]["attributed_rdmg"], 1_525_694);
        assert_eq!(
            receipt["production_replay"]["full_report_raw_damage"],
            receipt["production_replay"]["full_report_rdps_damage"]
        );
        assert_eq!(
            receipt["production_replay"]["full_report_contribution_given"],
            receipt["production_replay"]["full_report_contribution_received"]
        );
        for required in [
            "runtime_target_match",
            "report_conserved",
            "all_damage_context_complete",
            "all_encore_contributions_external",
            "all_encore_denominators_one",
        ] {
            assert_eq!(receipt["production_replay"][required], true);
        }
        assert_eq!(
            receipt["authority_boundary"]["recipient_skill_ownership_for_encore_authorized"],
            false
        );
        assert_eq!(
            receipt["authority_boundary"]["unresolved_or_multiple_provider_state_fails_closed"],
            true
        );
    }

    #[test]
    fn inspire_31602_current_build_replay_receipt_stays_conserved_and_external() {
        let receipt: serde_json::Value = serde_json::from_str(include_str!(
            "../protocol-packs/global/steam-24687926/observations/inspire-31602-provider-recipient-replay-001.json"
        ))
        .expect("the build-locked Inspire replay receipt must remain valid JSON");

        assert_eq!(receipt["deployment_id"], "global");
        assert_eq!(receipt["client_build"], "24687926");
        assert_eq!(receipt["effect_identity"]["effect_id"], 31_602);
        assert_eq!(receipt["effect_identity"]["source_type_id"], 1);
        assert_eq!(receipt["effect_identity"]["source_config_id"], 31_601);
        assert_eq!(receipt["packet_lifecycle"]["selected_status_events"], 18);
        assert_eq!(receipt["packet_lifecycle"]["applied_events"], 9);
        assert_eq!(receipt["packet_lifecycle"]["removed_events"], 9);
        assert_eq!(receipt["packet_lifecycle"]["unique_player_providers"], 1);
        assert_eq!(receipt["production_replay"]["unique_providers"], 1);
        assert_eq!(receipt["production_replay"]["unique_recipients"], 2);
        assert_eq!(receipt["production_replay"]["attributed_events"], 85);
        assert_eq!(receipt["production_replay"]["attributed_rdmg"], 242_165);
        assert_eq!(
            receipt["production_replay"]["recipient_attributed_rdmg"],
            serde_json::json!([57_962, 184_203])
        );
        assert_eq!(
            receipt["production_replay"]["full_report_raw_damage"],
            receipt["production_replay"]["full_report_rdps_damage"]
        );
        assert_eq!(
            receipt["production_replay"]["full_report_contribution_given"],
            receipt["production_replay"]["full_report_contribution_received"]
        );
        for required in [
            "runtime_target_match",
            "report_conserved",
            "all_inspire_contributions_external",
            "all_damage_context_complete",
        ] {
            assert_eq!(receipt["production_replay"][required], true);
        }
        assert_eq!(
            receipt["authority_boundary"]["unresolved_lifecycle_route_or_speed_state_fails_closed"],
            true
        );
        assert_eq!(
            receipt["authority_boundary"]["server_integer_counterfactual_authority"],
            false
        );
    }

    #[test]
    fn current_build_observation_index_is_complete_and_respects_runtime_boundaries() {
        let runtime = runtime_from_value(bundled_runtime_value());
        assert!(runtime.validate().is_ok());
        let external_state: ExternalStateRdpsInventory = serde_json::from_str(include_str!(
            "../game-data/runtime/external-state-rdps.v1.json"
        ))
        .expect("the external-state runtime inventory must remain valid JSON");

        let observation_directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("protocol-packs/global/steam-24687926/observations");
        let index_path = observation_directory.join("index.json");
        let index: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&index_path).expect("the current-build observation index must exist"),
        )
        .expect("the current-build observation index must remain valid JSON");

        assert_eq!(index["schema_version"], 1);
        assert_eq!(index["deployment_id"], "global");
        assert_eq!(index["client_build"], "24687926");
        for policy in [
            "every_json_receipt_is_indexed_exactly_once",
            "index_is_audit_registry_not_runtime_authority",
            "runtime_authority_still_requires_pack_validation",
            "negative_gates_must_remain_explicitly_false",
        ] {
            assert_eq!(index["policy"][policy], true);
        }

        let receipts = index["receipts"]
            .as_array()
            .expect("the observation index must contain receipt entries");
        let mut indexed_files = std::collections::BTreeSet::new();
        let mut observation_ids = std::collections::BTreeSet::new();

        for entry in receipts {
            let file = entry["file"]
                .as_str()
                .expect("every receipt entry must identify one file");
            assert_eq!(
                std::path::Path::new(file)
                    .file_name()
                    .and_then(|name| name.to_str()),
                Some(file),
                "receipt index paths must remain local leaf filenames"
            );
            assert!(
                indexed_files.insert(file.to_owned()),
                "duplicate receipt file {file}"
            );

            let document: serde_json::Value = serde_json::from_slice(
                &std::fs::read(observation_directory.join(file)).unwrap_or_else(|error| {
                    panic!("failed to read indexed receipt {file}: {error}")
                }),
            )
            .unwrap_or_else(|error| panic!("indexed receipt {file} is invalid JSON: {error}"));
            let observation_id = entry["observation_id"]
                .as_str()
                .expect("every receipt entry must identify its observation");
            assert!(
                observation_ids.insert(observation_id.to_owned()),
                "duplicate observation ID {observation_id}"
            );
            assert_eq!(document["observation_id"], observation_id);
            let build = document["client_build"]
                .as_str()
                .or_else(|| document["game_build"].as_str())
                .or_else(|| document["runtime_rule_build"].as_str());
            assert_eq!(build, Some("24687926"), "wrong build in receipt {file}");
            if let Some(deployment_id) = document["deployment_id"]
                .as_str()
                .or_else(|| document["runtime_rule_deployment"].as_str())
            {
                assert_eq!(
                    deployment_id, "global",
                    "wrong deployment in receipt {file}"
                );
            }

            let effect_ids = entry["effect_ids"]
                .as_array()
                .expect("every receipt entry must list exact effect IDs");
            assert!(!effect_ids.is_empty());
            let disposition = entry["disposition"]
                .as_str()
                .expect("every receipt entry must state its authority disposition");
            match disposition {
                "runtime-authority" => {
                    assert!(entry.get("negative_gate_pointer").is_none());
                    for effect_id in effect_ids {
                        let effect_id = effect_id
                            .as_i64()
                            .expect("indexed effect IDs must be positive integers");
                        assert!(effect_id > 0);
                        assert!(
                            runtime.effect_runtime_transfer_enabled(effect_id),
                            "receipt {file} claims runtime authority for disabled effect {effect_id}"
                        );
                    }
                }
                "external-state-runtime-authority" => {
                    assert!(entry.get("negative_gate_pointer").is_none());
                    for effect_id in effect_ids {
                        let effect_id = effect_id
                            .as_i64()
                            .expect("indexed effect IDs must be positive integers");
                        assert!(effect_id > 0);
                        assert!(
                            external_state
                                .rules
                                .iter()
                                .any(|rule| rule.effect_id == effect_id),
                            "receipt {file} claims external-state authority for an absent effect {effect_id}"
                        );
                    }
                }
                "conservation-authority" => {
                    assert!(entry.get("negative_gate_pointer").is_none());
                    assert_eq!(document["generated_by"], "rlogs-bpsr-rdps-replay-audit");
                    assert_eq!(document["attribution_mode"], "production_promoted_rules");
                    assert_eq!(document["all_runtime_targets_match"], true);
                    assert_eq!(document["all_reports_conserved"], true);
                    assert_eq!(
                        document["policy"]["canonical_integrity_seal_required"],
                        true
                    );
                    assert_eq!(document["policy"]["exact_runtime_identity_required"], true);
                    assert_eq!(document["policy"]["production_promoted_rules_only"], true);
                    assert_eq!(
                        document["policy"]["exact_party_conservation_required"],
                        true
                    );
                    assert_eq!(document["policy"]["raw_packet_payloads_included"], false);
                    assert_eq!(document["policy"]["source_paths_included"], false);
                    assert_eq!(document["policy"]["runtime_authority_changed"], false);
                    assert!(document["reports"].as_array().is_some_and(|reports| {
                        reports.len() >= 2
                            && reports.iter().all(|report| {
                                report["runtime_target_match"] == true
                                    && report["conserved"] == true
                                    && report["contribution_given"]
                                        == report["contribution_received"]
                            })
                    }));
                    assert!(
                        document["total_attributed_damage_events"]
                            .as_u64()
                            .is_some_and(|count| count > 0)
                    );
                    assert_eq!(
                        document["rule_effect_ids"]
                            .as_array()
                            .expect("conservation receipt rule IDs must be an array"),
                        effect_ids
                    );
                    for effect_id in effect_ids {
                        let effect_id = effect_id
                            .as_i64()
                            .expect("indexed effect IDs must be positive integers");
                        assert!(
                            runtime.effect_runtime_transfer_enabled(effect_id)
                                || external_state
                                    .rules
                                    .iter()
                                    .any(|rule| rule.effect_id == effect_id),
                            "conservation receipt includes non-production effect {effect_id}"
                        );
                    }
                    let expected_digest = document["content_sha256"]
                        .as_str()
                        .expect("conservation receipts must be self-hashed");
                    let mut digest_input = document.clone();
                    digest_input
                        .as_object_mut()
                        .expect("conservation receipt must be an object")
                        .remove("content_sha256");
                    let actual_digest = format!(
                        "sha256:{:x}",
                        Sha256::digest(
                            serde_json::to_vec(&digest_input)
                                .expect("conservation receipt must serialize")
                        )
                    );
                    assert_eq!(actual_digest, expected_digest);
                }
                "negative-gate" | "ownership-only-nontransfer" => {
                    let pointer = entry["negative_gate_pointer"]
                        .as_str()
                        .expect("non-authoritative receipts must bind an explicit false gate");
                    assert_eq!(
                        document.pointer(pointer),
                        Some(&serde_json::Value::Bool(false)),
                        "receipt {file} no longer preserves its negative authority gate"
                    );
                    if disposition == "ownership-only-nontransfer" {
                        for effect_id in effect_ids {
                            let effect_id = effect_id
                                .as_i64()
                                .expect("indexed effect IDs must be positive integers");
                            assert!(effect_id > 0);
                            assert!(!runtime.effect_runtime_transfer_enabled(effect_id));
                        }
                    }
                }
                other => panic!("unknown receipt disposition {other}"),
            }
        }

        let observed_files = std::fs::read_dir(&observation_directory)
            .expect("the current-build observation directory must remain readable")
            .map(|entry| {
                entry
                    .expect("observation directory entry must be readable")
                    .path()
            })
            .filter(|path| {
                path.extension().and_then(|extension| extension.to_str()) == Some("json")
                    && path.file_name().and_then(|name| name.to_str()) != Some("index.json")
            })
            .map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .expect("observation filenames must be UTF-8")
                    .to_owned()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(indexed_files, observed_files);
    }

    #[test]
    fn fiery_battle_will_requires_exact_local_observed_attack_authority() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(current.fiery_battle_will.runtime_transfer_enabled);
        assert_eq!(current.fiery_battle_will.effect_id, 2_110_065);
        assert_eq!(current.fiery_battle_will.source_config_id, 2_110_064);
        assert_eq!(current.fiery_battle_will.provider_raw_percent_delta, 1_000);
        assert!(current.fiery_battle_will.local_recipient_only);
        assert!(current.effect_runtime_transfer_enabled(2_110_065));

        let historical = rdps_runtime_config_for("global", "24252055")
            .unwrap()
            .expect("the prior build remains replayable");
        assert!(!historical.fiery_battle_will.runtime_transfer_enabled);
        assert!(!historical.effect_runtime_transfer_enabled(2_110_065));

        for authority in [
            "current_build_external_lifecycle_authority",
            "current_build_provider_ownership_authority",
            "exact_mirrored_attack_raw_percent_transition_authority",
            "local_recipient_only",
        ] {
            let mut missing_authority = base.clone();
            missing_authority["fiery_battle_will"][authority] = serde_json::Value::Bool(false);
            assert!(runtime_from_value(missing_authority).validate().is_err());
        }

        let mut wrong_delta = base.clone();
        wrong_delta["fiery_battle_will"]["provider_raw_percent_delta"] = serde_json::json!(999);
        assert!(runtime_from_value(wrong_delta).validate().is_err());

        let mut invented_server_integer = base.clone();
        invented_server_integer["fiery_battle_will"]["server_integer_counterfactual_authority"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(invented_server_integer)
                .validate()
                .is_err()
        );

        let mut guessed_overlap = base;
        guessed_overlap["fiery_battle_will"]["unresolved_overlap_fails_closed"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(guessed_overlap).validate().is_err());
    }

    #[test]
    fn harmony_grace_promotes_the_exact_class_11_packet_final_route() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(!current.runtime_promotion_allowed());
        assert!(current.harmony_grace.runtime_transfer_enabled);
        assert_eq!(current.harmony_grace.runtime_recipient_class_ids, [11]);
        assert!(
            current
                .harmony_grace
                .remote_paired_output_runtime_transfer_enabled
        );
        let class_11 = current
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == 11)
            .unwrap();
        assert_eq!(class_11.primary_percent_raw_delta, 200);
        assert_eq!(class_11.primary_to_attack_numerator, 58);
        assert_eq!(class_11.primary_to_attack_denominator, 100);
        assert_eq!(
            current.harmony_grace.accounting_method,
            "observed-final-damage-proportional-stage-share"
        );
        assert!(
            current
                .harmony_grace
                .class_11_current_pack_lifecycle_authority
        );
        assert!(
            current
                .harmony_grace
                .class_11_exact_rational_attribution_authority
        );
        assert!(
            !current
                .harmony_grace
                .server_integer_counterfactual_authority
        );
        assert!(current.harmony_grace.unresolved_overlap_fails_closed);
        assert!(current.effect_runtime_transfer_enabled(current.harmony_grace.effect_id));
        assert!(current.harmony_grace.direct_replay.is_current_authority());

        let mut altered_direct_replay = base.clone();
        altered_direct_replay["harmony_grace"]["direct_replay"]["direct_increment_rdmg"] =
            serde_json::json!(6966);
        assert!(
            runtime_from_value(altered_direct_replay)
                .validate()
                .is_err()
        );

        let mut missing_rational_authority = base.clone();
        missing_rational_authority["harmony_grace"]["class_11_exact_rational_attribution_authority"] =
            serde_json::Value::Bool(false);
        missing_rational_authority["harmony_grace"]["runtime_transfer_enabled"] =
            serde_json::Value::Bool(true);
        missing_rational_authority["harmony_grace"]["runtime_recipient_class_ids"] =
            serde_json::json!([11]);
        assert!(
            runtime_from_value(missing_rational_authority)
                .validate()
                .is_err(),
            "runtime transfer still requires exact-rational authority",
        );

        let mut exact_rational_reenable = base.clone();
        exact_rational_reenable["harmony_grace"]["class_11_exact_rational_attribution_authority"] =
            serde_json::Value::Bool(true);
        exact_rational_reenable["harmony_grace"]["runtime_transfer_enabled"] =
            serde_json::Value::Bool(true);
        exact_rational_reenable["harmony_grace"]["runtime_recipient_class_ids"] =
            serde_json::json!([11]);
        assert!(
            runtime_from_value(exact_rational_reenable)
                .validate()
                .is_ok(),
            "exact-rational packet-final attribution must not require hidden server integer authority",
        );

        let mut hidden_server_integer_stays_unclaimed = base.clone();
        hidden_server_integer_stays_unclaimed["harmony_grace"]["server_integer_counterfactual_authority"] =
            serde_json::Value::Bool(false);
        assert!(
            runtime_from_value(hidden_server_integer_stays_unclaimed)
                .validate()
                .is_ok()
        );

        let mut wrong_accounting_method = base.clone();
        wrong_accounting_method["harmony_grace"]["accounting_method"] =
            serde_json::Value::String("server-counterfactual-guess".into());
        assert!(
            runtime_from_value(wrong_accounting_method)
                .validate()
                .is_err()
        );

        let mut guessed_overlap = base.clone();
        guessed_overlap["harmony_grace"]["unresolved_overlap_fails_closed"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(guessed_overlap).validate().is_err());

        let mut wrong_projection = base.clone();
        wrong_projection["harmony_grace"]["rational_integer_projection"] =
            serde_json::Value::String("per-hit-floor".into());
        assert!(runtime_from_value(wrong_projection).validate().is_err());

        let mut disabled_with_class = base;
        disabled_with_class["harmony_grace"]["runtime_transfer_enabled"] =
            serde_json::Value::Bool(false);
        disabled_with_class["harmony_grace"]["runtime_recipient_class_ids"] =
            serde_json::json!([11]);
        assert!(runtime_from_value(disabled_with_class).validate().is_err());

        let mut disabled_remote_transfer = bundled_runtime_value();
        disabled_remote_transfer["harmony_grace"]["remote_paired_output_runtime_transfer_enabled"] =
            serde_json::Value::Bool(false);
        assert!(
            runtime_from_value(disabled_remote_transfer)
                .validate()
                .is_ok()
        );

        let mut altered_remote_context = bundled_runtime_value();
        altered_remote_context["harmony_grace"]["remote_paired_output_ignored_effect_ids"] =
            serde_json::json!([27016, 55301]);
        assert!(
            runtime_from_value(altered_remote_context)
                .validate()
                .is_err()
        );

        let mut altered_remote_formula_set = bundled_runtime_value();
        altered_remote_formula_set["harmony_grace"]["remote_paired_output_formula_effect_ids"][0] =
            serde_json::json!(1);
        assert!(
            runtime_from_value(altered_remote_formula_set)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn harmony_counterfactual_model_is_shared_by_candidate_and_production_replay() {
        assert_eq!(
            promoted_remote_effect_magnitude_model(3_003_052).unwrap(),
            Some(PromotedRemoteEffectMagnitudeModel::CounterfactualReplay)
        );
        let harmony = &rdps_runtime_config().unwrap().harmony_grace;
        assert!(harmony.runtime_transfer_enabled);
        assert_eq!(harmony.runtime_recipient_class_ids, [11]);
    }

    #[test]
    fn inspiration_recipient_dependencies_require_independent_runtime_authority() {
        let current = rdps_runtime_config().unwrap();
        assert!(
            current
                .inspiration
                .recipient_dependency_runtime_transfer_enabled
        );
        assert_eq!(
            current
                .inspiration
                .recipient_dependency
                .critical_damage_raw_delta(150),
            Some(75)
        );
        assert_eq!(
            current
                .inspiration
                .recipient_dependency
                .critical_damage_raw_delta(300),
            Some(150)
        );

        let mut unproven = bundled_runtime_value();
        unproven["inspiration"]["recipient_dependency"]["exact_build_formula_authority"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(unproven).validate().is_err());
    }
}
