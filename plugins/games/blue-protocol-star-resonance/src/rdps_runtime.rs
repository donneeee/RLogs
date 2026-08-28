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

use crate::state_formula::CriticalDamageFactorInterpretation;

const RDPS_RUNTIME_SCHEMA_VERSION: u16 = 13;

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TeamLuckRuntimeConfig {
    pub effect_id: i64,
    pub critical_damage_attribute_id: i32,
    pub lucky_damage_attribute_id: i32,
    pub critical_raw_delta: i64,
    pub lucky_raw_delta: i64,
    pub combined_critical_lucky_enabled: bool,
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
pub(crate) struct InspirationRuntimeConfig {
    pub effect_id: i64,
    pub source_config_id: i64,
    pub full_bloom_effect_id: i64,
    pub full_bloom_source_config_id: i64,
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
    pub provider_marker_effect_id: i64,
    pub companion_lockout_effect_id: i64,
    pub all_element_family: FixedPointFamilyRuntimeConfig,
    pub duration_millis: u64,
    pub lockout_duration_millis: u64,
    pub packet_proven_raw_deltas: Vec<i64>,
    pub excluded_provider_owned_damage_ids: Vec<i64>,
    pub requires_recipient_packet_transition: bool,
    pub runtime_transfer_enabled: bool,
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
    pub thunderwind: ThunderwindRuntimeConfig,
    pub inspiration: InspirationRuntimeConfig,
    pub highland_blood: HighlandBloodRuntimeConfig,
}

impl RdpsRuntimeConfig {
    pub(crate) fn runtime_promotion_allowed(&self) -> bool {
        self.policy.runtime_promotion_allowed
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
            || self.mechanical_power.runtime_transfer_enabled
            || self.harmony_grace.runtime_transfer_enabled
    }

    /// Effect-scoped production authority. A false result never hides the
    /// canonical event or ordinary damage; it only blocks provider transfer.
    pub(crate) fn effect_runtime_transfer_enabled(&self, effect_id: i64) -> bool {
        self.runtime_promotion_allowed()
            || self
                .target_vulnerability
                .runtime_transfer_effect_ids
                .contains(&effect_id)
            || (effect_id == self.functional_amp.effect_id
                && self.functional_amp.attack_magic_runtime_transfer_enabled)
            || (effect_id == self.mechanical_power.effect_id
                && self.mechanical_power.runtime_transfer_enabled)
            || (effect_id == self.harmony_grace.effect_id
                && self.harmony_grace.runtime_transfer_enabled)
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
        if team_luck.effect_id <= 0
            || team_luck.critical_damage_attribute_id <= 0
            || team_luck.lucky_damage_attribute_id <= 0
            || team_luck.critical_damage_attribute_id == team_luck.lucky_damage_attribute_id
            || team_luck.critical_raw_delta <= 0
            || team_luck.lucky_raw_delta <= 0
            || team_luck.combined_critical_lucky_enabled
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
            && mechanical.damage_stage_operation_order_authority
            && mechanical.damage_stage_integer_rounding_authority
            && mechanical.server_integer_counterfactual_authority
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
        if harmony.effect_id <= 0
            || harmony.source_terminal_effect_id <= 0
            || harmony.effect_id == harmony.source_terminal_effect_id
            || !harmony.has_valid_source_origin_rule()
            || !rules_are_valid_and_unique(&harmony.recipient_rules)
            || harmony.rational_integer_projection
                != "sum-exact-then-half-up-per-effect-provider-recipient"
            || !harmony.unresolved_overlap_fails_closed
            || !harmony_runtime_classes_are_valid
            || (harmony.runtime_transfer_enabled
                && (!harmony_runtime_authority || harmony.runtime_recipient_class_ids != [11]))
            || (!harmony.runtime_transfer_enabled
                && !harmony.runtime_recipient_class_ids.is_empty())
        {
            return Err(
                "bundled BPSR Harmony Grace formula is not ready for runtime transfer".into(),
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
            || inspiration.full_bloom_source_config_id <= 0
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
            || inspiration.runtime_transfer_enabled
        {
            return Err(
                "bundled BPSR Inspiration formula is not ready for runtime transfer".into(),
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
            || highland.runtime_transfer_enabled
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
    patch: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsRuntimeBuildOverrides {
    schema_version: u16,
    deployment_id: String,
    builds: Vec<RdpsRuntimeBuildOverride>,
}

#[derive(Debug)]
struct RdpsRuntimeRegistry {
    deployment_id: String,
    latest_build: String,
    by_build: HashMap<String, RdpsRuntimeConfig>,
}

static RDPS_RUNTIME_REGISTRY: OnceLock<Result<RdpsRuntimeRegistry, String>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromotedRemoteEffectMagnitudeModel {
    CounterfactualReplay,
}

pub(crate) fn rdps_runtime_config() -> Result<&'static RdpsRuntimeConfig, String> {
    let registry = rdps_runtime_registry()?;
    registry
        .by_build
        .get(&registry.latest_build)
        .ok_or_else(|| "bundled BPSR rDPS registry has no latest formula pack".into())
}

pub(crate) fn rdps_runtime_config_for(
    deployment_id: &str,
    game_build: &str,
) -> Result<Option<&'static RdpsRuntimeConfig>, String> {
    let registry = rdps_runtime_registry()?;
    if deployment_id != registry.deployment_id {
        return Ok(None);
    }
    if let Some(runtime) = registry.by_build.get(game_build) {
        return Ok(Some(runtime));
    }
    Ok(None)
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
            if overrides.schema_version != RDPS_RUNTIME_SCHEMA_VERSION
                || overrides.deployment_id != base.deployment_id
            {
                return Err(
                    "bundled BPSR rDPS formula overrides have an unsupported identity".into(),
                );
            }

            let deployment_id = base.deployment_id.clone();
            let mut latest_build = base.game_build.clone();
            let mut by_build = HashMap::from([(base.game_build.clone(), base)]);
            for build_override in overrides.builds {
                if build_override.game_build.is_empty()
                    || by_build.contains_key(&build_override.game_build)
                {
                    return Err(
                        "bundled BPSR rDPS formula overrides contain a duplicate or empty build"
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
                    || !is_prefixed_sha256(&config.protocol_pack_digest)
                {
                    return Err(
                        "bundled BPSR rDPS formula override changed its declared identity".into(),
                    );
                }
                config.validate()?;
                if numeric_build(&config.game_build) > numeric_build(&latest_build) {
                    latest_build = config.game_build.clone();
                }
                by_build.insert(config.game_build.clone(), config);
            }

            Ok(RdpsRuntimeRegistry {
                deployment_id,
                latest_build,
                by_build,
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
            CriticalDamageFactorInterpretation::Unresolved,
        );
        assert!(current.validate().is_ok());

        let mut candidate_without_authority = base.clone();
        candidate_without_authority["critical_damage_factor_interpretation"] =
            serde_json::Value::String("additive_bonus".into());
        assert!(
            runtime_from_value(candidate_without_authority)
                .validate()
                .is_err(),
            "a candidate name alone must not become runtime authority",
        );

        let mut authority_without_interpretation = base.clone();
        authority_without_interpretation["policy"]["critical_damage_factor_interpretation_authority"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(authority_without_interpretation)
                .validate()
                .is_err(),
            "authority without a resolved exact-build interpretation must fail",
        );

        let mut resolved_but_still_globally_blocked = base.clone();
        resolved_but_still_globally_blocked["policy"]["critical_damage_factor_interpretation_authority"] =
            serde_json::Value::Bool(true);
        resolved_but_still_globally_blocked["critical_damage_factor_interpretation"] =
            serde_json::Value::String("direct_total".into());
        resolved_but_still_globally_blocked["promotion_blockers"]
            .as_array_mut()
            .expect("promotion blockers should be an array")
            .retain(|blocker| {
                blocker.as_str() != Some("critical-damage-factor-interpretation-authority")
            });
        assert!(
            runtime_from_value(resolved_but_still_globally_blocked)
                .validate()
                .is_ok()
        );

        let mut promotion_without_interpretation_authority = base;
        promotion_without_interpretation_authority["promotion_state"] =
            serde_json::Value::String("approved".into());
        promotion_without_interpretation_authority["policy"]["runtime_promotion_allowed"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(promotion_without_interpretation_authority)
                .validate()
                .is_err(),
            "global promotion cannot bypass unresolved critical arithmetic",
        );
    }

    #[test]
    fn promotion_blockers_are_known_unique_and_match_runtime_authority() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert_eq!(
            current.promotion_blockers(),
            [
                "canonical-replay-conservation",
                "critical-damage-factor-interpretation-authority",
                "party-support-formula-frontier",
            ]
        );

        let mut duplicate = base.clone();
        duplicate["promotion_blockers"]
            .as_array_mut()
            .expect("promotion blockers should be an array")
            .push(serde_json::Value::String(
                "canonical-replay-conservation".into(),
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
    fn mechanical_power_requires_operation_order_and_integer_proof_for_runtime() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(!current.mechanical_power.runtime_transfer_enabled);
        assert!(
            current
                .mechanical_power
                .class_11_tier_0_current_pack_lifecycle_authority
        );
        assert!(
            !current
                .mechanical_power
                .class_11_tier_0_exact_rational_attribution_authority
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
        assert!(
            current
                .mechanical_power
                .runtime_recipient_class_ids
                .is_empty()
        );
        assert!(
            current
                .mechanical_power
                .runtime_primary_percent_raw_deltas
                .is_empty()
        );
        assert_eq!(
            current
                .mechanical_power
                .production_primary_percent_raw_delta(11),
            None
        );
        assert_eq!(
            current
                .mechanical_power
                .production_primary_percent_raw_delta(9),
            None
        );
        assert!(!current.effect_runtime_transfer_enabled(current.mechanical_power.effect_id));

        let mut premature_transfer = base.clone();
        premature_transfer["mechanical_power"]["runtime_transfer_enabled"] =
            serde_json::Value::Bool(true);
        premature_transfer["mechanical_power"]["runtime_recipient_class_ids"] =
            serde_json::json!([11]);
        premature_transfer["mechanical_power"]["runtime_primary_percent_raw_deltas"] =
            serde_json::json!([750]);
        assert!(
            runtime_from_value(premature_transfer.clone())
                .validate()
                .is_err(),
            "packet lifecycle and a candidate marginal do not prove the damage operator",
        );

        let mut complete_proof_contract = premature_transfer;
        complete_proof_contract["mechanical_power"]["class_11_tier_0_exact_rational_attribution_authority"] =
            serde_json::Value::Bool(true);
        complete_proof_contract["mechanical_power"]["damage_stage_operation_order_authority"] =
            serde_json::Value::Bool(true);
        complete_proof_contract["mechanical_power"]["damage_stage_integer_rounding_authority"] =
            serde_json::Value::Bool(true);
        complete_proof_contract["mechanical_power"]["server_integer_counterfactual_authority"] =
            serde_json::Value::Bool(true);
        assert!(
            runtime_from_value(complete_proof_contract)
                .validate()
                .is_ok()
        );

        let mut wrong_projection = base.clone();
        wrong_projection["mechanical_power"]["rational_integer_projection"] =
            serde_json::Value::String("per-hit-floor".into());
        assert!(runtime_from_value(wrong_projection).validate().is_err());

        let mut guessed_overlap = base.clone();
        guessed_overlap["mechanical_power"]["unresolved_overlap_fails_closed"] =
            serde_json::Value::Bool(false);
        assert!(runtime_from_value(guessed_overlap).validate().is_err());

        let mut disabled_with_scope = base;
        disabled_with_scope["mechanical_power"]["runtime_recipient_class_ids"] =
            serde_json::json!([11]);
        assert!(runtime_from_value(disabled_with_scope).validate().is_err());
    }

    #[test]
    fn harmony_grace_uses_the_corrected_proportional_accounting_contract() {
        let base = bundled_runtime_value();
        let current = runtime_from_value(base.clone());
        assert!(current.validate().is_ok());
        assert!(!current.runtime_promotion_allowed());
        assert!(current.harmony_grace.runtime_transfer_enabled);
        assert_eq!(current.harmony_grace.runtime_recipient_class_ids, [11]);
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

        let mut missing_rational_authority = base.clone();
        missing_rational_authority["harmony_grace"]["class_11_exact_rational_attribution_authority"] =
            serde_json::Value::Bool(false);
        assert!(
            runtime_from_value(missing_rational_authority)
                .validate()
                .is_err()
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
    }

    #[test]
    fn harmony_proportional_model_is_available_for_runtime_transfer() {
        assert_eq!(
            promoted_remote_effect_magnitude_model(3_003_052).unwrap(),
            Some(PromotedRemoteEffectMagnitudeModel::CounterfactualReplay)
        );
        assert!(
            rdps_runtime_config()
                .unwrap()
                .harmony_grace
                .runtime_transfer_enabled
        );
    }
}
