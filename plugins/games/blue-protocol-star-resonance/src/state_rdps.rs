//! Packet-proven BPSR state-scaling rDPS projection.
//!
//! This module never filters damage, healing, shields, statuses, or attribute
//! events. It emits an additional exact marginal transfer only when the same
//! capture proves the external provider, recipient, attribute transition,
//! calculation snapshot, and damage formula.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::OnceLock,
};

use rlogs_combat::{
    ActorAncestryResolver, ActorOwnershipEvidence, ExactDamageContributionEvent,
    ExactDamageContributionProjector, ExactRationalDamageContributionEvent,
};
use rlogs_events::{
    ActorKind, ActorState, CanonicalEvent, EncounterState, EntityAttribute,
    EntityAttributeUpdateKind, EntityAttributeValue, EntityRef, EventEnvelope, EvidenceSource,
    RunState, StatusState, TimelineEventKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    BPSR_FIXED_POINT_SCALE, CriticalDamageFactorInterpretation, PacketDamageScriptFamily,
    PositiveFixedPointRounding,
    damage_stage::{
        OffensiveStatKind, SelectedDamageStage, select_damage_stage, validate_damage_stage_catalog,
    },
    decode_known_entity_attribute_value,
    decoder::{ATTR_CURRENT_HP, ATTR_MAX_HP_EXTRA_ADD},
    exact_additive_fixed_point_marginal_from_observed_output,
    exact_external_attack_and_factors_fraction, exact_external_attack_coefficient_stage_fraction,
    exact_external_attack_ordered_stage_fraction,
    exact_external_combined_critical_lucky_chance_fraction,
    exact_external_critical_chance_and_damage_fraction, exact_external_critical_chance_fraction,
    exact_external_critical_damage_fraction, exact_external_lucky_chance_fraction,
    exact_external_lucky_damage_fraction, linear_state_scaled_damage_marginal,
    packet_attribute_family_provider_marginal, packet_attribute_family_value,
    rdps_runtime::{
        AttackFamilyRuntimeConfig, AttributeFamilyRounding, InspirationVectorRuntimeConfig,
        PrimaryAttackLane, PrimaryStatRecipientRule, RdpsRuntimeConfig,
        ThunderwindVectorRuntimeConfig, rdps_runtime_config, rdps_runtime_config_for,
        rdps_runtime_config_for_identity,
    },
    specialization_identity_from_observed_abilities, two_stage_percent_input_marginal,
};

const STATE_RDPS_SCHEMA_VERSION: u16 = 1;
const TARGET_VULNERABILITY_RDPS_SCHEMA_VERSION: u16 = 4;
/// Bump whenever the projector's operation order, window semantics, stacking,
/// or integer/rational calculation changes independently of the bundled data.
const STATE_RDPS_PROJECTOR_ALGORITHM_REVISION: &str = "bpsr-state-rdps-projector.v10";

static STATE_RDPS_FORMULA_IDENTITY: OnceLock<String> = OnceLock::new();

/// Content identity for every production input consumed by the state rDPS
/// projector plus its calculation algorithm. Localized presentation catalogs
/// are deliberately excluded because numeric effect and build identity are the
/// runtime authority.
pub fn state_damage_contribution_formula_identity() -> &'static str {
    STATE_RDPS_FORMULA_IDENTITY
        .get_or_init(|| {
            let mut hasher = Sha256::new();
            for (label, content) in [
                (
                    "algorithm_revision",
                    STATE_RDPS_PROJECTOR_ALGORITHM_REVISION.as_bytes(),
                ),
                ("crate_version", env!("CARGO_PKG_VERSION").as_bytes()),
                (
                    "rdps-formula-runtime.v1.json",
                    include_bytes!("../game-data/runtime/rdps-formula-runtime.v1.json").as_slice(),
                ),
                (
                    "rdps-formula-runtime-overrides.v1.json",
                    include_bytes!("../game-data/runtime/rdps-formula-runtime-overrides.v1.json")
                        .as_slice(),
                ),
                (
                    "external-state-rdps.v1.json",
                    include_bytes!("../game-data/runtime/external-state-rdps.v1.json").as_slice(),
                ),
                (
                    "external-target-vulnerability-rdps.v2.json",
                    include_bytes!(
                        "../game-data/runtime/external-target-vulnerability-rdps.v2.json"
                    )
                    .as_slice(),
                ),
                (
                    "damage-stage-rdps.v1.json",
                    include_bytes!("../game-data/runtime/damage-stage-rdps.v1.json").as_slice(),
                ),
            ] {
                hasher.update((label.len() as u64).to_le_bytes());
                hasher.update(label.as_bytes());
                hasher.update((content.len() as u64).to_le_bytes());
                hasher.update(content);
            }
            format!("sha256:{:x}", hasher.finalize())
        })
        .as_str()
}

/// Stable evidence contract for calculating another player's rDPS.
///
/// Remote character snapshots are optional presentation and explanation
/// evidence. They are deliberately not calculation inputs: another player's
/// exact equipment, factor tree, levels, and other private loadout details are
/// not guaranteed to be present in this client's packet stream. The live
/// projector instead requires the server-observed provider/recipient window,
/// the applied runtime state, and an exact damage counterfactual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteRdpsEvidencePolicy {
    pub build_snapshot_required: bool,
    pub character_level_required: bool,
    pub exact_equipment_required: bool,
    pub exact_factor_tree_required: bool,
    pub provider_recipient_window_required: bool,
    pub applied_runtime_magnitude_required: bool,
    pub exact_counterfactual_formula_required: bool,
    pub retain_damage_when_unresolved: bool,
}

/// The authoritative remote-player evidence policy shared by live, history,
/// submission, and audit consumers of the BPSR rDPS projector.
pub const fn remote_rdps_evidence_policy() -> RemoteRdpsEvidencePolicy {
    RemoteRdpsEvidencePolicy {
        build_snapshot_required: false,
        character_level_required: false,
        exact_equipment_required: false,
        exact_factor_tree_required: false,
        provider_recipient_window_required: true,
        applied_runtime_magnitude_required: true,
        exact_counterfactual_formula_required: true,
        retain_damage_when_unresolved: true,
    }
}

/// Effect IDs currently enabled by the packet-state projector. This list is
/// build-scoped and contains only mechanics whose provider, recipient, packet
/// state, and exact integer or explicitly versioned exact-rational observed-
/// damage attribution are proven.
pub fn proven_state_damage_contribution_effect_ids() -> Result<Vec<i64>, String> {
    let runtime = rdps_runtime_config()?;
    let mut effect_ids = Vec::new();
    if runtime.runtime_promotion_allowed() {
        effect_ids.extend(
            state_rdps_catalog()?
                .rules
                .iter()
                .map(|rule| rule.effect_id),
        );
        effect_ids.push(runtime.team_luck.effect_id);
        effect_ids.push(runtime.thunderwind.effect_id);
        effect_ids.extend(
            target_vulnerability_rdps_catalog()?
                .rules
                .iter()
                .map(|rule| rule.effect_id),
        );
    }
    effect_ids.extend_from_slice(runtime.target_vulnerability_runtime_transfer_effect_ids());
    if runtime.effect_runtime_transfer_enabled(runtime.team_luck.effect_id) {
        effect_ids.push(runtime.team_luck.effect_id);
    }
    if runtime.effect_runtime_transfer_enabled(runtime.functional_amp.effect_id) {
        effect_ids.push(runtime.functional_amp.effect_id);
    }
    if runtime.effect_runtime_transfer_enabled(runtime.mechanical_power.effect_id) {
        effect_ids.push(runtime.mechanical_power.effect_id);
    }
    if runtime.effect_runtime_transfer_enabled(runtime.harmony_grace.effect_id) {
        effect_ids.push(runtime.harmony_grace.effect_id);
    }
    effect_ids.sort_unstable();
    effect_ids.dedup();
    Ok(effect_ids)
}

/// Exact effect IDs available only to the offline target-vulnerability
/// candidate audit. These IDs are intentionally separate from
/// [`proven_state_damage_contribution_effect_ids`]: listing a reviewed
/// candidate here does not grant production formula, runtime, UI, or provider
/// credit authority.
pub fn target_vulnerability_candidate_effect_ids() -> Result<Vec<i64>, String> {
    let mut effect_ids = target_vulnerability_rdps_catalog()?
        .rules
        .iter()
        .map(|rule| rule.effect_id)
        .collect::<Vec<_>>();
    effect_ids.sort_unstable();
    effect_ids.dedup();
    Ok(effect_ids)
}

/// Deployment identity owned by the active, versioned packet-state rDPS
/// runtime. Consumers must use this target instead of the older review
/// catalog identity exposed by `rdps`.
pub fn state_damage_contribution_deployment_id() -> Result<&'static str, String> {
    Ok(rdps_runtime_config()?.deployment_id.as_str())
}

/// Client build owned by the active, versioned packet-state rDPS runtime.
/// This is the canonical target shared by live, history, submission, and
/// replay-audit consumers.
pub fn state_damage_contribution_game_build() -> Result<&'static str, String> {
    Ok(rdps_runtime_config()?.game_build.as_str())
}

/// Protocol-pack digest owned by the active packet-state rDPS runtime. Build
/// identity without this digest is never sufficient for live or history use.
pub fn state_damage_contribution_protocol_pack_digest() -> Result<&'static str, String> {
    Ok(rdps_runtime_config()?.protocol_pack_digest.as_str())
}

/// Whether packet evidence belongs to the exact runtime for which the active
/// state projector has formula authority.
pub fn state_damage_contribution_target_matches(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
) -> Result<bool, String> {
    Ok(
        rdps_runtime_config_for_identity(deployment_id, client_build, protocol_pack_digest)?
            .is_some_and(RdpsRuntimeConfig::has_any_runtime_transfer_enabled),
    )
}

/// Whether an exact deployment/build formula configuration exists, regardless
/// of whether that build is production-promoted. Offline candidate replay uses
/// this narrower identity check and still leaves live attribution disabled.
pub fn state_damage_contribution_formula_target_matches(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
) -> Result<bool, String> {
    Ok(
        rdps_runtime_config_for_identity(deployment_id, client_build, protocol_pack_digest)?
            .is_some(),
    )
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetVulnerabilityRdpsRule {
    effect_id: i64,
    ability_id: i64,
    hit_event_id: i32,
    damage_attr_id: i64,
    projection: TargetVulnerabilityProjection,
    inactive_factor: Option<i64>,
    active_factor: Option<i64>,
    provider_raw_delta: i64,
    rounding: Option<String>,
    integer_projection: Option<String>,
    required_critical: RequiredCriticalObservation,
    required_lucky: bool,
    #[serde(default)]
    runtime_eligible: bool,
    active_observed_damage: Option<i64>,
    inactive_observed_damage: Option<i64>,
    required_source_class_id: Option<i32>,
    required_source_context_sha256: Option<String>,
    #[serde(default)]
    allowed_target_context_sha256: Vec<String>,
    #[serde(default)]
    ignored_context_effect_ids: Vec<i64>,
}

impl TargetVulnerabilityRdpsRule {
    fn active_factor(&self) -> Option<i64> {
        match self.projection {
            TargetVulnerabilityProjection::Integer => {
                self.inactive_factor?.checked_add(self.provider_raw_delta)
            }
            TargetVulnerabilityProjection::RationalObservedOutput => self.active_factor,
            TargetVulnerabilityProjection::PairedObservedOutput => None,
        }
    }

    fn fixed_point_rounding(&self) -> Option<PositiveFixedPointRounding> {
        if self.projection != TargetVulnerabilityProjection::Integer {
            return None;
        }
        match self.rounding.as_deref()? {
            "floor" => Some(PositiveFixedPointRounding::Floor),
            "half_up" => Some(PositiveFixedPointRounding::HalfUp),
            _ => None,
        }
    }

    fn is_valid(&self) -> bool {
        match self.projection {
            TargetVulnerabilityProjection::Integer => {
                self.inactive_factor.is_some_and(|factor| factor > 0)
                    && self.active_factor.is_none()
                    && self.provider_raw_delta > 0
                    && self.active_factor().is_some()
                    && self.fixed_point_rounding().is_some()
                    && self.integer_projection.is_none()
            }
            TargetVulnerabilityProjection::RationalObservedOutput => {
                self.inactive_factor.is_none()
                    && self
                        .active_factor
                        .is_some_and(|factor| factor > self.provider_raw_delta)
                    && self.provider_raw_delta > 0
                    && self.rounding.is_none()
                    && self.integer_projection.as_deref()
                        == Some("sum_exact_then_half_up_per_effect_provider_recipient")
            }
            TargetVulnerabilityProjection::PairedObservedOutput => {
                self.inactive_factor.is_none()
                    && self.active_factor.is_none()
                    && self.provider_raw_delta == 0
                    && self.rounding.is_none()
                    && self.integer_projection.as_deref() == Some("exact_packet_pair")
                    && self.active_observed_damage.is_some_and(|damage| damage > 0)
                    && self
                        .inactive_observed_damage
                        .is_some_and(|damage| damage >= 0)
                    && self.active_observed_damage > self.inactive_observed_damage
                    && self
                        .required_source_class_id
                        .is_some_and(|class_id| class_id > 0)
                    && self
                        .required_source_context_sha256
                        .as_deref()
                        .is_some_and(is_prefixed_sha256)
                    && !self.allowed_target_context_sha256.is_empty()
                    && self
                        .allowed_target_context_sha256
                        .iter()
                        .all(|digest| is_prefixed_sha256(digest))
                    && self
                        .ignored_context_effect_ids
                        .iter()
                        .all(|effect_id| *effect_id > 0 && *effect_id != self.effect_id)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
enum RequiredCriticalObservation {
    Unreported,
    ReportedFalse,
    ReportedTrue,
}

impl RequiredCriticalObservation {
    fn packet_value(self) -> Option<bool> {
        match self {
            Self::Unreported => None,
            Self::ReportedFalse => Some(false),
            Self::ReportedTrue => Some(true),
        }
    }

    fn matches(self, observed: Option<bool>) -> bool {
        self.packet_value() == observed
    }
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
enum TargetVulnerabilityProjection {
    #[serde(rename = "exact_integer")]
    Integer,
    #[serde(rename = "exact_rational_observed_output")]
    RationalObservedOutput,
    #[serde(rename = "exact_paired_observed_output")]
    PairedObservedOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TargetVulnerabilityDamageKey {
    ability_id: i64,
    hit_event_id: i32,
    critical: Option<bool>,
    lucky: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetVulnerabilityRdpsCatalog {
    schema_version: u16,
    game_build: String,
    rules: Vec<TargetVulnerabilityRdpsRule>,
    #[serde(skip)]
    rule_indices_by_ability: HashMap<i64, Vec<usize>>,
    #[serde(skip)]
    rule_indices_by_damage_key: HashMap<TargetVulnerabilityDamageKey, Vec<usize>>,
    #[serde(skip)]
    effect_ids: HashSet<i64>,
}

impl TargetVulnerabilityRdpsCatalog {
    fn rule_indices_for_ability(&self, ability_id: i64) -> &[usize] {
        self.rule_indices_by_ability
            .get(&ability_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn rule_indices_for_damage(
        &self,
        ability_id: i64,
        hit_event_id: Option<i32>,
        critical: Option<bool>,
        lucky: Option<bool>,
    ) -> &[usize] {
        let (Some(hit_event_id), Some(lucky)) = (hit_event_id, lucky) else {
            return &[];
        };
        self.rule_indices_by_damage_key
            .get(&TargetVulnerabilityDamageKey {
                ability_id,
                hit_event_id,
                critical,
                lucky,
            })
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

static TARGET_VULNERABILITY_RDPS_CATALOG: OnceLock<Result<TargetVulnerabilityRdpsCatalog, String>> =
    OnceLock::new();

fn target_vulnerability_rdps_catalog() -> Result<&'static TargetVulnerabilityRdpsCatalog, String> {
    TARGET_VULNERABILITY_RDPS_CATALOG
        .get_or_init(|| {
            let mut catalog: TargetVulnerabilityRdpsCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/external-target-vulnerability-rdps.v2.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR target-vulnerability rDPS catalog is invalid: {error}")
            })?;
            let runtime = rdps_runtime_config_for("global", &catalog.game_build)?
                .ok_or_else(|| "bundled BPSR target-vulnerability rDPS catalog has no matching formula authority".to_string())?;
            if catalog.schema_version != TARGET_VULNERABILITY_RDPS_SCHEMA_VERSION
                || catalog.game_build != runtime.game_build
                || catalog.rules.is_empty()
            {
                return Err(
                    "bundled BPSR target-vulnerability rDPS catalog has an unsupported shape"
                        .into(),
                );
            }
            if catalog.rules.iter().any(|rule| {
                rule.effect_id <= 0
                    || rule.ability_id <= 0
                    || rule.hit_event_id < 0
                    || rule.damage_attr_id <= 0
                    || !rule.is_valid()
            }) {
                return Err(
                    "bundled BPSR target-vulnerability rDPS rule contains an invalid value".into(),
                );
            }
            let mut exact_rule_keys = HashSet::new();
            for (index, rule) in catalog.rules.iter().enumerate() {
                let exact_rule_key = (
                    rule.effect_id,
                    rule.ability_id,
                    rule.hit_event_id,
                    rule.damage_attr_id,
                    rule.required_critical,
                    rule.required_lucky,
                );
                if !exact_rule_keys.insert(exact_rule_key) {
                    return Err(
                        "bundled BPSR target-vulnerability rDPS catalog contains a duplicate exact rule"
                            .into(),
                    );
                }
                catalog
                    .rule_indices_by_ability
                    .entry(rule.ability_id)
                    .or_default()
                    .push(index);
                catalog
                    .rule_indices_by_damage_key
                    .entry(TargetVulnerabilityDamageKey {
                        ability_id: rule.ability_id,
                        hit_event_id: rule.hit_event_id,
                        critical: rule.required_critical.packet_value(),
                        lucky: rule.required_lucky,
                    })
                    .or_default()
                    .push(index);
                catalog.effect_ids.insert(rule.effect_id);
            }
            if runtime
                .target_vulnerability_runtime_transfer_effect_ids()
                .iter()
                .any(|effect_id| !catalog.effect_ids.contains(effect_id))
            {
                return Err(
                    "bundled BPSR target-vulnerability runtime authority references an unknown rule"
                        .into(),
                );
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn is_target_vulnerability_effect(effect_id: i64) -> bool {
    target_vulnerability_rdps_catalog().is_ok_and(|catalog| catalog.effect_ids.contains(&effect_id))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateRdpsRule {
    effect_id: i64,
    source_config_id: i64,
    percentage_attribute_id: i32,
    raw_percent_per_stack: i64,
    maximum_stacks: u32,
    enabled_provider_raw_percent_values: Vec<i64>,
    final_attribute_id: i32,
    base_attribute_id: i32,
    intermediate_attribute_id: i32,
    extra_percentage_attribute_id: i32,
    ability_id: i64,
    state_multiplier: i64,
    constant_offset: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateRdpsCatalog {
    schema_version: u16,
    game_build: String,
    rules: Vec<StateRdpsRule>,
    #[serde(default)]
    candidate_rules: Vec<CandidateStateRdpsRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateStateRdpsRule {
    proof_state: String,
    runtime_eligible: bool,
    blocker: String,
    rule: StateRdpsRule,
}

static STATE_RDPS_CATALOG: OnceLock<Result<StateRdpsCatalog, String>> = OnceLock::new();

fn state_rdps_catalog() -> Result<&'static StateRdpsCatalog, String> {
    STATE_RDPS_CATALOG
        .get_or_init(|| {
            let catalog: StateRdpsCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/external-state-rdps.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR state rDPS catalog is invalid: {error}"))?;
            let runtime =
                rdps_runtime_config_for("global", &catalog.game_build)?.ok_or_else(|| {
                    "bundled BPSR state rDPS catalog has no matching formula authority".to_string()
                })?;
            if catalog.schema_version != STATE_RDPS_SCHEMA_VERSION
                || catalog.game_build != runtime.game_build
                || catalog.rules.len() > 1
            {
                return Err("bundled BPSR state rDPS catalog has an unsupported shape".into());
            }
            if catalog
                .rules
                .iter()
                .any(|rule| !valid_state_rdps_rule(rule))
                || catalog.candidate_rules.iter().any(|candidate| {
                    candidate.runtime_eligible
                        || candidate.proof_state.is_empty()
                        || candidate.blocker.is_empty()
                        || !valid_state_rdps_rule(&candidate.rule)
                })
            {
                return Err("bundled BPSR state rDPS rule contains an invalid value".into());
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn valid_state_rdps_rule(rule: &StateRdpsRule) -> bool {
    rule.effect_id > 0
        && rule.source_config_id > 0
        && rule.percentage_attribute_id > 0
        && rule.raw_percent_per_stack > 0
        && rule.maximum_stacks > 0
        && !rule.enabled_provider_raw_percent_values.is_empty()
        && rule
            .enabled_provider_raw_percent_values
            .iter()
            .all(|value| {
                *value > 0
                    && *value
                        <= i64::from(rule.maximum_stacks).saturating_mul(rule.raw_percent_per_stack)
            })
        && rule.final_attribute_id > 0
        && rule.base_attribute_id > 0
        && rule.intermediate_attribute_id > 0
        && rule.extra_percentage_attribute_id > 0
        && rule.ability_id > 0
        && rule.state_multiplier > 0
}

fn state_rdps_observation_rule() -> Result<Option<&'static StateRdpsRule>, String> {
    let catalog = state_rdps_catalog()?;
    Ok(catalog.rules.first().or_else(|| {
        catalog
            .candidate_rules
            .first()
            .map(|candidate| &candidate.rule)
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WireKey {
    connection_id: u64,
    stream_id: u64,
    capture_sequence: u64,
}

#[derive(Debug, Clone, Default)]
struct ActorHpState {
    current_value: Option<i64>,
    final_value: Option<i64>,
    base_value: Option<i64>,
    extra_add: Option<i64>,
    raw_percent: Option<i64>,
    intermediate_value: Option<i64>,
    raw_extra_percent: Option<i64>,
    /// Exact raw percentage currently proven per provider.
    provider_raw_percent: BTreeMap<u64, i64>,
    critical_damage_raw: Option<i64>,
    lucky_damage_raw: Option<i64>,
    physical_attack: AttackFamilyState,
    magical_attack: AttackFamilyState,
    /// Complete packet-observed primary-stat families used by Harmony Grace.
    /// Each class route is isolated and provider decomposition is invalidated
    /// whenever the exact +200 status transition does not explain the packet.
    harmony_primary_by_class: BTreeMap<i32, AttackFamilyState>,
    /// Packet-observed primary-stat families used by promoted Mechanical Power
    /// recipient rules. Each class is isolated so adding a future proven rule
    /// cannot reinterpret another class's attribute lane.
    mechanical_primary_by_class: BTreeMap<i32, AttackFamilyState>,
    primary_raw_add: [Option<i64>; 4],
    critical_chance_raw: Option<i64>,
    critical_chance_raw_add: Option<i64>,
    lucky_chance_raw: Option<i64>,
    lucky_chance_raw_add: Option<i64>,
    mastery_raw: Option<i64>,
    mastery_raw_add: Option<i64>,
    versatility_raw: Option<i64>,
    versatility_raw_add: Option<i64>,
    external_damage_raw: Option<i64>,
    /// Final packet-observed property-damage family used by Inspiration's
    /// Mastery-derived Light component (attribute 13170 in this build). It is
    /// never inferred from Mastery while the serialized derived update is
    /// pending.
    property_damage_raw: Option<i64>,
    /// Final packet-observed HastePct family (attribute 11930), in basis
    /// points. This is not the raw Haste rating family (attribute 11120).
    haste_percent_basis_points: Option<i64>,
    /// Exact packet-transition contribution currently proven per Inspiration
    /// provider. These are inputs to later damage stages, never additional
    /// damage rows of their own.
    inspiration_providers: BTreeMap<u64, InspirationProviderState>,
    /// One verified Thunderwind provider and its exact packet-observed final
    /// Crit Rate / Crit Damage components. The visible parent status owns both
    /// components; the self-sourced hidden child is only a lifecycle witness.
    thunderwind_providers: BTreeMap<u64, ThunderwindProviderState>,
    /// Complete packet-observed all-element fixed-point family used by
    /// Arcane! Fatal Spiral (legacy/internal name Highland Blood). Provider
    /// decomposition is retained only after the same wire proves the status
    /// owner and the exact six-component recipient transition.
    all_element: FixedPointFamilyState,
}

#[derive(Debug, Clone, Default)]
struct FixedPointFamilyState {
    current_value: Option<i64>,
    total_value: Option<i64>,
    add_value: Option<i64>,
    extra_add_value: Option<i64>,
    percent_value: Option<i64>,
    extra_percent_value: Option<i64>,
    provider_basis_points: BTreeMap<u64, i64>,
}

#[derive(Debug, Clone, Default)]
struct AttackFamilyState {
    final_value: Option<i64>,
    intermediate_value: Option<i64>,
    base_add: Option<i64>,
    extra_add: Option<i64>,
    raw_percent: Option<i64>,
    /// True only when the raw-percent field itself was present in a packet.
    /// Algebraically completed values may be refreshed by a later uniquely
    /// solvable delta family; packet-observed values are never overwritten.
    raw_percent_packet_observed: bool,
    /// Exact raw percentage currently proven per external provider.
    provider_raw_percent: BTreeMap<u64, i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct InspirationProviderState {
    provider_full_bloom: bool,
    primary_raw_add_delta: i64,
    secondary_raw_add_delta: i64,
    physical_attack_base_add_delta: Option<i64>,
    magical_attack_base_add_delta: Option<i64>,
    external_damage_delta: i64,
    /// Exact property-damage component established by its own packet
    /// transition. `None` deliberately represents the Mastery -> Light
    /// serialization gap and makes Light events ineligible for attribution.
    property_damage_delta: Option<i64>,
    haste_delta: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct InspirationWindow {
    provider_full_bloom: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ThunderwindProviderState {
    critical_chance_raw_delta: i64,
    critical_damage_raw_delta: i64,
}

#[derive(Debug, Clone, Copy)]
struct ThunderwindWindow {
    source_level: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct EffectWindowKey {
    target_actor_id: u64,
    provider_actor_id: u64,
    instance_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UnresolvedStatusWindowKey {
    target_actor_id: u64,
    instance_id: Option<i64>,
}

/// One exact, uncontaminated packet transition that proves a primary-stat
/// marginal for a specific lifecycle instance and active primary state. This
/// is deliberately narrower than a build-wide rounding rule: a damage row can
/// use it only while the same provider/recipient/instance is active and the
/// observed base/raw state matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PrimaryStatTransitionWitness {
    wire: WireKey,
    instance_id: Option<i64>,
    base_add: i64,
    active_raw_percent: i64,
    provider_raw_percent: i64,
    provider_primary_marginal: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FatalSpiralWindowKey {
    target_actor_id: u64,
    target_entity_uuid: i64,
    provider_actor_id: u64,
    provider_entity_uuid: i64,
    instance_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixedPointFamilyTransition {
    provider_actor_id: u64,
    provider_entity_uuid: i64,
    active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TeamLuckWindowKey {
    target_actor_id: u64,
    target_entity_uuid: i64,
    provider_actor_id: u64,
    provider_entity_uuid: i64,
    instance_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FunctionalAmpWindowKey {
    target_actor_id: u64,
    target_entity_uuid: i64,
    provider_actor_id: u64,
    provider_entity_uuid: i64,
    instance_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TargetVulnerabilityWindowKey {
    target_actor_id: u64,
    provider_actor_id: u64,
    effect_id: i64,
    instance_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TargetVulnerabilityTransitionKey {
    target_actor_id: u64,
    provider_actor_id: u64,
    effect_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FormulaStatusKey {
    target_actor_id: u64,
    source_actor_id: u64,
    effect_id: i64,
    instance_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FormulaStatusValue {
    effect_id: i64,
    stacks: i64,
    level: i64,
}

#[derive(Debug, Clone, Copy)]
struct TargetVulnerabilityWindow {
    expires_at_observed_micros: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct EffectWindow {
    desired_stacks: u32,
}

/// Last target-vulnerability decision made for a damage event. This is exposed
/// to the offline replay audit so packet-proof coverage can be measured without
/// adding a second attribution implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetVulnerabilityAudit {
    pub gate: &'static str,
    pub exact_candidate_count: usize,
    pub rational_candidate_count: usize,
    pub unresolved_attack_overlap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttackContributionOverlapPolicy {
    None,
    Single,
    HarmonyFunctionalAmp,
    Suppress,
}

fn attack_contribution_overlap_policy(
    functional_amp: bool,
    harmony_grace: bool,
    mechanical_power: bool,
    inspiration: bool,
) -> AttackContributionOverlapPolicy {
    match (functional_amp, harmony_grace, mechanical_power, inspiration) {
        (true, true, false, false) => AttackContributionOverlapPolicy::HarmonyFunctionalAmp,
        (true, false, false, false)
        | (false, true, false, false)
        | (false, false, true, false)
        | (false, false, false, true) => AttackContributionOverlapPolicy::Single,
        (false, false, false, false) => AttackContributionOverlapPolicy::None,
        _ => AttackContributionOverlapPolicy::Suppress,
    }
}

/// Audit-only arithmetic receipt for one successfully projected Harmony Grace
/// damage row. Every value is taken from the selected build runtime and the
/// packet-observed recipient state; no profile snapshot is consulted.
#[derive(Debug, Clone)]
pub struct HarmonyGraceFormulaTrace {
    pub effect_id: i64,
    pub provider_actor_id: u64,
    pub recipient_actor_id: u64,
    pub recipient_class_id: i32,
    pub attack_lane: &'static str,
    pub ability_id: i64,
    pub hit_event_id: Option<i32>,
    pub damage_attr_id: i64,
    pub observed_damage: i64,
    pub primary_final: i64,
    pub primary_intermediate: i64,
    pub primary_base_add: i64,
    pub primary_extra_add: i64,
    pub primary_raw_percent: i64,
    pub primary_family_rounding: &'static str,
    pub provider_primary_raw_percent: i64,
    pub primary_provider_marginal_basis: &'static str,
    pub primary_transition_connection_id: u64,
    pub primary_transition_stream_id: u64,
    pub primary_transition_capture_sequence: u64,
    pub primary_transition_instance_id: Option<i64>,
    pub primary_provider_marginal: i64,
    pub primary_without_provider: i64,
    pub primary_to_attack_numerator: i64,
    pub primary_to_attack_denominator: i64,
    pub attack_component_with_provider: i64,
    pub attack_component_without_provider: i64,
    pub provider_attack_base_add: i64,
    pub attack_final: i64,
    pub attack_intermediate: i64,
    pub attack_base_add: i64,
    pub attack_extra_add: i64,
    pub attack_raw_percent: i64,
    pub provider_attack_marginal: i64,
    pub attack_without_provider: i64,
    pub coefficient_basis_points: i64,
    pub fixed_parameter: i64,
    pub active_coefficient_term: i64,
    pub active_stage_body: i64,
    pub without_provider_coefficient_term: i64,
    pub coefficient_stage_marginal: i64,
    pub contribution_numerator: i128,
    pub contribution_denominator: i128,
}

/// One distinct complete packet state observed while an external Harmony Grace
/// window is active. The offline replay audit uses this to distinguish floor
/// from round-to-nearest semantics before any production formula is changed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct HarmonyGraceFamilyRoundingDiagnostic {
    pub provider_actor_id: u64,
    pub recipient_actor_id: u64,
    pub recipient_class_id: i32,
    pub provider_decomposition_matches: bool,
    pub primary_base_add: i64,
    pub primary_raw_percent: i64,
    pub primary_extra_add: i64,
    pub primary_observed_intermediate: i64,
    pub primary_floor_intermediate: i64,
    pub primary_nearest_intermediate: i64,
    pub primary_observed_final: i64,
    pub primary_floor_final: i64,
    pub primary_nearest_final: i64,
    pub attack_base_add: i64,
    pub attack_raw_percent: i64,
    pub attack_extra_add: i64,
    pub attack_observed_intermediate: i64,
    pub attack_floor_intermediate: i64,
    pub attack_nearest_intermediate: i64,
    pub attack_observed_final: i64,
    pub attack_floor_final: i64,
    pub attack_nearest_final: i64,
}

/// Current-build BPSR projector for externally supplied state that changes a
/// state-scaled damage action. Its state is O(players + active effect windows)
/// and no packet archive is replayed on the live path.
#[derive(Debug)]
pub struct BpsrStateDamageContributionProjector {
    runtime: &'static RdpsRuntimeConfig,
    /// Offline proof tooling may pass packet-observed Inspiration state through
    /// the candidate counterfactual so its remaining gates can be measured.
    /// This is false for every production projector and does not promote the
    /// rule into history or the live overlay.
    inspiration_candidate_audit_enabled: bool,
    /// Offline proof tooling may replay Harmony Grace end to end while the
    /// global production promotion gate remains closed. Audit output is
    /// filtered to effect 3003052 and `enabled()` remains false.
    harmony_grace_candidate_audit_enabled: bool,
    /// Offline proof tooling may replay Mechanical Power's exact lifecycle and
    /// recipient transition while the production pack/promotion gates remain
    /// closed. Audit output is filtered to effect 2110140.
    mechanical_power_candidate_audit_enabled: bool,
    /// Exact observed magnitude selected by a bounded offline Mechanical Power
    /// audit. Production projectors and the ordinary tier-5 audit leave this
    /// unset and use the versioned runtime rule.
    mechanical_power_candidate_primary_percent_override: Option<i64>,
    /// Offline proof tooling may replay the exact action-scoped target-
    /// vulnerability rule while the global production promotion gate remains
    /// closed. Audit output is filtered to this candidate family and
    /// `enabled()` remains false.
    target_vulnerability_candidate_audit_enabled: bool,
    runtime_applicable: bool,
    observed_deployment_id: Option<String>,
    observed_client_build: Option<String>,
    observed_protocol_pack_digest: Option<String>,
    current_wire: Option<WireKey>,
    states: HashMap<u64, ActorHpState>,
    staged_states: HashMap<u64, ActorHpState>,
    /// Exact entity identity attached to the actor that most recently carried
    /// an attribute snapshot/delta. Damage actor ids may rotate while the
    /// stable entity UUID remains unchanged, so the offline gate audit uses
    /// this index to distinguish genuinely absent recipient state from state
    /// retained under an earlier exact actor alias.
    attribute_state_actor_by_entity_uuid: HashMap<i64, u64>,
    attribute_state_entity_uuid_by_actor: HashMap<u64, i64>,
    /// Audit-only lifetime markers for the two Team Luck recipient lanes. They
    /// distinguish attributes the receiving client never supplied from values
    /// that were supplied and later removed by an authoritative snapshot. The
    /// markers never substitute state into attribution.
    team_luck_critical_ever_observed: HashSet<u64>,
    team_luck_lucky_ever_observed: HashSet<u64>,
    team_luck_critical_cleared_by_snapshot: HashSet<u64>,
    team_luck_lucky_cleared_by_snapshot: HashSet<u64>,
    effect_windows: HashMap<EffectWindowKey, EffectWindow>,
    team_luck_windows: HashSet<TeamLuckWindowKey>,
    team_luck_transition_wire: Option<WireKey>,
    functional_amp_windows: HashSet<FunctionalAmpWindowKey>,
    functional_amp_transition_wires: HashMap<u64, WireKey>,
    mechanical_power_windows: HashSet<EffectWindowKey>,
    mechanical_power_transition_wires: HashMap<u64, WireKey>,
    mechanical_power_primary_transition_witnesses:
        HashMap<EffectWindowKey, HashSet<PrimaryStatTransitionWitness>>,
    harmony_grace_windows: HashSet<EffectWindowKey>,
    harmony_grace_transition_wires: HashMap<u64, WireKey>,
    harmony_grace_primary_transition_witnesses:
        HashMap<EffectWindowKey, HashSet<PrimaryStatTransitionWitness>>,
    thunderwind_windows: HashMap<EffectWindowKey, ThunderwindWindow>,
    thunderwind_child_targets: HashSet<u64>,
    thunderwind_transition_wires: HashMap<u64, WireKey>,
    full_bloom_targets: HashSet<u64>,
    inspiration_windows: HashMap<EffectWindowKey, InspirationWindow>,
    inspiration_transition_wires: HashMap<u64, WireKey>,
    inspiration_snapshot_targets: HashSet<u64>,
    fatal_spiral_windows: HashSet<FatalSpiralWindowKey>,
    fatal_spiral_transitions: HashMap<u64, Vec<FixedPointFamilyTransition>>,
    fatal_spiral_snapshot_targets: HashSet<u64>,
    /// Exact provider magnitudes learned from a recipient's complete packet
    /// transition. Entity UUID is used because actor IDs are lifetime-local.
    fatal_spiral_provider_basis_points_by_entity_uuid: HashMap<i64, i64>,
    /// Conflicting observations are retained as an explicit ambiguity gate;
    /// they are never silently replaced by the newest value.
    fatal_spiral_ambiguous_provider_entities: HashSet<i64>,
    target_vulnerability_windows: HashMap<TargetVulnerabilityWindowKey, TargetVulnerabilityWindow>,
    target_vulnerability_transitions: HashSet<TargetVulnerabilityTransitionKey>,
    /// Full packet-observed state used only by exact paired-output rules. The
    /// digest gate prevents a sealed counterfactual from leaking into a
    /// different remote build or overlapping status context.
    formula_attributes_by_actor: HashMap<u64, BTreeMap<i32, i64>>,
    formula_statuses: HashMap<FormulaStatusKey, FormulaStatusValue>,
    /// An unresolved lifecycle may be an offensive buff on a damage source or
    /// a vulnerability/mitigation effect on a damage target. Until an actor or
    /// run lifetime boundary supplies a clean state, no rDPS transfer involving
    /// that actor can be proven complete.
    unresolved_status_windows: HashSet<UnresolvedStatusWindowKey>,
    actor_ancestry: ActorAncestryResolver,
    latest_observed_micros: u64,
    entity_type_by_actor: HashMap<u64, i32>,
    summon_config_by_actor: HashMap<u64, i64>,
    /// Exact class identity observed inside this actor lifetime. Keeping it in
    /// the run projector prevents a later profile snapshot from rewriting an
    /// archived run's attribution classification.
    class_id_by_actor: HashMap<u64, i32>,
    active_players: HashSet<u64>,
    observed_ability_ids_by_actor: HashMap<u64, HashSet<i64>>,
    last_target_vulnerability_audit: Option<TargetVulnerabilityAudit>,
}

impl Default for BpsrStateDamageContributionProjector {
    fn default() -> Self {
        Self {
            runtime: rdps_runtime_config()
                .expect("bundled rDPS formula pack must be validated before projector use"),
            inspiration_candidate_audit_enabled: false,
            harmony_grace_candidate_audit_enabled: false,
            mechanical_power_candidate_audit_enabled: false,
            mechanical_power_candidate_primary_percent_override: None,
            target_vulnerability_candidate_audit_enabled: false,
            runtime_applicable: false,
            observed_deployment_id: None,
            observed_client_build: None,
            observed_protocol_pack_digest: None,
            current_wire: None,
            states: HashMap::new(),
            staged_states: HashMap::new(),
            attribute_state_actor_by_entity_uuid: HashMap::new(),
            attribute_state_entity_uuid_by_actor: HashMap::new(),
            team_luck_critical_ever_observed: HashSet::new(),
            team_luck_lucky_ever_observed: HashSet::new(),
            team_luck_critical_cleared_by_snapshot: HashSet::new(),
            team_luck_lucky_cleared_by_snapshot: HashSet::new(),
            effect_windows: HashMap::new(),
            team_luck_windows: HashSet::new(),
            team_luck_transition_wire: None,
            functional_amp_windows: HashSet::new(),
            functional_amp_transition_wires: HashMap::new(),
            mechanical_power_windows: HashSet::new(),
            mechanical_power_transition_wires: HashMap::new(),
            mechanical_power_primary_transition_witnesses: HashMap::new(),
            harmony_grace_windows: HashSet::new(),
            harmony_grace_transition_wires: HashMap::new(),
            harmony_grace_primary_transition_witnesses: HashMap::new(),
            thunderwind_windows: HashMap::new(),
            thunderwind_child_targets: HashSet::new(),
            thunderwind_transition_wires: HashMap::new(),
            full_bloom_targets: HashSet::new(),
            inspiration_windows: HashMap::new(),
            inspiration_transition_wires: HashMap::new(),
            inspiration_snapshot_targets: HashSet::new(),
            fatal_spiral_windows: HashSet::new(),
            fatal_spiral_transitions: HashMap::new(),
            fatal_spiral_snapshot_targets: HashSet::new(),
            fatal_spiral_provider_basis_points_by_entity_uuid: HashMap::new(),
            fatal_spiral_ambiguous_provider_entities: HashSet::new(),
            target_vulnerability_windows: HashMap::new(),
            target_vulnerability_transitions: HashSet::new(),
            formula_attributes_by_actor: HashMap::new(),
            formula_statuses: HashMap::new(),
            unresolved_status_windows: HashSet::new(),
            actor_ancestry: ActorAncestryResolver::default(),
            latest_observed_micros: 0,
            entity_type_by_actor: HashMap::new(),
            summon_config_by_actor: HashMap::new(),
            class_id_by_actor: HashMap::new(),
            active_players: HashSet::new(),
            observed_ability_ids_by_actor: HashMap::new(),
            last_target_vulnerability_audit: None,
        }
    }
}

impl BpsrStateDamageContributionProjector {
    pub fn new() -> Result<Self, String> {
        rdps_runtime_config()?;
        state_rdps_catalog()?;
        target_vulnerability_rdps_catalog()?;
        validate_damage_stage_catalog()?;
        Ok(Self::default())
    }

    /// Constructs an offline replay projector that evaluates the unpromoted
    /// Inspiration formula against captured packet evidence. This constructor
    /// must never be used by production reducers; it exists so the audit can
    /// expose the next exact proof gate instead of stopping at the policy gate.
    pub fn new_inspiration_candidate_audit() -> Result<Self, String> {
        let mut projector = Self::new()?;
        projector.inspiration_candidate_audit_enabled = true;
        Ok(projector)
    }

    /// Constructs an audit-only projector for one complete external-buff
    /// experiment: Harmony Grace status ownership, recipient stat transition,
    /// attack-family conversion, and the affected damage counterfactual.
    /// Production reducers never call this constructor.
    pub fn new_harmony_grace_candidate_audit() -> Result<Self, String> {
        let mut projector = Self::new()?;
        projector.harmony_grace_candidate_audit_enabled = true;
        Ok(projector)
    }

    /// Constructs an audit-only projector for Mechanical Power's provider,
    /// lifecycle, recipient transition, primary-to-Attack conversion, and
    /// damage-stage chain. Production reducers never call this constructor.
    pub fn new_mechanical_power_candidate_audit() -> Result<Self, String> {
        let mut projector = Self::new()?;
        projector.mechanical_power_candidate_audit_enabled = true;
        Ok(projector)
    }

    /// Constructs the same audit-only Mechanical Power projector for the
    /// separately observed tier-0 lifecycle magnitude. This is deliberately a
    /// fixed exact-build value, not a caller-selected override or a universal
    /// interpolation across unobserved tiers.
    pub fn new_mechanical_power_tier0_candidate_audit() -> Result<Self, String> {
        const EXACT_TIER0_PRIMARY_PERCENT_RAW_DELTA: i64 = 750;
        let mut projector = Self::new()?;
        if !projector
            .runtime
            .mechanical_power
            .recipient_rules
            .iter()
            .any(|rule| rule.recipient_class_id == 11)
        {
            return Err("Mechanical Power tier-0 audit recipient rule is missing".into());
        }
        projector.mechanical_power_candidate_primary_percent_override =
            Some(EXACT_TIER0_PRIMARY_PERCENT_RAW_DELTA);
        projector.mechanical_power_candidate_audit_enabled = true;
        Ok(projector)
    }

    /// Constructs an offline exact-build replay projector for the reviewed
    /// action-scoped target-vulnerability candidate. This never changes the
    /// production promotion gate and is not exposed to runtime consumers.
    pub fn new_target_vulnerability_candidate_audit() -> Result<Self, String> {
        let mut projector = Self::new()?;
        projector.target_vulnerability_candidate_audit_enabled = true;
        Ok(projector)
    }

    fn observe_timeline(
        &mut self,
        envelope: &EventEnvelope,
        output: &mut Vec<ExactDamageContributionEvent>,
        rational_output: &mut Vec<ExactRationalDamageContributionEvent>,
    ) {
        let exact_output_start = output.len();
        let rational_output_start = rational_output.len();
        self.last_target_vulnerability_audit = None;
        self.latest_observed_micros = envelope.time.observed_micros;
        let selected_runtime = rdps_runtime_config_for_identity(
            &envelope.region.identity.deployment_id,
            &envelope.region.client_build,
            &envelope.region.protocol_pack_digest,
        )
        .ok()
        .flatten();
        let Some(selected_runtime) = selected_runtime else {
            self.observed_deployment_id = Some(envelope.region.identity.deployment_id.clone());
            self.observed_client_build = Some(envelope.region.client_build.clone());
            self.observed_protocol_pack_digest = Some(envelope.region.protocol_pack_digest.clone());
            self.runtime_applicable = false;
            self.clear_state();
            return;
        };
        if !std::ptr::eq(self.runtime, selected_runtime) {
            self.clear_state();
            self.runtime = selected_runtime;
        }
        if self.observed_deployment_id.as_deref()
            != Some(envelope.region.identity.deployment_id.as_str())
        {
            self.observed_deployment_id = Some(envelope.region.identity.deployment_id.clone());
        }
        if self.observed_client_build.as_deref() != Some(envelope.region.client_build.as_str()) {
            self.observed_client_build = Some(envelope.region.client_build.clone());
        }
        if self.observed_protocol_pack_digest.as_deref()
            != Some(envelope.region.protocol_pack_digest.as_str())
        {
            self.observed_protocol_pack_digest = Some(envelope.region.protocol_pack_digest.clone());
        }
        let deployment_matches =
            envelope.region.identity.deployment_id == self.runtime.deployment_id;
        let build_matches = envelope.region.client_build == self.runtime.game_build;
        let protocol_pack_matches =
            envelope.region.protocol_pack_digest == self.runtime.protocol_pack_digest;
        self.runtime_applicable = deployment_matches
            && build_matches
            && protocol_pack_matches
            && self.runtime.has_any_runtime_transfer_enabled();
        let offline_candidate_audit_applicable = deployment_matches
            && build_matches
            && protocol_pack_matches
            && (self.inspiration_candidate_audit_enabled
                || self.harmony_grace_candidate_audit_enabled
                || self.mechanical_power_candidate_audit_enabled
                || self.target_vulnerability_candidate_audit_enabled);
        if !self.runtime_applicable && !offline_candidate_audit_applicable {
            // Regional deployments can use different identities and formulas.
            // Preserve their canonical events, but never apply Global rules.
            self.clear_state();
            return;
        }
        let Some(wire) = wire_key(envelope) else {
            return;
        };
        self.advance_wire(wire);
        self.expire_target_vulnerability_windows(envelope.time.observed_micros);
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary {
                state: RunState::Entered,
                ..
            } => self.clear_run_state(),
            TimelineEventKind::EncounterBoundary {
                state: EncounterState::Wiped,
                ..
            } => self.clear_run_state(),
            TimelineEventKind::Actor(actor) => {
                let actor_id = actor.actor.actor_id.0;
                if matches!(
                    actor.state,
                    ActorState::Spawned | ActorState::Transformed | ActorState::Despawned
                ) {
                    self.clear_actor(actor_id);
                    self.actor_ancestry
                        .clear_owner(envelope.time.observed_micros, actor.actor);
                }
                if actor.state != ActorState::Despawned {
                    self.actor_ancestry.observe_entity(actor.actor);
                    self.entity_type_by_actor
                        .insert(actor_id, actor.entity_type_id);
                    if let Some(class_id) = actor.class_id {
                        self.class_id_by_actor.insert(actor_id, class_id);
                    }
                }
                if actor.state != ActorState::Despawned && actor.kind == ActorKind::Player {
                    self.active_players.insert(actor_id);
                } else if actor.state == ActorState::Despawned {
                    self.active_players.remove(&actor_id);
                }
            }
            TimelineEventKind::Status(status) => {
                self.observe_status(status, envelope.time.observed_micros)
            }
            TimelineEventKind::UnresolvedStatus(status) => {
                self.observe_unresolved_status(status);
            }
            TimelineEventKind::EntityAttributes(attributes) => self.observe_attributes(
                attributes.actor.actor_id.0,
                attributes.actor.entity_uuid.0,
                attributes.update_kind,
                attributes.ownership.as_ref(),
                &attributes.attributes,
                envelope.time.observed_micros,
            ),
            TimelineEventKind::Damage(damage) => {
                self.actor_ancestry
                    .observe_damage(envelope.time.observed_micros, damage);
                if let Some(ability) = damage.ability {
                    self.observed_ability_ids_by_actor
                        .entry(damage.source.actor_id.0)
                        .or_default()
                        .insert(ability.0);
                }
                if self.damage_has_unresolved_status_confounder(damage) {
                    self.last_target_vulnerability_audit = Some(TargetVulnerabilityAudit {
                        gate: "unresolved_status_confounder",
                        exact_candidate_count: 0,
                        rational_candidate_count: 0,
                        unresolved_attack_overlap: false,
                    });
                    return;
                }
                let rule = state_rdps_catalog()
                    .expect("catalog was validated when the projector was built")
                    .rules
                    .first();
                let recipient_actor_id = damage.source.actor_id.0;
                let observed_damage = damage.amount;
                let state_contribution = rule.and_then(|rule| {
                    (damage.ability.map(|ability| ability.0) == Some(rule.ability_id))
                        .then(|| {
                            self.exact_damage_marginal(rule, recipient_actor_id, observed_damage)
                                .map(|(provider_actor_id, amount)| ExactDamageContributionEvent {
                                    observed_micros: envelope.time.observed_micros,
                                    effect_id: rule.effect_id,
                                    provider_actor_id,
                                    recipient_actor_id,
                                    amount,
                                    observed_damage,
                                    included: true,
                                })
                        })
                        .flatten()
                });
                let team_luck_contribution = self.team_luck_contribution(envelope, damage);
                let functional_amp_contribution =
                    self.functional_amp_contribution(envelope.time.observed_micros, damage);
                let harmony_grace_contribution =
                    self.harmony_grace_contribution(envelope.time.observed_micros, damage);
                let mechanical_power_contribution =
                    self.mechanical_power_contribution(envelope.time.observed_micros, damage);
                let mut inspiration_contribution =
                    self.inspiration_contribution(envelope.time.observed_micros, damage);
                let inspiration_occurrence_contribution =
                    inspiration_contribution.as_ref().and_then(|_| {
                        self.inspiration_occurrence_contribution(
                            envelope.time.observed_micros,
                            damage,
                        )
                    });
                if inspiration_contribution.is_some()
                    && (damage.flags.critical == Some(true) || damage.flags.lucky == Some(true))
                    && inspiration_occurrence_contribution.is_none()
                {
                    // A flagged row needs both the base/factor and occurrence
                    // components. Emitting only one would present partial
                    // Inspiration credit as a complete effect contribution.
                    inspiration_contribution = None;
                }
                let thunderwind_contribution =
                    self.thunderwind_contribution(envelope.time.observed_micros, damage);
                let target_vulnerability_catalog = target_vulnerability_rdps_catalog()
                    .expect("catalog was validated when the projector was built");
                let target_vulnerability_rule_indices = damage
                    .ability
                    .map(|ability| ability.0)
                    .into_iter()
                    .flat_map(|ability_id| {
                        target_vulnerability_catalog.rule_indices_for_damage(
                            ability_id,
                            damage.hit_event_id,
                            damage.flags.critical,
                            damage.flags.lucky,
                        )
                    })
                    .copied()
                    .filter(|index| {
                        target_vulnerability_catalog.rules[*index].runtime_eligible
                            || self.target_vulnerability_candidate_audit_enabled
                    })
                    .collect::<Vec<_>>();
                let target_vulnerability_contributions = target_vulnerability_rule_indices
                    .iter()
                    .filter_map(|index| {
                        self.target_vulnerability_exact_contribution(
                            envelope,
                            damage,
                            &target_vulnerability_catalog.rules[*index],
                        )
                    })
                    .collect::<Vec<_>>();
                let target_vulnerability_rational_contributions = target_vulnerability_rule_indices
                    .iter()
                    .filter_map(|index| {
                        self.target_vulnerability_rational_contribution(
                            envelope,
                            damage,
                            &target_vulnerability_catalog.rules[*index],
                        )
                    })
                    .collect::<Vec<_>>();

                let exact_candidate_count = usize::from(state_contribution.is_some())
                    + target_vulnerability_contributions.len();
                let attack_overlap_policy = attack_contribution_overlap_policy(
                    functional_amp_contribution.is_some(),
                    harmony_grace_contribution.is_some(),
                    mechanical_power_contribution.is_some(),
                    inspiration_contribution.is_some(),
                );
                let (attack_contributions, unresolved_attack_overlap) = match attack_overlap_policy
                {
                    AttackContributionOverlapPolicy::HarmonyFunctionalAmp => self
                        .combined_harmony_functional_amp_contributions(
                            envelope.time.observed_micros,
                            damage,
                            functional_amp_contribution
                                .expect("overlap policy requires Functional Amp"),
                            harmony_grace_contribution
                                .expect("overlap policy requires Harmony Grace"),
                        )
                        .map(|contributions| (Vec::from(contributions), false))
                        .unwrap_or_else(|| (Vec::new(), true)),
                    AttackContributionOverlapPolicy::Single => (
                        vec![
                            functional_amp_contribution
                                .or(harmony_grace_contribution)
                                .or(mechanical_power_contribution)
                                .or(inspiration_contribution)
                                .expect("single overlap policy requires one contribution"),
                        ],
                        false,
                    ),
                    AttackContributionOverlapPolicy::None => (Vec::new(), false),
                    // Mechanical Power changes the primary-stat/base-Add
                    // stage. Until a capture proves an allocation order
                    // for overlapping base-Add providers (or its ordered
                    // cross-term with Functional Amp), retain damage but
                    // emit no guessed transfer.
                    AttackContributionOverlapPolicy::Suppress => (Vec::new(), true),
                };
                let rational_candidate_count = usize::from(team_luck_contribution.is_some())
                    + attack_contributions.len()
                    + usize::from(inspiration_occurrence_contribution.is_some())
                    + usize::from(thunderwind_contribution.is_some())
                    + target_vulnerability_rational_contributions.len();
                let vulnerability_gate = self.target_vulnerability_audit_gate(damage);
                let has_target_vulnerability_candidate = !target_vulnerability_contributions
                    .is_empty()
                    || !target_vulnerability_rational_contributions.is_empty();
                self.last_target_vulnerability_audit = Some(TargetVulnerabilityAudit {
                    gate: if has_target_vulnerability_candidate {
                        if exact_candidate_count == 1 && rational_candidate_count == 0 {
                            "emitted_exact"
                        } else if exact_candidate_count == 0 && rational_candidate_count == 1 {
                            "emitted_rational"
                        } else if exact_candidate_count > 1 {
                            "suppressed_exact_overlap"
                        } else if rational_candidate_count > 0 {
                            "suppressed_rational_overlap"
                        } else {
                            "candidate_not_emitted"
                        }
                    } else {
                        vulnerability_gate
                    },
                    exact_candidate_count,
                    rational_candidate_count,
                    unresolved_attack_overlap,
                });
                if exact_candidate_count == 1 && rational_candidate_count == 0 {
                    if let Some(contribution) = state_contribution {
                        output.push(contribution);
                    }
                    output.extend(target_vulnerability_contributions);
                } else if exact_candidate_count == 0 {
                    if unresolved_attack_overlap {
                        // Both windows exist, but their exact adjacent Attack
                        // counterfactuals were not reproducible from this
                        // packet state. Retain the damage without guessing.
                        return;
                    }
                    let later_contribution = match (
                        team_luck_contribution,
                        inspiration_occurrence_contribution,
                        thunderwind_contribution,
                        target_vulnerability_rational_contributions.as_slice(),
                    ) {
                        (Some(team_luck), None, None, []) => Some(team_luck),
                        (None, Some(inspiration), None, []) => Some(inspiration),
                        (None, None, Some(thunderwind), []) => Some(thunderwind),
                        (None, None, None, [target_vulnerability]) => Some(*target_vulnerability),
                        (None, None, None, []) => None,
                        // Team Luck and Thunderwind both modify the critical
                        // stage. Target vulnerability is also a later damage
                        // bucket. Any unresolved multi-provider allocation or
                        // shared cross-term remains fail-closed.
                        _ => return,
                    };
                    if attack_contributions.is_empty() {
                        if let Some(later) = later_contribution {
                            rational_output.push(later);
                        }
                    } else if let Some(later) = later_contribution {
                        if let Some(later_after_attack) =
                            scale_later_rational_marginal_after_many(&attack_contributions, later)
                        {
                            rational_output.extend(attack_contributions);
                            rational_output.push(later_after_attack);
                        }
                    } else {
                        rational_output.extend(attack_contributions);
                    }
                } else if exact_candidate_count + rational_candidate_count > 1 {
                    // Removing multiple external mechanics independently can
                    // count their shared multiplicative cross-term twice.
                    // Keep the packet damage and emit no transfer until the
                    // combined stage order is proven and conserved.
                }
            }
            TimelineEventKind::Healing(healing) => {
                self.actor_ancestry.observe_attributed_source(
                    envelope.time.observed_micros,
                    healing.source,
                    healing.direct_source,
                );
                self.actor_ancestry.observe_entity(healing.target);
            }
            _ => {}
        }
        if !self.runtime_applicable && offline_candidate_audit_applicable {
            let exact_candidate_effect_ids = &target_vulnerability_rdps_catalog()
                .expect("catalog was validated when the projector was built")
                .effect_ids;
            let retained_exact = output
                .drain(exact_output_start..)
                .filter(|contribution| {
                    self.target_vulnerability_candidate_audit_enabled
                        && exact_candidate_effect_ids.contains(&contribution.effect_id)
                })
                .collect::<Vec<_>>();
            output.extend(retained_exact);
            let retained_rational = rational_output
                .drain(rational_output_start..)
                .filter(|contribution| {
                    (self.inspiration_candidate_audit_enabled
                        && contribution.effect_id == self.runtime.inspiration.effect_id)
                        || (self.harmony_grace_candidate_audit_enabled
                            && contribution.effect_id == self.runtime.harmony_grace.effect_id)
                        || (self.mechanical_power_candidate_audit_enabled
                            && contribution.effect_id == self.runtime.mechanical_power.effect_id)
                        || (self.target_vulnerability_candidate_audit_enabled
                            && exact_candidate_effect_ids.contains(&contribution.effect_id))
                })
                .collect::<Vec<_>>();
            rational_output.extend(retained_rational);
        } else if self.runtime_applicable && !self.runtime.runtime_promotion_allowed() {
            // A component-scoped promotion must not accidentally expose any
            // other candidate computed by the shared projector. Canonical
            // events and ordinary damage remain untouched; only provider
            // transfer rows are filtered to exact effect authority.
            let retained_exact = output
                .drain(exact_output_start..)
                .filter(|contribution| {
                    self.runtime
                        .effect_runtime_transfer_enabled(contribution.effect_id)
                })
                .collect::<Vec<_>>();
            output.extend(retained_exact);
            let retained_rational = rational_output
                .drain(rational_output_start..)
                .filter(|contribution| {
                    self.runtime
                        .effect_runtime_transfer_enabled(contribution.effect_id)
                })
                .collect::<Vec<_>>();
            rational_output.extend(retained_rational);
        }
    }

    fn advance_wire(&mut self, wire: WireKey) {
        if self.current_wire.is_some_and(|current| current != wire) {
            self.reconcile_inspiration_staged_states();
            self.reconcile_thunderwind_staged_states();
            self.reconcile_fatal_spiral_staged_states();
            self.states.extend(self.staged_states.drain());
            self.team_luck_transition_wire = None;
            self.functional_amp_transition_wires.clear();
            self.mechanical_power_transition_wires.clear();
            self.harmony_grace_transition_wires.clear();
            self.inspiration_transition_wires.clear();
            self.inspiration_snapshot_targets.clear();
            self.thunderwind_transition_wires.clear();
            self.fatal_spiral_transitions.clear();
            self.fatal_spiral_snapshot_targets.clear();
            self.target_vulnerability_transitions.clear();
        }
        self.current_wire = Some(wire);
    }

    fn damage_has_unresolved_status_confounder(&self, damage: &rlogs_events::DamageEvent) -> bool {
        self.unresolved_status_windows.iter().any(|window| {
            window.target_actor_id == damage.source.actor_id.0
                || window.target_actor_id == damage.target.actor_id.0
        })
    }

    fn observe_unresolved_status(&mut self, status: &rlogs_events::UnresolvedStatusEvent) {
        let key = UnresolvedStatusWindowKey {
            target_actor_id: status.target.actor_id.0,
            instance_id: status.instance_id.map(|instance| instance.0),
        };
        match status.state {
            Some(StatusState::Consumed | StatusState::Removed) => {
                // A terminal event proves that this exact unresolved instance
                // is not active after the packet. A terminal-only snapshot row
                // therefore must not poison all later damage in the run.
                if key.instance_id.is_some() {
                    self.unresolved_status_windows.remove(&key);
                }
            }
            Some(StatusState::Applied | StatusState::Refreshed | StatusState::Stacked) | None => {
                // Missing state remains a possible active lifecycle. Preserve
                // it as a confounder until an exact instance terminal or actor
                // boundary proves that the window ended.
                self.unresolved_status_windows.insert(key);
            }
        }
    }

    fn observe_status(&mut self, status: &rlogs_events::StatusEvent, observed_micros: u64) {
        self.observe_formula_status(status);
        if status.effect.0 == self.runtime.inspiration.full_bloom_effect_id
            && status.origin.map(|origin| origin.source_config_id)
                == Some(self.runtime.inspiration.full_bloom_source_config_id)
        {
            self.observe_full_bloom_status(status);
        }
        if status.effect.0 == self.runtime.inspiration.effect_id
            && status.origin.map(|origin| origin.source_config_id)
                == Some(self.runtime.inspiration.source_config_id)
        {
            self.observe_inspiration_status(status);
        }
        if status.effect.0 == self.runtime.team_luck.effect_id
            && status
                .origin
                .map(|origin| (origin.source_type_id, origin.source_config_id))
                == Some((
                    self.runtime.team_luck.source_type_id,
                    self.runtime.team_luck.source_config_id,
                ))
        {
            self.observe_team_luck_status(status);
        }
        if status.effect.0 == self.runtime.functional_amp.effect_id
            && status.origin.map(|origin| origin.source_config_id)
                == Some(self.runtime.functional_amp.source_config_id)
        {
            self.observe_functional_amp_status(status);
        }
        if (self.runtime.mechanical_power.runtime_transfer_enabled
            || self.mechanical_power_candidate_audit_enabled)
            && status.effect.0 == self.runtime.mechanical_power.effect_id
            && self.runtime.mechanical_power.source_config_must_be_absent
            && status.origin.is_none()
        {
            // Historical packets do not populate StatusOrigin for this
            // component. Exact ownership comes from StatusEvent.source while
            // source_config_id remains build-scoped static identity evidence.
            self.observe_mechanical_power_status(status);
        }
        if status.effect.0 == self.runtime.harmony_grace.effect_id
            && self.runtime.harmony_grace.matches_source_origin(
                status
                    .origin
                    .map(|origin| (origin.source_type_id, origin.source_config_id)),
            )
        {
            self.observe_harmony_grace_status(status);
        }
        if status.effect.0 == self.runtime.thunderwind.effect_id {
            self.observe_thunderwind_status(status, observed_micros);
        }
        if status.effect.0 == self.runtime.thunderwind.child_effect_id
            && status.origin.map(|origin| origin.source_config_id)
                == Some(self.runtime.thunderwind.child_source_config_id)
        {
            self.observe_thunderwind_child_status(status);
        }
        if status.effect.0 == self.runtime.highland_blood.effect_id {
            self.observe_fatal_spiral_status(status);
        }
        if is_target_vulnerability_effect(status.effect.0) {
            self.observe_target_vulnerability_status(status, observed_micros);
        }
        let Some(rule) = state_rdps_observation_rule()
            .expect("catalog was validated when the projector was built")
        else {
            return;
        };
        if status.effect.0 != rule.effect_id
            || status.origin.map(|origin| origin.source_config_id) != Some(rule.source_config_id)
        {
            return;
        }
        let Some(provider) = status.source else {
            return;
        };
        let key = EffectWindowKey {
            target_actor_id: status.target.actor_id.0,
            provider_actor_id: provider.actor_id.0,
            instance_id: status.instance_id.map(|instance| instance.0),
        };
        let desired_stacks = match status.state {
            StatusState::Removed => 0,
            StatusState::Consumed => match status.stacks {
                Some(stacks) => stacks.min(rule.maximum_stacks),
                None => return,
            },
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                status.stacks.unwrap_or(1).min(rule.maximum_stacks)
            }
        };
        self.effect_windows
            .insert(key, EffectWindow { desired_stacks });
    }

    fn observe_formula_status(&mut self, status: &rlogs_events::StatusEvent) {
        let target_actor_id = status.target.actor_id.0;
        let effect_id = status.effect.0;
        let instance_id = status.instance_id.map(|instance| instance.0);
        match status.state {
            StatusState::Removed | StatusState::Consumed => {
                self.formula_statuses.retain(|key, _| {
                    key.target_actor_id != target_actor_id
                        || key.effect_id != effect_id
                        || instance_id.is_some_and(|instance| key.instance_id != Some(instance))
                });
            }
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                let key = FormulaStatusKey {
                    target_actor_id,
                    source_actor_id: status.source.map(|source| source.actor_id.0).unwrap_or(0),
                    effect_id,
                    instance_id,
                };
                self.formula_statuses.insert(
                    key,
                    FormulaStatusValue {
                        effect_id,
                        stacks: status.stacks.map(i64::from).unwrap_or(-1),
                        level: status.level.map(i64::from).unwrap_or(-1),
                    },
                );
            }
        }
    }

    fn observe_fatal_spiral_status(&mut self, status: &rlogs_events::StatusEvent) {
        let target_actor_id = status.target.actor_id.0;
        let instance_id = status.instance_id.map(|instance| instance.0);
        let active = matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) || (status.state == StatusState::Consumed
            && status.stacks.unwrap_or_default() > 0);

        if active {
            let Some(provider) = status.source else {
                return;
            };
            let provider_actor_id = provider.actor_id.0;
            self.fatal_spiral_windows.insert(FatalSpiralWindowKey {
                target_actor_id,
                target_entity_uuid: status.target.entity_uuid.0,
                provider_actor_id,
                provider_entity_uuid: provider.entity_uuid.0,
                instance_id,
            });
            self.fatal_spiral_transitions
                .entry(target_actor_id)
                .or_default()
                .push(FixedPointFamilyTransition {
                    provider_actor_id,
                    provider_entity_uuid: provider.entity_uuid.0,
                    active: true,
                });
            return;
        }

        let mut removed_providers = self
            .fatal_spiral_windows
            .iter()
            .filter(|key| key.target_actor_id == target_actor_id)
            .filter(|key| instance_id.is_none() || key.instance_id == instance_id)
            .filter(|key| {
                status.source.is_none_or(|provider| {
                    key.provider_actor_id == provider.actor_id.0
                        && (provider.entity_uuid.0 == 0
                            || key.provider_entity_uuid == provider.entity_uuid.0)
                })
            })
            .map(|key| (key.provider_actor_id, key.provider_entity_uuid))
            .collect::<Vec<_>>();
        removed_providers.sort_unstable();
        removed_providers.dedup();
        self.fatal_spiral_windows.retain(|key| {
            !(key.target_actor_id == target_actor_id
                && removed_providers.contains(&(key.provider_actor_id, key.provider_entity_uuid))
                && (instance_id.is_none() || key.instance_id == instance_id))
        });
        self.fatal_spiral_transitions
            .entry(target_actor_id)
            .or_default()
            .extend(removed_providers.into_iter().map(
                |(provider_actor_id, provider_entity_uuid)| FixedPointFamilyTransition {
                    provider_actor_id,
                    provider_entity_uuid,
                    active: false,
                },
            ));
    }

    fn observe_full_bloom_status(&mut self, status: &rlogs_events::StatusEvent) {
        let target_actor_id = status.target.actor_id.0;
        if matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) || (status.state == StatusState::Consumed && status.stacks.unwrap_or_default() > 0)
        {
            self.full_bloom_targets.insert(target_actor_id);
        } else {
            self.full_bloom_targets.remove(&target_actor_id);
        }
    }

    fn observe_inspiration_status(&mut self, status: &rlogs_events::StatusEvent) {
        let target_actor_id = status.target.actor_id.0;
        let instance_id = status.instance_id.map(|instance| instance.0);
        let provider_actor_id = status.source.map(|source| source.actor_id.0);
        let active = matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) || (status.state == StatusState::Consumed
            && status.stacks.unwrap_or_default() > 0);
        if active {
            let Some(provider_actor_id) = provider_actor_id else {
                if let Some(wire) = self.current_wire {
                    self.inspiration_transition_wires
                        .insert(target_actor_id, wire);
                }
                return;
            };
            self.inspiration_windows.insert(
                EffectWindowKey {
                    target_actor_id,
                    provider_actor_id,
                    instance_id,
                },
                InspirationWindow {
                    provider_full_bloom: self.full_bloom_targets.contains(&provider_actor_id),
                },
            );
        } else {
            self.inspiration_windows.retain(|key, _| {
                if key.target_actor_id != target_actor_id {
                    return true;
                }
                if instance_id.is_some() && key.instance_id != instance_id {
                    return true;
                }
                if let Some(provider_actor_id) = provider_actor_id
                    && key.provider_actor_id != provider_actor_id
                {
                    return true;
                }
                false
            });
        }
        if let Some(wire) = self.current_wire {
            self.inspiration_transition_wires
                .insert(target_actor_id, wire);
        }
    }

    fn observe_target_vulnerability_status(
        &mut self,
        status: &rlogs_events::StatusEvent,
        observed_micros: u64,
    ) {
        let target_actor_id = status.target.actor_id.0;
        let effect_id = status.effect.0;
        let instance_id = status.instance_id.map(|instance| instance.0);
        if let Some(provider) = status.source {
            self.target_vulnerability_transitions
                .insert(TargetVulnerabilityTransitionKey {
                    target_actor_id,
                    provider_actor_id: provider.actor_id.0,
                    effect_id,
                });
        }
        match status.state {
            StatusState::Removed | StatusState::Consumed => {
                self.target_vulnerability_windows.retain(|key, _| {
                    key.target_actor_id != target_actor_id
                        || key.effect_id != effect_id
                        || instance_id.is_some_and(|instance| key.instance_id != Some(instance))
                });
            }
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                let Some(provider) = status.source else {
                    return;
                };
                let key = TargetVulnerabilityWindowKey {
                    target_actor_id,
                    provider_actor_id: provider.actor_id.0,
                    effect_id,
                    instance_id,
                };
                let expires_at_observed_micros = status
                    .duration_millis
                    .and_then(|duration| observed_micros.checked_add(duration.checked_mul(1_000)?));
                self.target_vulnerability_windows.insert(
                    key,
                    TargetVulnerabilityWindow {
                        expires_at_observed_micros,
                    },
                );
            }
        }
    }

    fn observe_team_luck_status(&mut self, status: &rlogs_events::StatusEvent) {
        let Some(provider) = status.source else {
            return;
        };
        let key = TeamLuckWindowKey {
            target_actor_id: status.target.actor_id.0,
            target_entity_uuid: status.target.entity_uuid.0,
            provider_actor_id: provider.actor_id.0,
            provider_entity_uuid: provider.entity_uuid.0,
            instance_id: status.instance_id.map(|instance| instance.0),
        };
        let active = match status.state {
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => true,
            StatusState::Removed => false,
            // The canonical state means this exact effect instance was
            // consumed. Remaining stacks, when present, describe the event
            // payload; they do not keep the consumed instance alive.
            StatusState::Consumed => false,
        };
        if active {
            self.team_luck_windows.insert(key);
        } else {
            self.team_luck_windows.remove(&key);
        }
        // Attribute events from the same decoded wire are staged until the
        // following wire. Exclude every selected-effect wire, including a
        // refresh whose membership is unchanged, so damage can never combine
        // the new status payload with the previous attribute state.
        self.team_luck_transition_wire = self.current_wire;
    }

    fn observe_functional_amp_status(&mut self, status: &rlogs_events::StatusEvent) {
        let target_actor_id = status.target.actor_id.0;
        let target_entity_uuid = status.target.entity_uuid.0;
        let instance_id = status.instance_id.map(|instance| instance.0);
        let active = matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) || (status.state == StatusState::Consumed
            && status.stacks.unwrap_or_default() > 0);
        if active {
            let Some(provider) = status.source else {
                if let Some(wire) = self.current_wire {
                    self.functional_amp_transition_wires
                        .insert(target_actor_id, wire);
                }
                return;
            };
            self.functional_amp_windows.insert(FunctionalAmpWindowKey {
                target_actor_id,
                target_entity_uuid,
                provider_actor_id: provider.actor_id.0,
                provider_entity_uuid: provider.entity_uuid.0,
                instance_id,
            });
        } else {
            let provider = status.source;
            self.functional_amp_windows.retain(|key| {
                if key.target_actor_id != target_actor_id
                    || key.target_entity_uuid != target_entity_uuid
                {
                    return true;
                }
                if instance_id.is_some() && key.instance_id != instance_id {
                    return true;
                }
                if let Some(provider) = provider {
                    if key.provider_actor_id != provider.actor_id.0
                        || key.provider_entity_uuid != provider.entity_uuid.0
                    {
                        return true;
                    }
                }
                false
            });
        }
        if let Some(wire) = self.current_wire {
            self.functional_amp_transition_wires
                .insert(target_actor_id, wire);
        }
    }

    fn observe_mechanical_power_status(&mut self, status: &rlogs_events::StatusEvent) {
        let target_actor_id = status.target.actor_id.0;
        let instance_id = status.instance_id.map(|instance| instance.0);
        let active = matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) || (status.state == StatusState::Consumed
            && status.stacks.unwrap_or_default() > 0);
        if active {
            let Some(provider) = status.source else {
                if let Some(wire) = self.current_wire {
                    self.mechanical_power_transition_wires
                        .insert(target_actor_id, wire);
                }
                return;
            };
            let window = EffectWindowKey {
                target_actor_id,
                provider_actor_id: provider.actor_id.0,
                instance_id,
            };
            if self.mechanical_power_windows.insert(window) {
                self.mechanical_power_primary_transition_witnesses
                    .remove(&window);
            }
        } else {
            let provider_actor_id = status.source.map(|source| source.actor_id.0);
            self.mechanical_power_windows.retain(|key| {
                if key.target_actor_id != target_actor_id {
                    return true;
                }
                if instance_id.is_some() && key.instance_id != instance_id {
                    return true;
                }
                if let Some(provider_actor_id) = provider_actor_id
                    && key.provider_actor_id != provider_actor_id
                {
                    return true;
                }
                false
            });
            self.mechanical_power_primary_transition_witnesses
                .retain(|key, _| self.mechanical_power_windows.contains(key));
        }
        // Entity attributes and the status lifecycle can be decoded from the
        // same wire frame with the attribute event first. Reconcile after the
        // lifecycle changes so the exact staged transition can name its
        // provider before the wire is committed.
        self.reconcile_mechanical_power_staged_state(target_actor_id);
        if let Some(wire) = self.current_wire {
            self.mechanical_power_transition_wires
                .insert(target_actor_id, wire);
        }
    }

    fn reconcile_mechanical_power_staged_state(&mut self, target_actor_id: u64) {
        let Some(class_id) = self.recipient_class_id_for_actor(
            target_actor_id,
            &self.runtime.mechanical_power.recipient_rules,
        ) else {
            return;
        };
        let Some(recipient_rule) = self
            .runtime
            .mechanical_power
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == class_id)
            .cloned()
        else {
            return;
        };
        let desired =
            self.desired_mechanical_power_provider_percentages(target_actor_id, &recipient_rule);
        let Some(previous) = self
            .states
            .get(&target_actor_id)
            .and_then(|state| state.mechanical_primary_by_class.get(&class_id))
            .cloned()
        else {
            return;
        };
        let Some(current) = self
            .staged_states
            .get_mut(&target_actor_id)
            .and_then(|state| state.mechanical_primary_by_class.get_mut(&class_id))
        else {
            return;
        };

        debug_assert_eq!(recipient_rule.recipient_class_id, class_id);
        reconcile_external_percent_family(previous.raw_percent, current, &desired);
        reconcile_external_percent_family_from_exact_prior(&previous, current, &desired);
        let current = current.clone();
        self.record_mechanical_power_primary_transition_witness(
            target_actor_id,
            &previous,
            &current,
            &desired,
        );
    }

    fn observe_harmony_grace_status(&mut self, status: &rlogs_events::StatusEvent) {
        let target_actor_id = status.target.actor_id.0;
        let instance_id = status.instance_id.map(|instance| instance.0);
        let provider_actor_id = status.source.map(|source| source.actor_id.0);
        let active = matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        );
        if active {
            let Some(provider) = status.source else {
                if let Some(wire) = self.current_wire {
                    self.harmony_grace_transition_wires
                        .insert(target_actor_id, wire);
                }
                return;
            };
            let window = EffectWindowKey {
                target_actor_id,
                provider_actor_id: provider.actor_id.0,
                instance_id,
            };
            if self.harmony_grace_windows.insert(window) {
                self.harmony_grace_primary_transition_witnesses
                    .remove(&window);
            }
        } else {
            let provider_targets = (status.state == StatusState::Consumed)
                .then(|| {
                    provider_actor_id.map(|provider_actor_id| {
                        self.harmony_grace_windows
                            .iter()
                            .filter(|key| key.provider_actor_id == provider_actor_id)
                            .map(|key| key.target_actor_id)
                            .collect::<HashSet<_>>()
                    })
                })
                .flatten();
            self.harmony_grace_windows.retain(|key| {
                // One Harmony activation is broadcast to the party, and the
                // server fans its Consumed lifecycle out as one row per
                // recipient. Damage for a later recipient can be ordered
                // between those duplicate rows. The first Consumed row is
                // therefore the exact provider-wide closure; Removed remains
                // a target-local lifecycle.
                if status.state == StatusState::Consumed {
                    return provider_actor_id != Some(key.provider_actor_id);
                }
                if key.target_actor_id != target_actor_id {
                    return true;
                }
                if instance_id.is_some() && key.instance_id != instance_id {
                    return true;
                }
                if let Some(provider_actor_id) = provider_actor_id
                    && key.provider_actor_id != provider_actor_id
                {
                    return true;
                }
                false
            });
            if let Some(provider_targets) = provider_targets {
                for provider_target in provider_targets {
                    self.reconcile_harmony_grace_staged_state(provider_target);
                    if let Some(wire) = self.current_wire {
                        self.harmony_grace_transition_wires
                            .insert(provider_target, wire);
                    }
                }
            }
            self.harmony_grace_primary_transition_witnesses
                .retain(|key, _| self.harmony_grace_windows.contains(key));
        }
        // Attribute deltas and their status lifecycle can share a wire frame.
        // The attribute event is decoded first, so its staged state cannot yet
        // name the external provider. Reconcile that exact staged transition
        // after the status window changes instead of waiting for another
        // attribute packet (which may omit this family entirely).
        self.reconcile_harmony_grace_staged_state(target_actor_id);
        if let Some(wire) = self.current_wire {
            self.harmony_grace_transition_wires
                .insert(target_actor_id, wire);
        }
    }

    fn reconcile_harmony_grace_staged_state(&mut self, target_actor_id: u64) {
        let Some(class_id) = self.recipient_class_id_for_actor(
            target_actor_id,
            &self.runtime.harmony_grace.recipient_rules,
        ) else {
            return;
        };
        let Some(recipient_rule) = self
            .runtime
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == class_id)
            .cloned()
        else {
            return;
        };
        let desired =
            self.desired_harmony_grace_provider_percentages(target_actor_id, &recipient_rule);
        let Some(previous) = self
            .states
            .get(&target_actor_id)
            .and_then(|state| state.harmony_primary_by_class.get(&class_id))
            .cloned()
        else {
            return;
        };
        let Some(current) = self
            .staged_states
            .get_mut(&target_actor_id)
            .and_then(|state| state.harmony_primary_by_class.get_mut(&class_id))
        else {
            return;
        };

        debug_assert_eq!(recipient_rule.recipient_class_id, class_id);
        reconcile_external_percent_family(previous.raw_percent, current, &desired);
        reconcile_external_percent_family_from_exact_prior(&previous, current, &desired);
        let current = current.clone();
        self.record_harmony_grace_primary_transition_witness(
            target_actor_id,
            &previous,
            &current,
            &desired,
        );
    }

    fn observe_thunderwind_status(
        &mut self,
        status: &rlogs_events::StatusEvent,
        observed_micros: u64,
    ) {
        let target_actor_id = status.target.actor_id.0;
        let instance_id = status.instance_id.map(|instance| instance.0);
        let provider_actor_id = status.source.map(|source| source.actor_id.0);
        let active = matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) || (status.state == StatusState::Consumed
            && status.stacks.unwrap_or_default() > 0);

        if active {
            let Some(provider_actor_id) = provider_actor_id else {
                self.mark_thunderwind_transition(target_actor_id);
                return;
            };
            let exact_proxy = self.entity_type_by_actor.get(&provider_actor_id).copied()
                == i32::try_from(self.runtime.thunderwind.summon_entity_type_id).ok()
                && self.summon_config_by_actor.get(&provider_actor_id).copied()
                    == Some(self.runtime.thunderwind.summon_config_id)
                && self.actor_ancestry.has_direct_owner_evidence_at(
                    provider_actor_id,
                    observed_micros,
                    ActorOwnershipEvidence::ConfirmedEntityAttributes,
                );
            let source_level = status.level.map(i64::from);
            if !exact_proxy
                || source_level.is_none()
                || self.thunderwind_vector(source_level.unwrap()).is_none()
            {
                self.thunderwind_windows.retain(|key, _| {
                    key.target_actor_id != target_actor_id
                        || key.provider_actor_id != provider_actor_id
                        || (instance_id.is_some() && key.instance_id != instance_id)
                });
                self.clear_thunderwind_provider_state(target_actor_id);
                self.mark_thunderwind_transition(target_actor_id);
                return;
            }
            self.thunderwind_windows.insert(
                EffectWindowKey {
                    target_actor_id,
                    provider_actor_id,
                    instance_id,
                },
                ThunderwindWindow {
                    source_level: source_level.expect("validated above"),
                },
            );
        } else {
            self.thunderwind_windows.retain(|key, _| {
                if key.target_actor_id != target_actor_id {
                    return true;
                }
                if instance_id.is_some() && key.instance_id != instance_id {
                    return true;
                }
                if let Some(provider_actor_id) = provider_actor_id
                    && key.provider_actor_id != provider_actor_id
                {
                    return true;
                }
                false
            });
            self.clear_thunderwind_provider_state(target_actor_id);
        }
        self.mark_thunderwind_transition(target_actor_id);
    }

    fn observe_thunderwind_child_status(&mut self, status: &rlogs_events::StatusEvent) {
        let target_actor_id = status.target.actor_id.0;
        let self_sourced = status.source.is_some_and(|source| source == status.target);
        let active = matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) || (status.state == StatusState::Consumed
            && status.stacks.unwrap_or_default() > 0);
        if active && self_sourced {
            self.thunderwind_child_targets.insert(target_actor_id);
        } else {
            self.thunderwind_child_targets.remove(&target_actor_id);
            self.clear_thunderwind_provider_state(target_actor_id);
        }
        self.mark_thunderwind_transition(target_actor_id);
    }

    fn mark_thunderwind_transition(&mut self, target_actor_id: u64) {
        if let Some(wire) = self.current_wire {
            self.thunderwind_transition_wires
                .insert(target_actor_id, wire);
        }
    }

    fn clear_thunderwind_provider_state(&mut self, target_actor_id: u64) {
        if let Some(state) = self.states.get_mut(&target_actor_id) {
            state.thunderwind_providers.clear();
        }
        if let Some(state) = self.staged_states.get_mut(&target_actor_id) {
            state.thunderwind_providers.clear();
        }
    }

    fn thunderwind_vector(&self, source_level: i64) -> Option<ThunderwindVectorRuntimeConfig> {
        self.runtime
            .thunderwind
            .packet_proven_vectors
            .iter()
            .copied()
            .find(|vector| vector.source_level == source_level)
    }

    fn observe_attributes(
        &mut self,
        actor_id: u64,
        entity_uuid: i64,
        update_kind: EntityAttributeUpdateKind,
        ownership: Option<&rlogs_events::ActorOwnershipUpdate>,
        attributes: &[EntityAttribute],
        observed_micros: u64,
    ) {
        self.actor_ancestry.observe_entity(EntityRef {
            actor_id: rlogs_events::ActorId(actor_id),
            entity_uuid: rlogs_events::EntityUuid(entity_uuid),
        });
        if entity_uuid != 0 {
            if let Some(previous_entity_uuid) = self
                .attribute_state_entity_uuid_by_actor
                .insert(actor_id, entity_uuid)
            {
                if previous_entity_uuid != entity_uuid
                    && self
                        .attribute_state_actor_by_entity_uuid
                        .get(&previous_entity_uuid)
                        == Some(&actor_id)
                {
                    self.attribute_state_actor_by_entity_uuid
                        .remove(&previous_entity_uuid);
                }
            }
            self.attribute_state_actor_by_entity_uuid
                .insert(entity_uuid, actor_id);
        }
        self.observe_canonical_ownership(actor_id, entity_uuid, ownership, observed_micros);
        let mut formula_attributes = if update_kind == EntityAttributeUpdateKind::Snapshot {
            BTreeMap::new()
        } else {
            self.formula_attributes_by_actor
                .get(&actor_id)
                .cloned()
                .unwrap_or_default()
        };
        for attribute in attributes {
            if let Some(value) = integer_attribute(attribute) {
                formula_attributes.insert(attribute.attribute_id, value);
            }
        }
        self.formula_attributes_by_actor
            .insert(actor_id, formula_attributes);
        self.observe_thunderwind_proxy_attributes(actor_id, attributes);
        if update_kind == EntityAttributeUpdateKind::Snapshot {
            self.inspiration_snapshot_targets.insert(actor_id);
            self.fatal_spiral_snapshot_targets.insert(actor_id);
        }
        let rule = state_rdps_observation_rule()
            .expect("catalog was validated when the projector was built");
        let previous_state = self
            .staged_states
            .get(&actor_id)
            .cloned()
            .or_else(|| self.states.get(&actor_id).cloned())
            .unwrap_or_default();
        let snapshot_has_critical_damage = attributes.iter().any(|attribute| {
            attribute.attribute_id == self.runtime.team_luck.critical_damage_attribute_id
        });
        let snapshot_has_lucky_damage = attributes.iter().any(|attribute| {
            attribute.attribute_id == self.runtime.team_luck.lucky_damage_attribute_id
        });
        let mut next = if update_kind == EntityAttributeUpdateKind::Snapshot {
            // A snapshot is authoritative. Carrying omitted values forward
            // would combine two different character states and can silently
            // manufacture rDPS. Provider decomposition is deliberately reset
            // until a later packet transition proves it again.
            ActorHpState::default()
        } else {
            previous_state.clone()
        };
        let previous_percent = next.raw_percent;
        let previous_physical_attack_percent = next.physical_attack.raw_percent;
        let previous_magical_attack_percent = next.magical_attack.raw_percent;
        let previous_harmony_primary = next.harmony_primary_by_class.clone();
        let previous_mechanical_primary = next.mechanical_primary_by_class.clone();
        let harmony_actor_class_id = self
            .recipient_class_id_for_actor(actor_id, &self.runtime.harmony_grace.recipient_rules);
        let mechanical_actor_class_id = self
            .recipient_class_id_for_actor(actor_id, &self.runtime.mechanical_power.recipient_rules);
        for attribute in attributes {
            let Some(value) = integer_attribute(attribute) else {
                continue;
            };
            for recipient_rule in &self.runtime.mechanical_power.recipient_rules {
                update_attack_family_attribute(
                    next.mechanical_primary_by_class
                        .entry(recipient_rule.recipient_class_id)
                        .or_default(),
                    recipient_rule.primary_attribute_family,
                    attribute.attribute_id,
                    value,
                );
            }
            for recipient_rule in &self.runtime.harmony_grace.recipient_rules {
                update_attack_family_attribute(
                    next.harmony_primary_by_class
                        .entry(recipient_rule.recipient_class_id)
                        .or_default(),
                    recipient_rule.primary_attribute_family,
                    attribute.attribute_id,
                    value,
                );
            }
            if let Some(index) = self
                .runtime
                .inspiration
                .primary_raw_add_attribute_ids
                .iter()
                .position(|attribute_id| *attribute_id == attribute.attribute_id)
            {
                next.primary_raw_add[index] = Some(value);
            }
            if attribute.attribute_id == ATTR_CURRENT_HP {
                next.current_value = Some(value);
            } else if rule.is_some_and(|rule| attribute.attribute_id == rule.base_attribute_id) {
                next.base_value = Some(value);
            } else if rule.is_some_and(|rule| attribute.attribute_id == rule.final_attribute_id) {
                next.final_value = Some(value);
            } else if attribute.attribute_id == ATTR_MAX_HP_EXTRA_ADD {
                next.extra_add = Some(value);
            } else if rule
                .is_some_and(|rule| attribute.attribute_id == rule.percentage_attribute_id)
            {
                next.raw_percent = Some(value);
            } else if rule
                .is_some_and(|rule| attribute.attribute_id == rule.intermediate_attribute_id)
            {
                next.intermediate_value = Some(value);
            } else if rule
                .is_some_and(|rule| attribute.attribute_id == rule.extra_percentage_attribute_id)
            {
                next.raw_extra_percent = Some(value);
            } else if attribute.attribute_id == self.runtime.team_luck.critical_damage_attribute_id
            {
                next.critical_damage_raw = Some(value);
            } else if attribute.attribute_id == self.runtime.team_luck.lucky_damage_attribute_id {
                next.lucky_damage_raw = Some(value);
            } else if attribute.attribute_id
                == self.runtime.thunderwind.critical_chance_attribute_id
            {
                next.critical_chance_raw = Some(value);
            } else if attribute.attribute_id
                == self.runtime.thunderwind.critical_damage_attribute_id
            {
                next.critical_damage_raw = Some(value);
            } else if attribute.attribute_id
                == self.runtime.attack_families.physical.final_attribute_id
            {
                next.physical_attack.final_value = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .attack_families
                    .physical
                    .intermediate_attribute_id
            {
                next.physical_attack.intermediate_value = Some(value);
            } else if attribute.attribute_id
                == self.runtime.attack_families.physical.base_add_attribute_id
            {
                next.physical_attack.base_add = Some(value);
            } else if attribute.attribute_id
                == self.runtime.attack_families.physical.extra_add_attribute_id
            {
                next.physical_attack.extra_add = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .attack_families
                    .physical
                    .raw_percent_attribute_id
            {
                next.physical_attack.raw_percent = Some(value);
                next.physical_attack.raw_percent_packet_observed = true;
            } else if attribute.attribute_id
                == self.runtime.attack_families.magical.final_attribute_id
            {
                next.magical_attack.final_value = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .attack_families
                    .magical
                    .intermediate_attribute_id
            {
                next.magical_attack.intermediate_value = Some(value);
            } else if attribute.attribute_id
                == self.runtime.attack_families.magical.base_add_attribute_id
            {
                next.magical_attack.base_add = Some(value);
            } else if attribute.attribute_id
                == self.runtime.attack_families.magical.extra_add_attribute_id
            {
                next.magical_attack.extra_add = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .attack_families
                    .magical
                    .raw_percent_attribute_id
            {
                next.magical_attack.raw_percent = Some(value);
                next.magical_attack.raw_percent_packet_observed = true;
            } else if attribute.attribute_id
                == self.runtime.inspiration.critical_chance_attribute_id
            {
                next.critical_chance_raw = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .inspiration
                    .critical_chance_raw_add_attribute_id
            {
                next.critical_chance_raw_add = Some(value);
            } else if attribute.attribute_id == self.runtime.inspiration.lucky_chance_attribute_id {
                next.lucky_chance_raw = Some(value);
            } else if attribute.attribute_id
                == self.runtime.inspiration.lucky_chance_raw_add_attribute_id
            {
                next.lucky_chance_raw_add = Some(value);
            } else if attribute.attribute_id == self.runtime.inspiration.mastery_attribute_id {
                next.mastery_raw = Some(value);
            } else if attribute.attribute_id
                == self.runtime.inspiration.mastery_raw_add_attribute_id
            {
                next.mastery_raw_add = Some(value);
            } else if attribute.attribute_id == self.runtime.inspiration.versatility_attribute_id {
                next.versatility_raw = Some(value);
            } else if attribute.attribute_id
                == self.runtime.inspiration.versatility_raw_add_attribute_id
            {
                next.versatility_raw_add = Some(value);
            } else if attribute.attribute_id
                == self.runtime.inspiration.external_damage_attribute_id
            {
                next.external_damage_raw = Some(value);
            } else if attribute.attribute_id
                == self.runtime.inspiration.property_damage_attribute_id
            {
                next.property_damage_raw = Some(value);
            } else if attribute.attribute_id == self.runtime.inspiration.haste_attribute_id {
                next.haste_percent_basis_points = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .highland_blood
                    .all_element_family
                    .current_attribute_id
            {
                next.all_element.current_value = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .highland_blood
                    .all_element_family
                    .total_attribute_id
            {
                next.all_element.total_value = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .highland_blood
                    .all_element_family
                    .add_attribute_id
            {
                next.all_element.add_value = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .highland_blood
                    .all_element_family
                    .extra_add_attribute_id
            {
                next.all_element.extra_add_value = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .highland_blood
                    .all_element_family
                    .percent_attribute_id
            {
                next.all_element.percent_value = Some(value);
            } else if attribute.attribute_id
                == self
                    .runtime
                    .highland_blood
                    .all_element_family
                    .extra_percent_attribute_id
            {
                next.all_element.extra_percent_value = Some(value);
            }
        }

        if next.critical_damage_raw.is_some() {
            self.team_luck_critical_ever_observed.insert(actor_id);
            self.team_luck_critical_cleared_by_snapshot
                .remove(&actor_id);
        } else if update_kind == EntityAttributeUpdateKind::Snapshot
            && !snapshot_has_critical_damage
            && self.team_luck_critical_ever_observed.contains(&actor_id)
        {
            self.team_luck_critical_cleared_by_snapshot.insert(actor_id);
        }
        if next.lucky_damage_raw.is_some() {
            self.team_luck_lucky_ever_observed.insert(actor_id);
            self.team_luck_lucky_cleared_by_snapshot.remove(&actor_id);
        } else if update_kind == EntityAttributeUpdateKind::Snapshot
            && !snapshot_has_lucky_damage
            && self.team_luck_lucky_ever_observed.contains(&actor_id)
        {
            self.team_luck_lucky_cleared_by_snapshot.insert(actor_id);
        }

        if let Some(rule) = rule
            && let (Some(previous), Some(current)) = (previous_percent, next.raw_percent)
            && previous != current
        {
            let desired = self.desired_provider_percentages(rule, actor_id);
            let current_total = next.provider_raw_percent.values().copied().sum::<i64>();
            let desired_total = desired.values().copied().sum::<i64>();
            if desired_total.checked_sub(current_total) == current.checked_sub(previous) {
                next.provider_raw_percent = desired;
            } else {
                // The attribute changed, but the packet-observed provider
                // windows do not explain it exactly. Keep every event and stop
                // attribution until a later exact transition re-establishes
                // the provider decomposition.
                next.provider_raw_percent.clear();
            }
        }
        let desired_functional_amp = self
            .desired_functional_amp_provider_percentages(actor_id, entity_uuid)
            .into_iter()
            .filter_map(|((provider_actor_id, provider_entity_uuid), value)| {
                (self.actor_ancestry.actor_for_entity(provider_entity_uuid)
                    == Some(provider_actor_id))
                .then_some((provider_actor_id, value))
            })
            .collect::<BTreeMap<_, _>>();
        complete_exact_raw_percent(&mut next.physical_attack);
        complete_exact_raw_percent(&mut next.magical_attack);
        complete_exact_extra_add(&mut next.physical_attack);
        complete_exact_extra_add(&mut next.magical_attack);
        for family in next.harmony_primary_by_class.values_mut() {
            complete_exact_raw_percent(family);
            complete_exact_extra_add(family);
        }
        for family in next.mechanical_primary_by_class.values_mut() {
            complete_exact_raw_percent(family);
            complete_exact_extra_add(family);
        }
        reconcile_external_percent_family(
            previous_physical_attack_percent,
            &mut next.physical_attack,
            &desired_functional_amp,
        );
        reconcile_external_percent_family(
            previous_magical_attack_percent,
            &mut next.magical_attack,
            &desired_functional_amp,
        );
        let mut harmony_transition_candidates = Vec::new();
        for recipient_rule in &self.runtime.harmony_grace.recipient_rules {
            let desired = if harmony_actor_class_id == Some(recipient_rule.recipient_class_id) {
                self.desired_harmony_grace_provider_percentages(actor_id, recipient_rule)
            } else {
                BTreeMap::new()
            };
            let family = next
                .harmony_primary_by_class
                .entry(recipient_rule.recipient_class_id)
                .or_default();
            let previous = previous_harmony_primary
                .get(&recipient_rule.recipient_class_id)
                .cloned()
                .unwrap_or_default();
            reconcile_external_percent_family(previous.raw_percent, family, &desired);
            reconcile_external_percent_family_from_exact_prior(&previous, family, &desired);
            harmony_transition_candidates.push((previous, family.clone(), desired));
        }
        for (previous, current, desired) in harmony_transition_candidates {
            self.record_harmony_grace_primary_transition_witness(
                actor_id, &previous, &current, &desired,
            );
        }
        let mut mechanical_transition_candidates = Vec::new();
        for recipient_rule in &self.runtime.mechanical_power.recipient_rules {
            let desired = if mechanical_actor_class_id == Some(recipient_rule.recipient_class_id) {
                self.desired_mechanical_power_provider_percentages(actor_id, recipient_rule)
            } else {
                BTreeMap::new()
            };
            let family = next
                .mechanical_primary_by_class
                .entry(recipient_rule.recipient_class_id)
                .or_default();
            let previous = previous_mechanical_primary
                .get(&recipient_rule.recipient_class_id)
                .cloned()
                .unwrap_or_default();
            reconcile_external_percent_family(previous.raw_percent, family, &desired);
            reconcile_external_percent_family_from_exact_prior(&previous, family, &desired);
            mechanical_transition_candidates.push((previous, family.clone(), desired));
        }
        for (previous, current, desired) in mechanical_transition_candidates {
            self.record_mechanical_power_primary_transition_witness(
                actor_id, &previous, &current, &desired,
            );
        }
        self.staged_states.insert(actor_id, next);
    }

    fn recipient_class_id_for_actor(
        &self,
        actor_id: u64,
        recipient_rules: &[PrimaryStatRecipientRule],
    ) -> Option<i32> {
        self.class_id_by_actor
            .get(&actor_id)
            .copied()
            .or_else(|| {
                let observed_abilities = self.observed_ability_ids_by_actor.get(&actor_id)?;
                specialization_identity_from_observed_abilities(observed_abilities.iter().copied())
                    .ok()
                    .flatten()
                    .map(|(class_id, _)| class_id)
            })
            .or_else(|| {
                // A build-specific runtime with exactly one recipient rule has
                // already constrained the legal route. This preserves the
                // sealed legacy proof when its attribute snapshot precedes
                // actor identity, without guessing on current multi-class
                // builds.
                (recipient_rules.len() == 1).then_some(recipient_rules[0].recipient_class_id)
            })
    }

    fn observe_thunderwind_proxy_attributes(
        &mut self,
        actor_id: u64,
        attributes: &[EntityAttribute],
    ) {
        if self.entity_type_by_actor.get(&actor_id).copied()
            != i32::try_from(self.runtime.thunderwind.summon_entity_type_id).ok()
        {
            return;
        }
        let config = attributes
            .iter()
            .find(|attribute| {
                attribute.attribute_id == self.runtime.thunderwind.summon_config_attribute_id
            })
            .and_then(integer_attribute);
        if let Some(config) = config {
            if config == self.runtime.thunderwind.summon_config_id {
                self.summon_config_by_actor.insert(actor_id, config);
            } else {
                self.summon_config_by_actor.remove(&actor_id);
            }
        }
    }

    fn observe_canonical_ownership(
        &mut self,
        actor_id: u64,
        direct_entity_uuid: i64,
        ownership: Option<&rlogs_events::ActorOwnershipUpdate>,
        observed_micros: u64,
    ) {
        match ownership {
            Some(rlogs_events::ActorOwnershipUpdate::Confirmed { owner_entity_uuid }) => {
                self.actor_ancestry.observe_owner_entity(
                    observed_micros,
                    EntityRef {
                        actor_id: rlogs_events::ActorId(actor_id),
                        entity_uuid: rlogs_events::EntityUuid(direct_entity_uuid),
                    },
                    owner_entity_uuid.0,
                    ActorOwnershipEvidence::ConfirmedEntityAttributes,
                );
            }
            Some(rlogs_events::ActorOwnershipUpdate::Cleared) => {
                self.actor_ancestry.clear_owner(
                    observed_micros,
                    EntityRef {
                        actor_id: rlogs_events::ActorId(actor_id),
                        entity_uuid: rlogs_events::EntityUuid(direct_entity_uuid),
                    },
                );
            }
            None => {}
        }
    }

    fn desired_provider_percentages(
        &self,
        rule: &StateRdpsRule,
        target_actor_id: u64,
    ) -> BTreeMap<u64, i64> {
        let mut desired = BTreeMap::<u64, i64>::new();
        for (key, window) in &self.effect_windows {
            if key.target_actor_id != target_actor_id || window.desired_stacks == 0 {
                continue;
            }
            let raw = i64::from(window.desired_stacks).saturating_mul(rule.raw_percent_per_stack);
            desired
                .entry(key.provider_actor_id)
                .and_modify(|value| *value = (*value).max(raw))
                .or_insert(raw);
        }
        desired
    }

    fn desired_functional_amp_provider_percentages(
        &self,
        target_actor_id: u64,
        target_entity_uuid: i64,
    ) -> BTreeMap<(u64, i64), i64> {
        self.functional_amp_windows
            .iter()
            .filter(|key| {
                key.target_actor_id == target_actor_id
                    && key.target_entity_uuid == target_entity_uuid
            })
            .fold(BTreeMap::new(), |mut desired, key| {
                desired
                    .entry((key.provider_actor_id, key.provider_entity_uuid))
                    .and_modify(|value| {
                        *value = (*value).max(self.runtime.functional_amp.attack_percent_raw_delta)
                    })
                    .or_insert(self.runtime.functional_amp.attack_percent_raw_delta);
                desired
            })
    }

    fn desired_harmony_grace_provider_percentages(
        &self,
        target_actor_id: u64,
        recipient_rule: &PrimaryStatRecipientRule,
    ) -> BTreeMap<u64, i64> {
        self.harmony_grace_windows
            .iter()
            .filter(|key| key.target_actor_id == target_actor_id)
            .fold(BTreeMap::new(), |mut desired, key| {
                desired
                    .entry(key.provider_actor_id)
                    .and_modify(|value| {
                        *value = (*value).max(recipient_rule.primary_percent_raw_delta)
                    })
                    .or_insert(recipient_rule.primary_percent_raw_delta);
                desired
            })
    }

    fn record_harmony_grace_primary_transition_witness(
        &mut self,
        target_actor_id: u64,
        previous: &AttackFamilyState,
        current: &AttackFamilyState,
        desired: &BTreeMap<u64, i64>,
    ) {
        let Some((provider_actor_id, witness)) =
            exact_primary_stat_transition_witness(self.current_wire, previous, current, desired)
        else {
            return;
        };
        let mut matching = self.harmony_grace_windows.iter().copied().filter(|key| {
            key.target_actor_id == target_actor_id && key.provider_actor_id == provider_actor_id
        });
        let Some(window) = matching.next() else {
            return;
        };
        if matching.next().is_some() {
            // Same-provider overlapping instances have unresolved stacking and
            // removal arbitration. Do not attach one transition to either.
            return;
        }
        self.harmony_grace_primary_transition_witnesses
            .entry(window)
            .or_default()
            .insert(PrimaryStatTransitionWitness {
                instance_id: window.instance_id,
                ..witness
            });
    }

    fn harmony_grace_primary_transition_witness(
        &self,
        recipient_actor_id: u64,
        provider_actor_id: u64,
        primary: &AttackFamilyState,
        provider_raw_percent: i64,
    ) -> Result<PrimaryStatTransitionWitness, &'static str> {
        let mut matching = self.harmony_grace_windows.iter().copied().filter(|key| {
            key.target_actor_id == recipient_actor_id && key.provider_actor_id == provider_actor_id
        });
        let window = matching.next().ok_or("provider_lifecycle_missing")?;
        if matching.next().is_some() {
            return Err("provider_lifecycle_ambiguous");
        }
        let base_add = primary.base_add.ok_or("primary_family_incomplete")?;
        let raw_percent = primary.raw_percent.ok_or("primary_family_incomplete")?;
        self.harmony_grace_primary_transition_witnesses
            .get(&window)
            .and_then(|witnesses| {
                witnesses.iter().copied().find(|witness| {
                    witness.base_add == base_add
                        && witness.active_raw_percent == raw_percent
                        && witness.provider_raw_percent == provider_raw_percent
                })
            })
            .ok_or("primary_transition_witness_missing")
    }

    fn desired_mechanical_power_provider_percentages(
        &self,
        target_actor_id: u64,
        recipient_rule: &PrimaryStatRecipientRule,
    ) -> BTreeMap<u64, i64> {
        let primary_percent_raw_delta = self
            .mechanical_power_candidate_primary_percent_override
            .or_else(|| {
                self.runtime
                    .mechanical_power
                    .production_primary_percent_raw_delta(recipient_rule.recipient_class_id)
            })
            .unwrap_or(recipient_rule.primary_percent_raw_delta);
        self.mechanical_power_windows
            .iter()
            .filter(|key| key.target_actor_id == target_actor_id)
            .fold(BTreeMap::new(), |mut desired, key| {
                desired
                    .entry(key.provider_actor_id)
                    .and_modify(|value| *value = (*value).max(primary_percent_raw_delta))
                    .or_insert(primary_percent_raw_delta);
                desired
            })
    }

    fn mechanical_power_exact_scoped_rebase_allowed(&self, provider_raw_percent: i64) -> bool {
        self.mechanical_power_candidate_primary_percent_override
            .is_some_and(|candidate| candidate == provider_raw_percent)
            || (self.runtime.mechanical_power.runtime_transfer_enabled
                && self
                    .runtime
                    .mechanical_power
                    .runtime_primary_percent_raw_deltas
                    .contains(&provider_raw_percent))
    }

    fn record_mechanical_power_primary_transition_witness(
        &mut self,
        target_actor_id: u64,
        previous: &AttackFamilyState,
        current: &AttackFamilyState,
        desired: &BTreeMap<u64, i64>,
    ) {
        let Some((provider_actor_id, witness)) =
            exact_primary_stat_transition_witness(self.current_wire, previous, current, desired)
        else {
            return;
        };
        let mut matching = self.mechanical_power_windows.iter().copied().filter(|key| {
            key.target_actor_id == target_actor_id && key.provider_actor_id == provider_actor_id
        });
        let Some(window) = matching.next() else {
            return;
        };
        if matching.next().is_some() {
            // Same-provider overlapping instances have unresolved stacking and
            // removal arbitration. Do not attach one transition to either.
            return;
        }
        self.mechanical_power_primary_transition_witnesses
            .entry(window)
            .or_default()
            .insert(PrimaryStatTransitionWitness {
                instance_id: window.instance_id,
                ..witness
            });
    }

    fn mechanical_power_primary_transition_witness(
        &self,
        recipient_actor_id: u64,
        provider_actor_id: u64,
        primary: &AttackFamilyState,
        provider_raw_percent: i64,
    ) -> Result<PrimaryStatTransitionWitness, &'static str> {
        let mut matching = self.mechanical_power_windows.iter().copied().filter(|key| {
            key.target_actor_id == recipient_actor_id && key.provider_actor_id == provider_actor_id
        });
        let window = matching.next().ok_or("provider_lifecycle_missing")?;
        if matching.next().is_some() {
            return Err("provider_lifecycle_ambiguous");
        }
        let base_add = primary.base_add.ok_or("primary_family_incomplete")?;
        let raw_percent = primary.raw_percent.ok_or("primary_family_incomplete")?;
        let exact_scoped_rebase_allowed =
            self.mechanical_power_exact_scoped_rebase_allowed(provider_raw_percent);
        let witness = self
            .mechanical_power_primary_transition_witnesses
            .get(&window)
            .and_then(|witnesses| {
                witnesses.iter().copied().find(|witness| {
                    witness.provider_raw_percent == provider_raw_percent
                        && (exact_scoped_rebase_allowed
                            || (witness.base_add == base_add
                                && witness.active_raw_percent == raw_percent))
                })
            })
            .ok_or("primary_transition_witness_missing")?;
        if exact_scoped_rebase_allowed {
            rebase_primary_stat_transition_witness(primary, witness)
                .ok_or("primary_transition_rebase_unproven")
        } else {
            Ok(witness)
        }
    }

    fn desired_inspiration_provider_modes(
        &self,
        target_actor_id: u64,
    ) -> Option<BTreeMap<u64, bool>> {
        let mut desired = BTreeMap::new();
        for (key, window) in &self.inspiration_windows {
            if key.target_actor_id != target_actor_id {
                continue;
            }
            if let Some(existing) =
                desired.insert(key.provider_actor_id, window.provider_full_bloom)
                && existing != window.provider_full_bloom
            {
                // Two active instances disagree about the provider snapshot.
                // Retain both lifecycles but do not manufacture one magnitude.
                return None;
            }
        }
        Some(desired)
    }

    fn reconcile_inspiration_staged_states(&mut self) {
        let actor_ids = self.staged_states.keys().copied().collect::<Vec<_>>();
        for actor_id in actor_ids {
            let desired = self.desired_inspiration_provider_modes(actor_id);
            let previous = self.states.get(&actor_id).cloned();
            let Some(next) = self.staged_states.get_mut(&actor_id) else {
                continue;
            };
            if self.inspiration_snapshot_targets.contains(&actor_id) {
                next.inspiration_providers.clear();
                continue;
            }
            let Some(desired) = desired else {
                next.inspiration_providers.clear();
                continue;
            };
            reconcile_inspiration_state(
                previous.as_ref(),
                next,
                &desired,
                &self.runtime.inspiration.packet_proven_vectors,
            );
        }
    }

    fn reconcile_thunderwind_staged_states(&mut self) {
        let actor_ids = self.staged_states.keys().copied().collect::<Vec<_>>();
        for actor_id in actor_ids {
            let desired = self.desired_thunderwind_provider(actor_id);
            let previous = self.states.get(&actor_id).cloned();
            let Some(next) = self.staged_states.get_mut(&actor_id) else {
                continue;
            };
            let Some((provider_actor_id, vector)) = desired else {
                next.thunderwind_providers.clear();
                continue;
            };
            let Some(previous) = previous else {
                next.thunderwind_providers.clear();
                continue;
            };

            if previous.thunderwind_providers.len() == 1
                && previous.thunderwind_providers.get(&provider_actor_id)
                    == Some(&ThunderwindProviderState {
                        critical_chance_raw_delta: vector.critical_chance_raw_delta,
                        critical_damage_raw_delta: vector.critical_damage_raw_delta,
                    })
            {
                if option_delta(previous.critical_chance_raw, next.critical_chance_raw) == Some(0)
                    && option_delta(previous.critical_damage_raw, next.critical_damage_raw)
                        == Some(0)
                {
                    next.thunderwind_providers
                        .clone_from(&previous.thunderwind_providers);
                } else {
                    next.thunderwind_providers.clear();
                }
                continue;
            }

            next.thunderwind_providers.clear();
            if previous.thunderwind_providers.is_empty()
                && option_delta(previous.critical_chance_raw, next.critical_chance_raw)
                    == Some(vector.critical_chance_raw_delta)
                && option_delta(previous.critical_damage_raw, next.critical_damage_raw)
                    == Some(vector.critical_damage_raw_delta)
            {
                next.thunderwind_providers.insert(
                    provider_actor_id,
                    ThunderwindProviderState {
                        critical_chance_raw_delta: vector.critical_chance_raw_delta,
                        critical_damage_raw_delta: vector.critical_damage_raw_delta,
                    },
                );
            }
        }
    }

    fn reconcile_fatal_spiral_staged_states(&mut self) {
        let actor_ids = self.staged_states.keys().copied().collect::<Vec<_>>();
        for actor_id in actor_ids {
            let desired_provider = self.desired_fatal_spiral_provider(actor_id);
            let transitions = self
                .fatal_spiral_transitions
                .get(&actor_id)
                .into_iter()
                .flatten()
                .map(|transition| FixedPointFamilyTransition {
                    provider_actor_id: self.resolve_owner_actor_id(transition.provider_actor_id),
                    provider_entity_uuid: transition.provider_entity_uuid,
                    active: transition.active,
                })
                .collect::<Vec<_>>();
            let previous = self.states.get(&actor_id).cloned();
            let Some(staged) = self.staged_states.get(&actor_id).cloned() else {
                continue;
            };

            if self.fatal_spiral_snapshot_targets.contains(&actor_id) {
                if let Some(next) = self.staged_states.get_mut(&actor_id) {
                    next.all_element.provider_basis_points.clear();
                }
                continue;
            }

            let deltas = previous.as_ref().and_then(|previous| {
                fixed_point_family_component_deltas(&previous.all_element, &staged.all_element)
            });

            // The first local snapshot can arrive after the buff was applied,
            // so application has no baseline. Removal still supplies the full
            // inverse transition and therefore proves the provider magnitude.
            if let Some([current, total, add, extra_add, percent, extra_percent]) = deltas {
                let mut inactive = transitions
                    .iter()
                    .filter(|transition| !transition.active)
                    .filter(|transition| transition.provider_actor_id != actor_id)
                    .map(|transition| {
                        (
                            transition.provider_actor_id,
                            transition.provider_entity_uuid,
                        )
                    })
                    .collect::<Vec<_>>();
                inactive.sort_unstable();
                inactive.dedup();
                let has_active_transition = transitions.iter().any(|transition| transition.active);
                let removed_basis_points = current.checked_neg();
                if !has_active_transition
                    && inactive.len() == 1
                    && current == total
                    && total == add
                    && extra_add == 0
                    && percent == 0
                    && extra_percent == 0
                    && removed_basis_points.is_some_and(|value| {
                        self.runtime
                            .highland_blood
                            .packet_proven_raw_deltas
                            .contains(&value)
                    })
                {
                    let (_, provider_entity_uuid) = inactive[0];
                    self.learn_fatal_spiral_provider_basis_points(
                        provider_entity_uuid,
                        removed_basis_points.expect("checked above"),
                    );
                }
            }

            let mut provider_basis_points = BTreeMap::new();
            if let Some((provider_actor_id, provider_entity_uuid)) = desired_provider {
                let preserved = previous.as_ref().is_some_and(|previous| {
                    previous.all_element.provider_basis_points.len() == 1
                        && previous
                            .all_element
                            .provider_basis_points
                            .contains_key(&provider_actor_id)
                        && transitions.is_empty()
                        && deltas == Some([0; 6])
                });
                if preserved {
                    provider_basis_points.clone_from(
                        &previous
                            .as_ref()
                            .expect("preservation requires previous state")
                            .all_element
                            .provider_basis_points,
                    );
                } else if let Some([current, total, add, extra_add, percent, extra_percent]) =
                    deltas
                {
                    let active_transitions = transitions
                        .iter()
                        .filter(|transition| {
                            transition.active
                                && transition.provider_actor_id == provider_actor_id
                                && transition.provider_entity_uuid == provider_entity_uuid
                        })
                        .count();
                    let inactive_transitions = transitions
                        .iter()
                        .filter(|transition| !transition.active)
                        .count();
                    let exact_application = previous.as_ref().is_some_and(|previous| {
                        previous.all_element.provider_basis_points.is_empty()
                    }) && active_transitions == 1
                        && inactive_transitions == 0
                        && current == total
                        && total == add
                        && extra_add == 0
                        && percent == 0
                        && extra_percent == 0
                        && self
                            .runtime
                            .highland_blood
                            .packet_proven_raw_deltas
                            .contains(&current);
                    if exact_application {
                        self.learn_fatal_spiral_provider_basis_points(
                            provider_entity_uuid,
                            current,
                        );
                        provider_basis_points.insert(provider_actor_id, current);
                    }
                }

                if provider_basis_points.is_empty()
                    && !self
                        .fatal_spiral_ambiguous_provider_entities
                        .contains(&provider_entity_uuid)
                {
                    if let Some(&basis_points) = self
                        .fatal_spiral_provider_basis_points_by_entity_uuid
                        .get(&provider_entity_uuid)
                    {
                        provider_basis_points.insert(provider_actor_id, basis_points);
                    }
                }
            }

            if let Some(next) = self.staged_states.get_mut(&actor_id) {
                next.all_element.provider_basis_points = provider_basis_points;
            }
        }
    }

    fn desired_fatal_spiral_provider(&self, target_actor_id: u64) -> Option<(u64, i64)> {
        let mut providers = self
            .fatal_spiral_windows
            .iter()
            .filter(|key| key.target_actor_id == target_actor_id)
            .map(|key| {
                (
                    self.resolve_owner_actor_id(key.provider_actor_id),
                    key.provider_entity_uuid,
                )
            })
            .filter(|(provider_actor_id, _)| *provider_actor_id != target_actor_id)
            .filter(|(provider_actor_id, _)| self.active_players.contains(provider_actor_id))
            .collect::<Vec<_>>();
        providers.sort_unstable();
        providers.dedup();
        (providers.len() == 1).then(|| providers[0])
    }

    fn learn_fatal_spiral_provider_basis_points(
        &mut self,
        provider_entity_uuid: i64,
        basis_points: i64,
    ) {
        if provider_entity_uuid == 0
            || self
                .fatal_spiral_ambiguous_provider_entities
                .contains(&provider_entity_uuid)
        {
            return;
        }
        match self
            .fatal_spiral_provider_basis_points_by_entity_uuid
            .get(&provider_entity_uuid)
            .copied()
        {
            None => {
                self.fatal_spiral_provider_basis_points_by_entity_uuid
                    .insert(provider_entity_uuid, basis_points);
            }
            Some(previous) if previous == basis_points => {}
            Some(_) => {
                self.fatal_spiral_provider_basis_points_by_entity_uuid
                    .remove(&provider_entity_uuid);
                self.fatal_spiral_ambiguous_provider_entities
                    .insert(provider_entity_uuid);
            }
        }
    }

    fn desired_thunderwind_provider(
        &self,
        target_actor_id: u64,
    ) -> Option<(u64, ThunderwindVectorRuntimeConfig)> {
        if !self.thunderwind_child_targets.contains(&target_actor_id) {
            return None;
        }
        let mut desired = self
            .thunderwind_windows
            .iter()
            .filter(|(key, _)| key.target_actor_id == target_actor_id)
            .filter_map(|(key, window)| {
                let owner_actor_id = self
                    .actor_ancestry
                    .direct_owner_at(key.provider_actor_id, self.latest_observed_micros)?
                    .actor_id
                    .0;
                let vector = self.thunderwind_vector(window.source_level)?;
                Some((owner_actor_id, vector))
            })
            .collect::<Vec<_>>();
        desired.sort_unstable_by_key(|(provider, vector)| (*provider, vector.source_level));
        desired.dedup();
        (desired.len() == 1).then(|| desired[0])
    }

    fn exact_damage_marginal(
        &self,
        rule: &StateRdpsRule,
        recipient_actor_id: u64,
        observed_damage: i64,
    ) -> Option<(u64, i64)> {
        let state = self.states.get(&recipient_actor_id)?;
        let mut external = state.provider_raw_percent.iter().filter(|(provider, raw)| {
            **provider != recipient_actor_id && **raw > 0 && self.active_players.contains(provider)
        });
        let (&provider_actor_id, &provider_raw_percent) = external.next()?;
        if external.next().is_some() {
            // Multi-provider rounding allocation has not been proven for this
            // rule. Preserve the damage and emit no guessed transfer.
            return None;
        }
        if !rule
            .enabled_provider_raw_percent_values
            .contains(&provider_raw_percent)
        {
            // Every event remains canonical, but only provider totals whose
            // integer marginal has been proved exactly are projected.
            return None;
        }
        let final_value = state.final_value?;
        let base_value = state.base_value?;
        let raw_percent = state.raw_percent?;
        let intermediate_value = state.intermediate_value?;
        let raw_extra_percent = state.raw_extra_percent?;

        let inferred_state = observed_damage
            .checked_sub(rule.constant_offset)?
            .checked_div(rule.state_multiplier)?;
        if inferred_state
            .checked_mul(rule.state_multiplier)?
            .checked_add(rule.constant_offset)?
            != observed_damage
        {
            return None;
        }
        if final_value != inferred_state {
            return None;
        }

        let marginal_state = two_stage_percent_input_marginal(
            base_value,
            raw_percent,
            provider_raw_percent,
            intermediate_value,
            raw_extra_percent,
        )?;
        let amount = linear_state_scaled_damage_marginal(rule.state_multiplier, marginal_state)?;
        (amount > 0 && amount <= observed_damage).then_some((provider_actor_id, amount))
    }

    fn team_luck_contribution(
        &self,
        envelope: &EventEnvelope,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<ExactRationalDamageContributionEvent> {
        self.team_luck_decision(envelope.time.observed_micros, damage)
            .ok()
    }

    pub fn team_luck_audit_gate(&self, damage: &rlogs_events::DamageEvent) -> &'static str {
        if !self.runtime_applicable {
            return "runtime_identity_inapplicable";
        }
        match self.team_luck_decision(self.latest_observed_micros, damage) {
            Ok(_) => "emitted",
            Err(gate) => gate,
        }
    }

    fn team_luck_decision(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Result<ExactRationalDamageContributionEvent, &'static str> {
        let critical = damage.flags.critical == Some(true);
        let lucky = damage.flags.lucky == Some(true);
        if !critical && !lucky {
            return Err("occurrence_missing");
        }
        if !self.runtime.runtime_promotion_allowed() {
            if critical
                && !self
                    .runtime
                    .team_luck
                    .critical_damage_runtime_transfer_enabled
            {
                return Err("critical_damage_runtime_transfer_disabled");
            }
            if lucky && !self.runtime.team_luck.lucky_damage_runtime_transfer_enabled {
                return Err("lucky_damage_runtime_transfer_disabled");
            }
            if lucky
                && !self.runtime.team_luck.is_lucky_damage_route(
                    damage.ability.map(|ability| ability.0),
                    damage.hit_event_id,
                )
            {
                return Err("lucky_damage_route_unproven");
            }
        }
        if damage.amount <= 0 {
            return Err("non_positive_damage");
        }
        if self.team_luck_transition_wire == self.current_wire {
            return Err("same_wire_transition");
        }
        let recipient_actor_id = damage.source.actor_id.0;
        let recipient_entity_uuid = damage.source.entity_uuid.0;
        let mut providers = self
            .team_luck_windows
            .iter()
            .filter(|key| key.target_entity_uuid == recipient_entity_uuid)
            .filter(|key| key.provider_entity_uuid != recipient_entity_uuid)
            .filter(|key| self.active_players.contains(&key.provider_actor_id))
            .map(|key| key.provider_actor_id)
            .collect::<HashSet<_>>()
            .into_iter();
        let provider_actor_id = providers.next().ok_or("provider_window_missing")?;
        if providers.next().is_some() {
            return Err("provider_window_ambiguous");
        }
        let state_actor_id = self
            .team_luck_state_actor_id(recipient_actor_id, recipient_entity_uuid, false)
            .ok_or_else(|| {
                self.team_luck_missing_state_gate(
                    recipient_actor_id,
                    recipient_entity_uuid,
                    critical,
                    lucky,
                    "recipient_state_missing",
                )
            })?;
        let state = self.states.get(&state_actor_id).ok_or_else(|| {
            self.team_luck_missing_state_gate(
                recipient_actor_id,
                recipient_entity_uuid,
                critical,
                lucky,
                "recipient_state_missing",
            )
        })?;
        // Team Luck has two independent outcome lanes. A critical-only hit is
        // counterfactual against the recipient's critical-damage state; a
        // Lucky-only hit is counterfactual against Lucky-damage state. Do not
        // require the unrelated lane to have appeared in the retained actor
        // attributes, otherwise a fully proven critical hit is discarded just
        // because no Lucky-damage snapshot was observed (and vice versa).
        let critical_damage_raw = if critical {
            state.critical_damage_raw.ok_or_else(|| {
                self.team_luck_missing_state_gate(
                    recipient_actor_id,
                    recipient_entity_uuid,
                    true,
                    false,
                    "critical_damage_state_missing",
                )
            })?
        } else {
            0
        };
        let lucky_damage_raw = if lucky {
            state.lucky_damage_raw.ok_or_else(|| {
                self.team_luck_missing_state_gate(
                    recipient_actor_id,
                    recipient_entity_uuid,
                    false,
                    true,
                    "lucky_damage_state_missing",
                )
            })?
        } else {
            0
        };
        let (numerator, denominator) = exact_team_luck_accounting_fraction(
            damage.amount,
            critical,
            lucky,
            critical_damage_raw,
            lucky_damage_raw,
            self.runtime.team_luck.critical_raw_delta,
            self.runtime.team_luck.lucky_raw_delta,
            self.runtime.team_luck.combined_critical_lucky_enabled,
            self.runtime.critical_damage_factor_interpretation,
        )
        .ok_or("damage_counterfactual_unproven")?;
        Ok(ExactRationalDamageContributionEvent {
            observed_micros,
            effect_id: self.runtime.team_luck.effect_id,
            provider_actor_id,
            recipient_actor_id,
            numerator,
            denominator,
            observed_damage: damage.amount,
            included: true,
        })
    }

    /// Resolves an attribute state only through the damage actor itself or an
    /// actor alias proven to carry the same nonzero entity UUID. Actor IDs can
    /// rotate during a run, while entity UUID is the stable player identity.
    /// Never fall through to a state whose recorded entity differs.
    fn team_luck_state_actor_id(
        &self,
        recipient_actor_id: u64,
        recipient_entity_uuid: i64,
        staged: bool,
    ) -> Option<u64> {
        let states = if staged {
            &self.staged_states
        } else {
            &self.states
        };
        let direct_identity_matches = self
            .attribute_state_entity_uuid_by_actor
            .get(&recipient_actor_id)
            .is_none_or(|entity_uuid| {
                recipient_entity_uuid == 0 || *entity_uuid == recipient_entity_uuid
            });
        if direct_identity_matches && states.contains_key(&recipient_actor_id) {
            return Some(recipient_actor_id);
        }
        if recipient_entity_uuid == 0 {
            return None;
        }
        let alias_actor_id = self
            .attribute_state_actor_by_entity_uuid
            .get(&recipient_entity_uuid)
            .copied()?;
        (self
            .attribute_state_entity_uuid_by_actor
            .get(&alias_actor_id)
            == Some(&recipient_entity_uuid)
            && states.contains_key(&alias_actor_id))
        .then_some(alias_actor_id)
    }

    fn team_luck_missing_state_gate(
        &self,
        recipient_actor_id: u64,
        recipient_entity_uuid: i64,
        critical: bool,
        lucky: bool,
        missing_gate: &'static str,
    ) -> &'static str {
        let never_observed_gate = match missing_gate {
            "recipient_state_missing" => "recipient_state_never_observed",
            "critical_damage_state_missing" => "critical_damage_state_never_observed",
            "lucky_damage_state_missing" => "lucky_damage_state_never_observed",
            _ => missing_gate,
        };
        let staged_actor_id =
            self.team_luck_state_actor_id(recipient_actor_id, recipient_entity_uuid, true);
        if let Some(staged_state) =
            staged_actor_id.and_then(|actor_id| self.staged_states.get(&actor_id))
        {
            let requested_state_is_present = (!critical
                || staged_state.critical_damage_raw.is_some())
                && (!lucky || staged_state.lucky_damage_raw.is_some());
            if requested_state_is_present {
                return match missing_gate {
                    "recipient_state_missing" => "recipient_state_staged_pending",
                    "critical_damage_state_missing" => "critical_damage_state_staged_pending",
                    "lucky_damage_state_missing" => "lucky_damage_state_staged_pending",
                    _ => missing_gate,
                };
            }
        }
        let state_actor_id = self
            .team_luck_state_actor_id(recipient_actor_id, recipient_entity_uuid, false)
            .unwrap_or(recipient_actor_id);
        if critical
            && self
                .team_luck_critical_cleared_by_snapshot
                .contains(&state_actor_id)
        {
            return "critical_damage_state_cleared_by_snapshot";
        }
        if lucky
            && self
                .team_luck_lucky_cleared_by_snapshot
                .contains(&state_actor_id)
        {
            return "lucky_damage_state_cleared_by_snapshot";
        }
        if critical
            && self
                .team_luck_critical_ever_observed
                .contains(&state_actor_id)
        {
            return "critical_damage_state_previously_observed_missing";
        }
        if lucky && self.team_luck_lucky_ever_observed.contains(&state_actor_id) {
            return "lucky_damage_state_previously_observed_missing";
        }
        never_observed_gate
    }

    fn thunderwind_contribution(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<ExactRationalDamageContributionEvent> {
        if !self
            .runtime
            .effect_runtime_transfer_enabled(self.runtime.thunderwind.effect_id)
        {
            // Thunderwind remains an auditable candidate. A newly resolved
            // shared critical factor must not let it participate in production
            // overlap selection or suppress an independently promoted effect.
            return None;
        }
        self.thunderwind_decision(observed_micros, damage).ok()
    }

    pub fn thunderwind_audit_gate(&self, damage: &rlogs_events::DamageEvent) -> &'static str {
        match self.thunderwind_decision(self.latest_observed_micros, damage) {
            Ok(_) => "emitted",
            Err(gate) => gate,
        }
    }

    fn thunderwind_decision(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Result<ExactRationalDamageContributionEvent, &'static str> {
        if damage.amount <= 0 {
            return Err("non_positive_damage");
        }
        if damage.flags.critical != Some(true) {
            return Err("critical_occurrence_missing");
        }
        if damage.flags.lucky == Some(true) {
            return Err("lucky_overlap_unsupported");
        }
        if self.current_wire.is_some_and(|wire| {
            self.thunderwind_transition_wires
                .get(&damage.source.actor_id.0)
                == Some(&wire)
        }) {
            return Err("same_wire_transition");
        }
        let recipient_actor_id = damage.source.actor_id.0;
        let state = self
            .states
            .get(&recipient_actor_id)
            .ok_or("recipient_state_missing")?;
        let mut providers = state
            .thunderwind_providers
            .iter()
            .filter(|(provider_actor_id, _)| {
                **provider_actor_id != recipient_actor_id
                    && self.active_players.contains(provider_actor_id)
            });
        let (&provider_actor_id, provider) = providers.next().ok_or("provider_window_missing")?;
        if providers.next().is_some() {
            return Err("provider_window_ambiguous");
        }
        let (numerator, denominator) = exact_external_critical_chance_and_damage_fraction(
            damage.amount,
            state
                .critical_chance_raw
                .ok_or("critical_chance_state_missing")?,
            provider.critical_chance_raw_delta,
            state
                .critical_damage_raw
                .ok_or("critical_damage_state_missing")?,
            provider.critical_damage_raw_delta,
            self.runtime.critical_damage_factor_interpretation,
        )
        .ok_or("damage_counterfactual_unproven")?;
        Ok(ExactRationalDamageContributionEvent {
            observed_micros,
            effect_id: self.runtime.thunderwind.effect_id,
            provider_actor_id,
            recipient_actor_id,
            numerator,
            denominator,
            observed_damage: damage.amount,
            included: true,
        })
    }

    fn functional_amp_contribution(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<ExactRationalDamageContributionEvent> {
        self.functional_amp_decision(observed_micros, damage).ok()
    }

    pub fn functional_amp_audit_gate(&self, damage: &rlogs_events::DamageEvent) -> &'static str {
        match self.functional_amp_decision(self.latest_observed_micros, damage) {
            Ok(_) => "emitted",
            Err(gate) => gate,
        }
    }

    fn functional_amp_decision(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Result<ExactRationalDamageContributionEvent, &'static str> {
        if damage.amount <= 0 {
            return Err("non_positive_damage");
        }
        if self.current_wire.is_some_and(|wire| {
            self.functional_amp_transition_wires
                .get(&damage.source.actor_id.0)
                == Some(&wire)
        }) {
            return Err("same_wire_transition");
        }
        let recipient_actor_id = damage.source.actor_id.0;
        let selected = select_damage_stage(
            damage.ability.ok_or("ability_missing")?.0,
            damage.hit_event_id,
            damage.damage_source,
            damage.packet.owner_stage,
            damage.packet.owner_level,
        )
        .ok_or("damage_stage_missing")?;
        let actor_state = self
            .states
            .get(&recipient_actor_id)
            .ok_or("recipient_state_missing")?;
        let family = match selected.offensive_stat {
            OffensiveStatKind::PhysicalAttack => &actor_state.physical_attack,
            OffensiveStatKind::MagicalAttack => &actor_state.magical_attack,
        };
        let desired = self.desired_functional_amp_provider_percentages(
            recipient_actor_id,
            damage.source.entity_uuid.0,
        );
        if desired.len() != 1 {
            // Functional Amp is refresh-only. More than one packet-active
            // provider makes ownership of the one +360 component ambiguous.
            return Err(if desired.is_empty() {
                "provider_window_missing"
            } else {
                "provider_window_ambiguous"
            });
        }
        let (&(provider_actor_id, provider_entity_uuid), &provider_percent) =
            desired.first_key_value().ok_or("provider_window_missing")?;
        if provider_actor_id == recipient_actor_id {
            return Err("provider_is_recipient");
        }
        if provider_percent != self.runtime.functional_amp.attack_percent_raw_delta {
            return Err("provider_magnitude_mismatch");
        }
        if !self.active_players.contains(&provider_actor_id) {
            return Err("provider_inactive");
        }
        if self.actor_ancestry.actor_for_entity(provider_entity_uuid) != Some(provider_actor_id) {
            return Err("provider_ancestry_mismatch");
        }
        if self
            .actor_ancestry
            .actor_for_entity(damage.source.entity_uuid.0)
            != Some(recipient_actor_id)
        {
            return Err("recipient_ancestry_mismatch");
        }
        if family.provider_raw_percent != BTreeMap::from([(provider_actor_id, provider_percent)]) {
            return Err("provider_state_decomposition_mismatch");
        }

        exact_attack_family_stage_contribution(
            observed_micros,
            self.runtime.functional_amp.effect_id,
            provider_actor_id,
            recipient_actor_id,
            damage.amount,
            family,
            0,
            provider_percent,
            selected,
        )
        .ok_or("damage_counterfactual_unproven")
    }

    fn harmony_grace_contribution(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<ExactRationalDamageContributionEvent> {
        self.harmony_grace_decision(observed_micros, damage).ok()
    }

    /// Audit-only explanation for why a damage row did or did not receive a
    /// Harmony Grace transfer. Live projection does not call this method, so
    /// detailed proof diagnostics add no work to the meter's hot path.
    pub fn harmony_grace_audit_gate(&self, damage: &rlogs_events::DamageEvent) -> &'static str {
        match self.harmony_grace_decision(self.latest_observed_micros, damage) {
            Ok(_) => "emitted",
            Err(gate) => gate,
        }
    }

    /// Bounded offline diagnostic for a rejected Harmony Grace damage row.
    /// This deliberately reuses the live decision function, then snapshots
    /// only the state needed to explain that decision. The replay auditor is
    /// the only caller, so none of this formatting is on the live meter path.
    pub fn harmony_grace_audit_detail(&self, damage: &rlogs_events::DamageEvent) -> String {
        let gate = self.harmony_grace_audit_gate(damage);
        let recipient_actor_id = damage.source.actor_id.0;
        let ability_id = damage.ability.map(|id| id.0);
        let class_id = self.recipient_class_id_for_actor(
            recipient_actor_id,
            &self.runtime.harmony_grace.recipient_rules,
        );
        let recipient_rule = class_id.and_then(|class_id| {
            self.runtime
                .harmony_grace
                .recipient_rules
                .iter()
                .find(|rule| rule.recipient_class_id == class_id)
        });
        let desired = recipient_rule
            .map(|rule| self.desired_harmony_grace_provider_percentages(recipient_actor_id, rule))
            .unwrap_or_default();
        let windows = self
            .harmony_grace_windows
            .iter()
            .filter(|key| key.target_actor_id == recipient_actor_id)
            .map(|key| (key.provider_actor_id, key.instance_id))
            .collect::<Vec<_>>();
        let state = self.states.get(&recipient_actor_id);
        let staged_state = self.staged_states.get(&recipient_actor_id);
        let primary = recipient_rule.and_then(|rule| {
            state.and_then(|state| state.harmony_primary_by_class.get(&rule.recipient_class_id))
        });
        let staged_primary = recipient_rule.and_then(|rule| {
            staged_state
                .and_then(|state| state.harmony_primary_by_class.get(&rule.recipient_class_id))
        });
        let providers = desired
            .keys()
            .map(|provider| (*provider, self.active_players.contains(provider)))
            .collect::<Vec<_>>();

        format!(
            "gate={gate}; recipient={recipient_actor_id}; ability={ability_id:?}; class={class_id:?}; rule_count={}; desired={desired:?}; windows={windows:?}; providers_active={providers:?}; current_wire={:?}; transition_wire={:?}; state_primary={primary:?}; staged_primary={staged_primary:?}",
            self.runtime.harmony_grace.recipient_rules.len(),
            self.current_wire,
            self.harmony_grace_transition_wires.get(&recipient_actor_id),
        )
    }

    /// Returns the complete exact arithmetic receipt for an emitted Harmony
    /// Grace row. This repeats the inexpensive audit calculation only when an
    /// offline caller asks for the trace; the live hot path is unchanged.
    pub fn harmony_grace_formula_trace(
        &self,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<HarmonyGraceFormulaTrace> {
        let contribution = self
            .harmony_grace_decision(self.latest_observed_micros, damage)
            .ok()?;
        let ability_id = damage.ability?.0;
        let recipient_actor_id = damage.source.actor_id.0;
        let recipient_class_id = self.recipient_class_id_for_actor(
            recipient_actor_id,
            &self.runtime.harmony_grace.recipient_rules,
        )?;
        let recipient_rule = self
            .runtime
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == recipient_class_id)?;
        let selected = select_damage_stage(
            ability_id,
            damage.hit_event_id,
            damage.damage_source,
            damage.packet.owner_stage,
            damage.packet.owner_level,
        )?;
        let desired =
            self.desired_harmony_grace_provider_percentages(recipient_actor_id, recipient_rule);
        let (&provider_actor_id, &provider_primary_raw_percent) = desired.first_key_value()?;
        let state = self.states.get(&recipient_actor_id)?;
        let primary = state
            .harmony_primary_by_class
            .get(&recipient_rule.recipient_class_id)?;
        let primary_final = primary.final_value?;
        let primary_intermediate = primary.intermediate_value?;
        let primary_base_add = primary.base_add?;
        let primary_extra_add = primary.extra_add?;
        let primary_raw_percent = primary.raw_percent?;
        let primary_witness = self
            .harmony_grace_primary_transition_witness(
                recipient_actor_id,
                provider_actor_id,
                primary,
                provider_primary_raw_percent,
            )
            .ok()?;
        let primary_provider_marginal = primary_witness.provider_primary_marginal;
        if primary_intermediate.checked_add(primary_extra_add) != Some(primary_final) {
            return None;
        }
        let primary_without_provider = primary_final.checked_sub(primary_provider_marginal)?;
        let attack_component_with_provider = checked_positive_floor_ratio(
            primary_final,
            recipient_rule.primary_to_attack_numerator,
            recipient_rule.primary_to_attack_denominator,
        )?;
        let attack_component_without_provider = checked_positive_floor_ratio(
            primary_without_provider,
            recipient_rule.primary_to_attack_numerator,
            recipient_rule.primary_to_attack_denominator,
        )?;
        let provider_attack_base_add =
            attack_component_with_provider.checked_sub(attack_component_without_provider)?;
        let attack = attack_family_for_lane(state, recipient_rule.attack_lane);
        let attack_final = attack.final_value?;
        let attack_intermediate = attack.intermediate_value?;
        let attack_base_add = attack.base_add?;
        let attack_extra_add = attack.extra_add?;
        let attack_raw_percent = attack.raw_percent?;
        let provider_attack_marginal =
            exact_packet_attack_provider_marginal(attack, provider_attack_base_add, 0)?;
        let attack_without_provider = attack_final.checked_sub(provider_attack_marginal)?;
        let active_coefficient_term =
            fixed_point_stage_term(attack_final, selected.coefficient_basis_points)?;
        let active_stage_body = active_coefficient_term.checked_add(selected.fixed_parameter)?;
        let without_provider_coefficient_term =
            fixed_point_stage_term(attack_without_provider, selected.coefficient_basis_points)?;
        let coefficient_stage_marginal =
            active_coefficient_term.checked_sub(without_provider_coefficient_term)?;

        Some(HarmonyGraceFormulaTrace {
            effect_id: contribution.effect_id,
            provider_actor_id,
            recipient_actor_id,
            recipient_class_id,
            attack_lane: match recipient_rule.attack_lane {
                PrimaryAttackLane::PhysicalAttack => "physical_attack",
                PrimaryAttackLane::MagicalAttack => "magical_attack",
            },
            ability_id,
            hit_event_id: damage.hit_event_id,
            damage_attr_id: selected.damage_attr_id,
            observed_damage: damage.amount,
            primary_final,
            primary_intermediate,
            primary_base_add,
            primary_extra_add,
            primary_raw_percent,
            primary_family_rounding: attribute_family_rounding_name(
                self.runtime.harmony_grace.primary_family_rounding,
            ),
            provider_primary_raw_percent,
            primary_provider_marginal_basis: "same_lifecycle_packet_transition",
            primary_transition_connection_id: primary_witness.wire.connection_id,
            primary_transition_stream_id: primary_witness.wire.stream_id,
            primary_transition_capture_sequence: primary_witness.wire.capture_sequence,
            primary_transition_instance_id: primary_witness.instance_id,
            primary_provider_marginal,
            primary_without_provider,
            primary_to_attack_numerator: recipient_rule.primary_to_attack_numerator,
            primary_to_attack_denominator: recipient_rule.primary_to_attack_denominator,
            attack_component_with_provider,
            attack_component_without_provider,
            provider_attack_base_add,
            attack_final,
            attack_intermediate,
            attack_base_add,
            attack_extra_add,
            attack_raw_percent,
            provider_attack_marginal,
            attack_without_provider,
            coefficient_basis_points: selected.coefficient_basis_points,
            fixed_parameter: selected.fixed_parameter,
            active_coefficient_term,
            active_stage_body,
            without_provider_coefficient_term,
            coefficient_stage_marginal,
            contribution_numerator: contribution.numerator,
            contribution_denominator: contribution.denominator,
        })
    }

    /// Captures one complete active-window state under both plausible positive
    /// fixed-point rounding rules. It does not emit attribution or alter live
    /// state; the replay audit aggregates equal states and their row counts.
    pub fn harmony_grace_family_rounding_diagnostic(
        &self,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<HarmonyGraceFamilyRoundingDiagnostic> {
        let recipient_actor_id = damage.source.actor_id.0;
        let recipient_class_id = self.recipient_class_id_for_actor(
            recipient_actor_id,
            &self.runtime.harmony_grace.recipient_rules,
        )?;
        let recipient_rule = self
            .runtime
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == recipient_class_id)?;
        let desired =
            self.desired_harmony_grace_provider_percentages(recipient_actor_id, recipient_rule);
        if desired.len() != 1 {
            return None;
        }
        let (&provider_actor_id, _) = desired.first_key_value()?;
        if provider_actor_id == recipient_actor_id
            || !self.active_players.contains(&provider_actor_id)
        {
            return None;
        }
        let state = self.states.get(&recipient_actor_id)?;
        let primary = state
            .harmony_primary_by_class
            .get(&recipient_rule.recipient_class_id)?;
        let primary_base_add = primary.base_add?;
        let primary_raw_percent = primary.raw_percent?;
        let primary_extra_add = primary.extra_add?;
        let primary_observed_intermediate = primary.intermediate_value?;
        let primary_observed_final = primary.final_value?;
        let primary_floor_intermediate = fixed_point_stage_term(
            primary_base_add,
            BPSR_FIXED_POINT_SCALE + primary_raw_percent,
        )?;
        let primary_nearest_intermediate = fixed_point_stage_term_nearest(
            primary_base_add,
            BPSR_FIXED_POINT_SCALE + primary_raw_percent,
        )?;
        let attack = attack_family_for_lane(state, recipient_rule.attack_lane);
        let attack_base_add = attack.base_add?;
        let attack_raw_percent = attack.raw_percent?;
        let attack_extra_add = attack.extra_add?;
        let attack_observed_intermediate = attack.intermediate_value?;
        let attack_observed_final = attack.final_value?;
        let attack_floor_intermediate =
            fixed_point_stage_term(attack_base_add, BPSR_FIXED_POINT_SCALE + attack_raw_percent)?;
        let attack_nearest_intermediate = fixed_point_stage_term_nearest(
            attack_base_add,
            BPSR_FIXED_POINT_SCALE + attack_raw_percent,
        )?;

        Some(HarmonyGraceFamilyRoundingDiagnostic {
            provider_actor_id,
            recipient_actor_id,
            recipient_class_id,
            provider_decomposition_matches: primary.provider_raw_percent == desired,
            primary_base_add,
            primary_raw_percent,
            primary_extra_add,
            primary_observed_intermediate,
            primary_floor_intermediate,
            primary_nearest_intermediate,
            primary_observed_final,
            primary_floor_final: primary_floor_intermediate.checked_add(primary_extra_add)?,
            primary_nearest_final: primary_nearest_intermediate.checked_add(primary_extra_add)?,
            attack_base_add,
            attack_raw_percent,
            attack_extra_add,
            attack_observed_intermediate,
            attack_floor_intermediate,
            attack_nearest_intermediate,
            attack_observed_final,
            attack_floor_final: attack_floor_intermediate.checked_add(attack_extra_add)?,
            attack_nearest_final: attack_nearest_intermediate.checked_add(attack_extra_add)?,
        })
    }

    fn harmony_grace_decision(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Result<ExactRationalDamageContributionEvent, &'static str> {
        if !self.runtime.harmony_grace.runtime_transfer_enabled
            && !self.harmony_grace_candidate_audit_enabled
        {
            return Err("runtime_transfer_disabled");
        }
        if damage.amount <= 0 {
            return Err("non_positive_damage");
        }
        if self.current_wire.is_some_and(|wire| {
            self.harmony_grace_transition_wires
                .get(&damage.source.actor_id.0)
                == Some(&wire)
        }) {
            return Err("same_wire_transition");
        }
        let ability_id = damage.ability.ok_or("ability_missing")?.0;
        let class_id = self
            .recipient_class_id_for_actor(
                damage.source.actor_id.0,
                &self.runtime.harmony_grace.recipient_rules,
            )
            .ok_or("recipient_class_missing")?;
        if !self.harmony_grace_candidate_audit_enabled
            && !self
                .runtime
                .harmony_grace
                .runtime_recipient_class_ids
                .contains(&class_id)
        {
            return Err("recipient_class_not_runtime_promoted");
        }
        let recipient_rule = self
            .runtime
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == class_id)
            .ok_or("recipient_rule_missing")?;
        let selected = select_damage_stage(
            ability_id,
            damage.hit_event_id,
            damage.damage_source,
            damage.packet.owner_stage,
            damage.packet.owner_level,
        )
        .ok_or("damage_stage_missing")?;
        if !attack_lane_matches(recipient_rule.attack_lane, selected.offensive_stat) {
            return Err("attack_lane_mismatch");
        }

        let recipient_actor_id = damage.source.actor_id.0;
        let desired =
            self.desired_harmony_grace_provider_percentages(recipient_actor_id, recipient_rule);
        if desired.len() != 1 {
            return Err(if desired.is_empty() {
                "provider_window_missing"
            } else {
                "provider_window_ambiguous"
            });
        }
        let (&provider_actor_id, &provider_percent) =
            desired.first_key_value().ok_or("provider_window_missing")?;
        if provider_actor_id == recipient_actor_id {
            return Err("self_provider");
        }
        if provider_percent != recipient_rule.primary_percent_raw_delta {
            return Err("provider_percent_mismatch");
        }
        if !self.active_players.contains(&provider_actor_id) {
            return Err("provider_not_active_player");
        }
        let actor_state = self
            .states
            .get(&recipient_actor_id)
            .ok_or("recipient_state_missing")?;
        let primary = actor_state
            .harmony_primary_by_class
            .get(&recipient_rule.recipient_class_id)
            .ok_or("primary_family_missing")?;
        let (
            Some(primary_final),
            Some(primary_intermediate),
            Some(_primary_base_add),
            Some(primary_extra_add),
            Some(_primary_raw_percent),
        ) = (
            primary.final_value,
            primary.intermediate_value,
            primary.base_add,
            primary.extra_add,
            primary.raw_percent,
        )
        else {
            return Err("primary_family_incomplete");
        };
        if primary_intermediate.checked_add(primary_extra_add) != Some(primary_final) {
            return Err("primary_family_operation_order_mismatch");
        }
        if primary.provider_raw_percent != desired {
            return Err("provider_decomposition_mismatch");
        }
        let primary_witness = self.harmony_grace_primary_transition_witness(
            recipient_actor_id,
            provider_actor_id,
            primary,
            provider_percent,
        )?;
        let provider_attack_base_add = exact_primary_to_attack_provider_base_add_from_witness(
            primary_final,
            primary_witness.provider_primary_marginal,
            recipient_rule.primary_to_attack_numerator,
            recipient_rule.primary_to_attack_denominator,
        )
        .ok_or("primary_to_attack_unproven")?;

        let attack_family = attack_family_for_lane(actor_state, recipient_rule.attack_lane);
        let (
            Some(attack_final),
            Some(attack_intermediate),
            Some(attack_base_add),
            Some(attack_extra_add),
            Some(attack_raw_percent),
        ) = (
            attack_family.final_value,
            attack_family.intermediate_value,
            attack_family.base_add,
            attack_family.extra_add,
            attack_family.raw_percent,
        )
        else {
            return Err("attack_family_incomplete");
        };
        if packet_attribute_family_value(attack_base_add, attack_raw_percent, 0)
            != Some(attack_intermediate)
            || packet_attribute_family_value(attack_base_add, attack_raw_percent, attack_extra_add)
                != Some(attack_final)
        {
            return Err("attack_family_formula_mismatch");
        }

        exact_attack_family_stage_contribution(
            observed_micros,
            self.runtime.harmony_grace.effect_id,
            provider_actor_id,
            recipient_actor_id,
            damage.amount,
            attack_family,
            provider_attack_base_add,
            0,
            selected,
        )
        .ok_or("damage_counterfactual_unproven")
    }

    fn mechanical_power_contribution(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<ExactRationalDamageContributionEvent> {
        self.mechanical_power_decision(observed_micros, damage).ok()
    }

    pub fn mechanical_power_audit_gate(&self, damage: &rlogs_events::DamageEvent) -> &'static str {
        if self.damage_has_unresolved_status_confounder(damage) {
            return "unresolved_status_confounder";
        }
        match self.mechanical_power_decision(self.latest_observed_micros, damage) {
            Ok(_) => "emitted",
            Err(gate) => gate,
        }
    }

    fn mechanical_power_decision(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Result<ExactRationalDamageContributionEvent, &'static str> {
        if !self.runtime.mechanical_power.runtime_transfer_enabled
            && !self.mechanical_power_candidate_audit_enabled
        {
            return Err("runtime_transfer_disabled");
        }
        if damage.amount <= 0 {
            return Err("non_positive_damage");
        }
        if self.current_wire.is_some_and(|wire| {
            self.mechanical_power_transition_wires
                .get(&damage.source.actor_id.0)
                == Some(&wire)
        }) {
            return Err("same_wire_transition");
        }
        let recipient_actor_id = damage.source.actor_id.0;
        let class_id = self
            .recipient_class_id_for_actor(
                recipient_actor_id,
                &self.runtime.mechanical_power.recipient_rules,
            )
            .ok_or("recipient_class_missing")?;
        let recipient_rule = self
            .runtime
            .mechanical_power
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == class_id)
            .ok_or("recipient_rule_missing")?;
        let desired =
            self.desired_mechanical_power_provider_percentages(recipient_actor_id, recipient_rule);
        if desired.len() != 1 {
            return Err(if desired.is_empty() {
                "provider_window_missing"
            } else {
                "provider_window_ambiguous"
            });
        }
        let (&provider_actor_id, &provider_percent) =
            desired.first_key_value().ok_or("provider_window_missing")?;
        if provider_actor_id == recipient_actor_id {
            return Err("provider_is_recipient");
        }
        let expected_provider_percent = self
            .mechanical_power_candidate_primary_percent_override
            .or_else(|| {
                self.runtime
                    .mechanical_power
                    .production_primary_percent_raw_delta(recipient_rule.recipient_class_id)
            })
            .unwrap_or(recipient_rule.primary_percent_raw_delta);
        if provider_percent != expected_provider_percent {
            return Err("provider_magnitude_mismatch");
        }
        if !self.active_players.contains(&provider_actor_id) {
            return Err("provider_inactive");
        }
        let selected = select_damage_stage(
            damage.ability.ok_or("ability_missing")?.0,
            damage.hit_event_id,
            damage.damage_source,
            damage.packet.owner_stage,
            damage.packet.owner_level,
        )
        .ok_or("damage_stage_missing")?;
        if !attack_lane_matches(recipient_rule.attack_lane, selected.offensive_stat) {
            return Err("attack_lane_mismatch");
        }
        let actor_state = self
            .states
            .get(&recipient_actor_id)
            .ok_or("recipient_state_missing")?;
        let primary = actor_state
            .mechanical_primary_by_class
            .get(&recipient_rule.recipient_class_id)
            .ok_or("recipient_primary_state_missing")?;
        if !self.mechanical_power_exact_scoped_rebase_allowed(provider_percent)
            && primary.provider_raw_percent != desired
        {
            return Err("provider_state_decomposition_mismatch");
        }
        let primary_witness = self.mechanical_power_primary_transition_witness(
            recipient_actor_id,
            provider_actor_id,
            primary,
            provider_percent,
        )?;
        let primary_current = primary.final_value.ok_or("primary_family_incomplete")?;
        let provider_attack_base_add = exact_primary_to_attack_provider_base_add_from_witness(
            primary_current,
            primary_witness.provider_primary_marginal,
            recipient_rule.primary_to_attack_numerator,
            recipient_rule.primary_to_attack_denominator,
        )
        .ok_or("primary_to_attack_unproven")?;

        let attack_family = attack_family_for_lane(actor_state, recipient_rule.attack_lane);
        let current_attack = attack_family
            .final_value
            .ok_or("attack_family_incomplete")?;
        let provider_attack_marginal =
            exact_packet_attack_provider_marginal(attack_family, provider_attack_base_add, 0)
                .ok_or("attack_family_counterfactual_unproven")?;
        let (numerator, denominator) = exact_external_attack_coefficient_stage_fraction(
            damage.amount,
            PacketDamageScriptFamily::StandardAttack,
            current_attack,
            provider_attack_marginal,
            selected.coefficient_basis_points,
            selected.fixed_parameter,
        )
        .ok_or("damage_coefficient_counterfactual_unproven")?;
        Ok(ExactRationalDamageContributionEvent {
            observed_micros,
            effect_id: self.runtime.mechanical_power.effect_id,
            provider_actor_id,
            recipient_actor_id,
            numerator,
            denominator,
            observed_damage: damage.amount,
            included: true,
        })
    }

    fn inspiration_contribution(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<ExactRationalDamageContributionEvent> {
        self.inspiration_decision(observed_micros, damage).ok()
    }

    pub fn inspiration_audit_gate(&self, damage: &rlogs_events::DamageEvent) -> &'static str {
        match self.inspiration_decision(self.latest_observed_micros, damage) {
            Ok(_) => "emitted",
            Err(gate) => gate,
        }
    }

    fn inspiration_decision(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Result<ExactRationalDamageContributionEvent, &'static str> {
        if !self.runtime.inspiration.runtime_transfer_enabled
            && !self.inspiration_candidate_audit_enabled
        {
            return Err("runtime_transfer_disabled");
        }
        if damage.amount <= 0 {
            return Err("non_positive_damage");
        }
        if self.current_wire.is_some_and(|wire| {
            self.inspiration_transition_wires
                .get(&damage.source.actor_id.0)
                == Some(&wire)
        }) {
            return Err("same_wire_transition");
        }
        let recipient_actor_id = damage.source.actor_id.0;
        let actor_state = self
            .states
            .get(&recipient_actor_id)
            .ok_or("recipient_state_missing")?;
        if actor_state.inspiration_providers.len() != 1 {
            return Err(if actor_state.inspiration_providers.is_empty() {
                "provider_window_missing"
            } else {
                "provider_window_ambiguous"
            });
        }
        let (&provider_actor_id, provider) = actor_state
            .inspiration_providers
            .first_key_value()
            .ok_or("provider_window_missing")?;
        if provider_actor_id == recipient_actor_id {
            return Err("provider_is_recipient");
        }
        if !self.active_players.contains(&provider_actor_id) {
            return Err("provider_inactive");
        }
        if self
            .desired_inspiration_provider_modes(recipient_actor_id)
            .ok_or("provider_mode_window_missing")?
            != BTreeMap::from([(provider_actor_id, provider.provider_full_bloom)])
        {
            return Err("provider_mode_mismatch");
        }
        let selected = select_damage_stage(
            damage.ability.ok_or("ability_missing")?.0,
            damage.hit_event_id,
            damage.damage_source,
            damage.packet.owner_stage,
            damage.packet.owner_level,
        )
        .ok_or("damage_stage_missing")?;
        let (family, provider_attack_base_add) = match selected.offensive_stat {
            OffensiveStatKind::PhysicalAttack => (
                &actor_state.physical_attack,
                provider
                    .physical_attack_base_add_delta
                    .ok_or("provider_physical_attack_delta_missing")?,
            ),
            OffensiveStatKind::MagicalAttack => (
                &actor_state.magical_attack,
                provider
                    .magical_attack_base_add_delta
                    .ok_or("provider_magical_attack_delta_missing")?,
            ),
        };
        let provider_attack_marginal =
            exact_packet_attack_provider_marginal(family, provider_attack_base_add, 0)
                .ok_or("attack_counterfactual_unproven")?;
        let current_external_factor = BPSR_FIXED_POINT_SCALE
            .checked_add(
                actor_state
                    .external_damage_raw
                    .ok_or("external_damage_state_missing")?,
            )
            .ok_or("external_damage_overflow")?;
        let external_factor = (current_external_factor, provider.external_damage_delta);
        let property_matches =
            damage.packet.property == Some(self.runtime.inspiration.property_damage_property);
        let (numerator, denominator) = if property_matches {
            let provider_property_delta = provider
                .property_damage_delta
                .ok_or("provider_property_damage_delta_missing")?;
            let current_property_factor = BPSR_FIXED_POINT_SCALE
                .checked_add(
                    actor_state
                        .property_damage_raw
                        .ok_or("property_damage_state_missing")?,
                )
                .ok_or("property_damage_overflow")?;
            exact_external_attack_and_factors_fraction(
                damage.amount,
                PacketDamageScriptFamily::StandardAttack,
                family.final_value.ok_or("attack_final_state_missing")?,
                provider_attack_marginal,
                selected.coefficient_basis_points,
                selected.fixed_parameter,
                &[
                    external_factor,
                    (current_property_factor, provider_property_delta),
                ],
            )
            .ok_or("damage_counterfactual_unproven")?
        } else {
            exact_external_attack_and_factors_fraction(
                damage.amount,
                PacketDamageScriptFamily::StandardAttack,
                family.final_value.ok_or("attack_final_state_missing")?,
                provider_attack_marginal,
                selected.coefficient_basis_points,
                selected.fixed_parameter,
                &[external_factor],
            )
            .ok_or("damage_counterfactual_unproven")?
        };
        Ok(ExactRationalDamageContributionEvent {
            observed_micros,
            effect_id: self.runtime.inspiration.effect_id,
            provider_actor_id,
            recipient_actor_id,
            numerator,
            denominator,
            observed_damage: damage.amount,
            included: true,
        })
    }

    fn inspiration_occurrence_contribution(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Option<ExactRationalDamageContributionEvent> {
        self.inspiration_occurrence_decision(observed_micros, damage)
            .ok()
    }

    pub fn inspiration_occurrence_audit_gate(
        &self,
        damage: &rlogs_events::DamageEvent,
    ) -> &'static str {
        match self.inspiration_occurrence_decision(self.latest_observed_micros, damage) {
            Ok(_) => "emitted",
            Err(gate) => gate,
        }
    }

    fn inspiration_occurrence_decision(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
    ) -> Result<ExactRationalDamageContributionEvent, &'static str> {
        if !self.runtime.inspiration.runtime_transfer_enabled
            && !self.inspiration_candidate_audit_enabled
        {
            return Err("runtime_transfer_disabled");
        }
        if damage.amount <= 0 {
            return Err("non_positive_damage");
        }
        if self.current_wire.is_some_and(|wire| {
            self.inspiration_transition_wires
                .get(&damage.source.actor_id.0)
                == Some(&wire)
        }) {
            return Err("same_wire_transition");
        }
        let critical = damage.flags.critical == Some(true);
        let lucky = damage.flags.lucky == Some(true);
        if !critical && !lucky {
            return Err("occurrence_missing");
        }
        let recipient_actor_id = damage.source.actor_id.0;
        let state = self
            .states
            .get(&recipient_actor_id)
            .ok_or("recipient_state_missing")?;
        if state.inspiration_providers.len() != 1 {
            return Err(if state.inspiration_providers.is_empty() {
                "provider_window_missing"
            } else {
                "provider_window_ambiguous"
            });
        }
        let (&provider_actor_id, provider) = state
            .inspiration_providers
            .first_key_value()
            .ok_or("provider_window_missing")?;
        if provider_actor_id == recipient_actor_id {
            return Err("provider_is_recipient");
        }
        if !self.active_players.contains(&provider_actor_id) {
            return Err("provider_inactive");
        }
        let (numerator, denominator) = exact_inspiration_occurrence_fraction(
            damage.amount,
            critical,
            lucky,
            state
                .critical_chance_raw
                .ok_or("critical_chance_state_missing")?,
            state.lucky_chance_raw.ok_or("lucky_chance_state_missing")?,
            provider.secondary_raw_add_delta,
            state
                .critical_damage_raw
                .ok_or("critical_damage_state_missing")?,
            self.runtime.critical_damage_factor_interpretation,
        )
        .ok_or("damage_counterfactual_unproven")?;
        Ok(ExactRationalDamageContributionEvent {
            observed_micros,
            effect_id: self.runtime.inspiration.effect_id,
            provider_actor_id,
            recipient_actor_id,
            numerator,
            denominator,
            observed_damage: damage.amount,
            included: true,
        })
    }

    fn combined_harmony_functional_amp_contributions(
        &self,
        observed_micros: u64,
        damage: &rlogs_events::DamageEvent,
        functional_amp: ExactRationalDamageContributionEvent,
        harmony_grace: ExactRationalDamageContributionEvent,
    ) -> Option<[ExactRationalDamageContributionEvent; 2]> {
        if functional_amp.observed_damage != damage.amount
            || harmony_grace.observed_damage != damage.amount
            || functional_amp.recipient_actor_id != harmony_grace.recipient_actor_id
            || functional_amp.effect_id != self.runtime.functional_amp.effect_id
            || harmony_grace.effect_id != self.runtime.harmony_grace.effect_id
        {
            return None;
        }

        let recipient_actor_id = damage.source.actor_id.0;
        let selected = select_damage_stage(
            damage.ability?.0,
            damage.hit_event_id,
            damage.damage_source,
            damage.packet.owner_stage,
            damage.packet.owner_level,
        )?;
        let class_id = self.recipient_class_id_for_actor(
            recipient_actor_id,
            &self.runtime.harmony_grace.recipient_rules,
        )?;
        let harmony_rule = self
            .runtime
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == class_id)?;
        if !attack_lane_matches(harmony_rule.attack_lane, selected.offensive_stat) {
            return None;
        }
        let actor_state = self.states.get(&recipient_actor_id)?;
        let family = attack_family_for_lane(actor_state, harmony_rule.attack_lane);
        let active_attack = family.final_value?;
        let intermediate = family.intermediate_value?;
        let base_add = family.base_add?;
        let extra_add = family.extra_add?;
        let raw_percent = family.raw_percent?;
        if packet_attribute_family_value(base_add, raw_percent, 0)? != intermediate
            || packet_attribute_family_value(base_add, raw_percent, extra_add)? != active_attack
        {
            return None;
        }

        let harmony_percent = harmony_rule.primary_percent_raw_delta;
        let harmony_primary = actor_state
            .harmony_primary_by_class
            .get(&harmony_rule.recipient_class_id)?;
        let harmony_witness = self
            .harmony_grace_primary_transition_witness(
                recipient_actor_id,
                harmony_grace.provider_actor_id,
                harmony_primary,
                harmony_percent,
            )
            .ok()?;
        let harmony_base_add = exact_primary_to_attack_provider_base_add_from_witness(
            harmony_primary.final_value?,
            harmony_witness.provider_primary_marginal,
            harmony_rule.primary_to_attack_numerator,
            harmony_rule.primary_to_attack_denominator,
        )?;
        let functional_percent = self.runtime.functional_amp.attack_percent_raw_delta;
        let base_without_harmony = base_add.checked_sub(harmony_base_add)?;
        let percent_without_functional = raw_percent.checked_sub(functional_percent)?;
        let attack_without_harmony =
            packet_attribute_family_value(base_without_harmony, raw_percent, extra_add)?;
        let attack_without_both = packet_attribute_family_value(
            base_without_harmony,
            percent_without_functional,
            extra_add,
        )?;
        if active_attack <= attack_without_harmony || attack_without_harmony <= attack_without_both
        {
            return None;
        }

        let (harmony_numerator, harmony_denominator) =
            exact_external_attack_ordered_stage_fraction(
                damage.amount,
                PacketDamageScriptFamily::StandardAttack,
                active_attack,
                active_attack,
                attack_without_harmony,
                selected.coefficient_basis_points,
                selected.fixed_parameter,
            )?;
        let (functional_numerator, functional_denominator) =
            exact_external_attack_ordered_stage_fraction(
                damage.amount,
                PacketDamageScriptFamily::StandardAttack,
                active_attack,
                attack_without_harmony,
                attack_without_both,
                selected.coefficient_basis_points,
                selected.fixed_parameter,
            )?;

        Some([
            ExactRationalDamageContributionEvent {
                observed_micros,
                effect_id: harmony_grace.effect_id,
                provider_actor_id: harmony_grace.provider_actor_id,
                recipient_actor_id,
                numerator: harmony_numerator,
                denominator: harmony_denominator,
                observed_damage: damage.amount,
                included: true,
            },
            ExactRationalDamageContributionEvent {
                observed_micros,
                effect_id: functional_amp.effect_id,
                provider_actor_id: functional_amp.provider_actor_id,
                recipient_actor_id,
                numerator: functional_numerator,
                denominator: functional_denominator,
                observed_damage: damage.amount,
                included: true,
            },
        ])
    }

    fn target_vulnerability_exact_contribution(
        &self,
        envelope: &EventEnvelope,
        damage: &rlogs_events::DamageEvent,
        rule: &TargetVulnerabilityRdpsRule,
    ) -> Option<ExactDamageContributionEvent> {
        if rule.projection == TargetVulnerabilityProjection::PairedObservedOutput {
            return self.target_vulnerability_paired_contribution(envelope, damage, rule);
        }
        if rule.projection != TargetVulnerabilityProjection::Integer {
            return None;
        }
        if damage.amount <= 0
            || damage.ability.map(|ability| ability.0) != Some(rule.ability_id)
            || damage.hit_event_id != Some(rule.hit_event_id)
            || !rule.required_critical.matches(damage.flags.critical)
            || damage.flags.lucky != Some(rule.required_lucky)
        {
            return None;
        }

        let recipient_actor_id = damage.source.actor_id.0;
        let target_actor_id = damage.target.actor_id.0;
        let (provider_ids, _) = self.target_vulnerability_provider_ids(
            recipient_actor_id,
            target_actor_id,
            rule.effect_id,
        );
        let mut providers = provider_ids.into_iter();
        let provider_actor_id = providers.next()?;
        if providers.next().is_some() {
            return None;
        }

        let current_factor = rule.active_factor()?;
        let amount = exact_additive_fixed_point_marginal_from_observed_output(
            damage.amount,
            current_factor,
            rule.provider_raw_delta,
            rule.fixed_point_rounding()?,
        )?;
        Some(ExactDamageContributionEvent {
            observed_micros: envelope.time.observed_micros,
            effect_id: rule.effect_id,
            provider_actor_id,
            recipient_actor_id,
            amount,
            observed_damage: damage.amount,
            included: true,
        })
    }

    fn target_vulnerability_paired_contribution(
        &self,
        envelope: &EventEnvelope,
        damage: &rlogs_events::DamageEvent,
        rule: &TargetVulnerabilityRdpsRule,
    ) -> Option<ExactDamageContributionEvent> {
        let active_observed_damage = rule.active_observed_damage?;
        let inactive_observed_damage = rule.inactive_observed_damage?;
        if damage.amount != active_observed_damage
            || damage.ability.map(|ability| ability.0) != Some(rule.ability_id)
            || damage.hit_event_id != Some(rule.hit_event_id)
            || !rule.required_critical.matches(damage.flags.critical)
            || damage.flags.lucky != Some(rule.required_lucky)
        {
            return None;
        }

        let recipient_actor_id = damage.source.actor_id.0;
        let target_actor_id = damage.target.actor_id.0;
        if self.class_id_by_actor.get(&recipient_actor_id).copied() != rule.required_source_class_id
            || self
                .formula_context_sha256(
                    recipient_actor_id,
                    rule.effect_id,
                    &rule.ignored_context_effect_ids,
                )
                .as_deref()
                != rule.required_source_context_sha256.as_deref()
        {
            return None;
        }
        let target_context = self.formula_context_sha256(
            target_actor_id,
            rule.effect_id,
            &rule.ignored_context_effect_ids,
        )?;
        if !rule
            .allowed_target_context_sha256
            .iter()
            .any(|allowed| allowed == &target_context)
        {
            return None;
        }

        let (provider_ids, _) = self.target_vulnerability_provider_ids(
            recipient_actor_id,
            target_actor_id,
            rule.effect_id,
        );
        let mut providers = provider_ids.into_iter();
        let provider_actor_id = providers.next()?;
        if providers.next().is_some() {
            return None;
        }

        Some(ExactDamageContributionEvent {
            observed_micros: envelope.time.observed_micros,
            effect_id: rule.effect_id,
            provider_actor_id,
            recipient_actor_id,
            amount: active_observed_damage.checked_sub(inactive_observed_damage)?,
            observed_damage: damage.amount,
            included: true,
        })
    }

    fn formula_context_sha256(
        &self,
        actor_id: u64,
        excluded_effect_id: i64,
        ignored_effect_ids: &[i64],
    ) -> Option<String> {
        let attributes = self.formula_attributes_by_actor.get(&actor_id)?;
        let mut statuses = self
            .formula_statuses
            .iter()
            .filter(|(key, _)| {
                key.target_actor_id == actor_id
                    && key.effect_id != excluded_effect_id
                    && !ignored_effect_ids.contains(&key.effect_id)
            })
            .map(|(_, value)| *value)
            .collect::<Vec<_>>();
        statuses.sort_unstable();

        let mut hasher = Sha256::new();
        hasher.update(b"rlogs-bpsr-formula-context-v1\0");
        for (attribute_id, value) in attributes {
            if *attribute_id == ATTR_CURRENT_HP {
                continue;
            }
            hasher.update(b"A");
            hasher.update(i64::from(*attribute_id).to_le_bytes());
            hasher.update(value.to_le_bytes());
        }
        for status in statuses {
            hasher.update(b"S");
            hasher.update(status.effect_id.to_le_bytes());
            hasher.update(status.stacks.to_le_bytes());
            hasher.update(status.level.to_le_bytes());
        }
        Some(format!("sha256:{:x}", hasher.finalize()))
    }

    fn formula_context_debug(
        &self,
        actor_id: u64,
        excluded_effect_id: i64,
        ignored_effect_ids: &[i64],
    ) -> (Vec<(i32, i64)>, Vec<FormulaStatusValue>) {
        let attributes = self
            .formula_attributes_by_actor
            .get(&actor_id)
            .into_iter()
            .flat_map(|attributes| attributes.iter())
            .filter(|(attribute_id, _)| **attribute_id != ATTR_CURRENT_HP)
            .map(|(attribute_id, value)| (*attribute_id, *value))
            .collect::<Vec<_>>();
        let mut statuses = self
            .formula_statuses
            .iter()
            .filter(|(key, _)| {
                key.target_actor_id == actor_id
                    && key.effect_id != excluded_effect_id
                    && !ignored_effect_ids.contains(&key.effect_id)
            })
            .map(|(_, value)| *value)
            .collect::<Vec<_>>();
        statuses.sort_unstable();
        (attributes, statuses)
    }

    fn target_vulnerability_rational_contribution(
        &self,
        envelope: &EventEnvelope,
        damage: &rlogs_events::DamageEvent,
        rule: &TargetVulnerabilityRdpsRule,
    ) -> Option<ExactRationalDamageContributionEvent> {
        if rule.projection != TargetVulnerabilityProjection::RationalObservedOutput {
            return None;
        }
        if damage.amount <= 0
            || damage.ability.map(|ability| ability.0) != Some(rule.ability_id)
            || damage.hit_event_id != Some(rule.hit_event_id)
            || !rule.required_critical.matches(damage.flags.critical)
            || damage.flags.lucky != Some(rule.required_lucky)
        {
            return None;
        }

        let recipient_actor_id = damage.source.actor_id.0;
        let target_actor_id = damage.target.actor_id.0;
        let (provider_ids, _) = self.target_vulnerability_provider_ids(
            recipient_actor_id,
            target_actor_id,
            rule.effect_id,
        );
        let mut providers = provider_ids.into_iter();
        let provider_actor_id = providers.next()?;
        if providers.next().is_some() {
            return None;
        }

        Some(ExactRationalDamageContributionEvent {
            observed_micros: envelope.time.observed_micros,
            effect_id: rule.effect_id,
            provider_actor_id,
            recipient_actor_id,
            numerator: i128::from(damage.amount)
                .checked_mul(i128::from(rule.provider_raw_delta))?,
            denominator: i128::from(rule.active_factor()?),
            observed_damage: damage.amount,
            included: true,
        })
    }

    fn target_vulnerability_audit_gate(&self, damage: &rlogs_events::DamageEvent) -> &'static str {
        let Ok(catalog) = target_vulnerability_rdps_catalog() else {
            return "ability_mismatch";
        };
        let Some(ability_id) = damage.ability.map(|ability| ability.0) else {
            return "ability_mismatch";
        };
        let rule_indices = catalog.rule_indices_for_ability(ability_id);
        if rule_indices.is_empty() {
            return "ability_mismatch";
        }
        if damage.amount <= 0 {
            return "nonpositive_damage";
        }
        if !rule_indices
            .iter()
            .any(|index| damage.hit_event_id == Some(catalog.rules[*index].hit_event_id))
        {
            return "hit_event_mismatch";
        }
        if !rule_indices.iter().any(|index| {
            let rule = &catalog.rules[*index];
            damage.hit_event_id == Some(rule.hit_event_id)
                && rule.required_critical.matches(damage.flags.critical)
        }) {
            return "critical_flag_mismatch";
        }
        let Some(rule) = rule_indices.iter().find_map(|index| {
            let rule = &catalog.rules[*index];
            (damage.hit_event_id == Some(rule.hit_event_id)
                && rule.required_critical.matches(damage.flags.critical)
                && damage.flags.lucky == Some(rule.required_lucky))
            .then_some(rule)
        }) else {
            return "lucky_flag_mismatch";
        };

        let recipient_actor_id = damage.source.actor_id.0;
        let target_actor_id = damage.target.actor_id.0;
        if rule.projection == TargetVulnerabilityProjection::PairedObservedOutput {
            if damage.amount == rule.inactive_observed_damage.unwrap_or_default() {
                return "paired_inactive_observed_witness";
            }
            if damage.amount != rule.active_observed_damage.unwrap_or_default() {
                return "paired_observed_damage_mismatch";
            }
            if self.class_id_by_actor.get(&recipient_actor_id).copied()
                != rule.required_source_class_id
            {
                return "paired_source_class_mismatch";
            }
            if self
                .formula_context_sha256(
                    recipient_actor_id,
                    rule.effect_id,
                    &rule.ignored_context_effect_ids,
                )
                .as_deref()
                != rule.required_source_context_sha256.as_deref()
            {
                return "paired_source_context_mismatch";
            }
            let target_context = self.formula_context_sha256(
                target_actor_id,
                rule.effect_id,
                &rule.ignored_context_effect_ids,
            );
            if !target_context.as_ref().is_some_and(|context| {
                rule.allowed_target_context_sha256
                    .iter()
                    .any(|allowed| allowed == context)
            }) {
                return "paired_target_context_mismatch";
            }
        }
        let (providers, has_same_wire_provider) = self.target_vulnerability_provider_ids(
            recipient_actor_id,
            target_actor_id,
            rule.effect_id,
        );
        match providers.len() {
            0 => "no_external_active_provider",
            1 if has_same_wire_provider => "candidate_same_wire_provider",
            1 => "candidate_active_window_provider",
            _ => "multiple_external_active_providers",
        }
    }

    fn target_vulnerability_provider_ids(
        &self,
        recipient_actor_id: u64,
        target_actor_id: u64,
        effect_id: i64,
    ) -> (HashSet<u64>, bool) {
        let active_window_providers = self
            .target_vulnerability_windows
            .keys()
            .filter(|key| key.target_actor_id == target_actor_id && key.effect_id == effect_id)
            .map(|key| key.provider_actor_id);
        let transition_providers = self
            .target_vulnerability_transitions
            .iter()
            .filter(|key| key.target_actor_id == target_actor_id && key.effect_id == effect_id)
            .map(|key| key.provider_actor_id)
            .collect::<Vec<_>>();
        let has_same_wire_provider = transition_providers.iter().any(|provider_actor_id| {
            let provider_actor_id = self.resolve_owner_actor_id(*provider_actor_id);
            provider_actor_id != recipient_actor_id
                && self.active_players.contains(&provider_actor_id)
        });
        let providers = active_window_providers
            .chain(transition_providers)
            .map(|provider_actor_id| self.resolve_owner_actor_id(provider_actor_id))
            .filter(|provider_actor_id| *provider_actor_id != recipient_actor_id)
            .filter(|provider_actor_id| self.active_players.contains(provider_actor_id))
            .collect::<HashSet<_>>();
        (providers, has_same_wire_provider)
    }

    pub fn target_vulnerability_audit(&self) -> Option<TargetVulnerabilityAudit> {
        self.last_target_vulnerability_audit
    }

    /// Analysis-only detail for the replay audit. The live projector keeps the
    /// same constant-time attribution path; this merely exposes the retained
    /// target/provider identities when a strict packet candidate is rejected.
    pub fn target_vulnerability_audit_detail(&self, damage: &rlogs_events::DamageEvent) -> String {
        let rule = target_vulnerability_rdps_catalog()
            .ok()
            .and_then(|catalog| {
                catalog
                    .rules
                    .iter()
                    .find(|rule| damage.ability.map(|ability| ability.0) == Some(rule.ability_id))
            });
        let effect_id = rule.map(|rule| rule.effect_id);
        let ignored_effect_ids = rule
            .map(|rule| rule.ignored_context_effect_ids.as_slice())
            .unwrap_or_default();
        let transitions = self
            .target_vulnerability_transitions
            .iter()
            .filter(|key| effect_id == Some(key.effect_id))
            .map(|key| {
                (
                    key.target_actor_id,
                    key.provider_actor_id,
                    self.resolve_owner_actor_id(key.provider_actor_id),
                )
            })
            .collect::<Vec<_>>();
        let windows = self
            .target_vulnerability_windows
            .keys()
            .filter(|key| effect_id == Some(key.effect_id))
            .map(|key| {
                (
                    key.target_actor_id,
                    key.provider_actor_id,
                    self.resolve_owner_actor_id(key.provider_actor_id),
                    key.instance_id,
                )
            })
            .collect::<Vec<_>>();
        let source_context = effect_id.and_then(|effect_id| {
            self.formula_context_sha256(damage.source.actor_id.0, effect_id, ignored_effect_ids)
        });
        let target_context = effect_id.and_then(|effect_id| {
            self.formula_context_sha256(damage.target.actor_id.0, effect_id, ignored_effect_ids)
        });
        let source_state = effect_id.map(|effect_id| {
            self.formula_context_debug(damage.source.actor_id.0, effect_id, ignored_effect_ids)
        });
        let target_state = effect_id.map(|effect_id| {
            self.formula_context_debug(damage.target.actor_id.0, effect_id, ignored_effect_ids)
        });
        format!(
            "effect={effect_id:?} amount={} source={} source_class={:?} source_context={source_context:?} source_state={source_state:?} target={} target_context={target_context:?} target_state={target_state:?} transitions={transitions:?} windows={windows:?}",
            damage.amount,
            damage.source.actor_id.0,
            self.class_id_by_actor.get(&damage.source.actor_id.0),
            damage.target.actor_id.0,
        )
    }

    /// Analysis-only gate for Arcane! Fatal Spiral's packet-proven external
    /// all-element state. This deliberately stops before attribution: the
    /// provider, recipient, tier delta, and affected elemental hit are proven,
    /// but the client's integer damage-stage inversion is not yet proven.
    pub fn fatal_spiral_audit_gate(&self, damage: &rlogs_events::DamageEvent) -> &'static str {
        if !self.runtime_applicable {
            return "runtime_mismatch";
        }
        if damage.amount <= 0 {
            return "nonpositive_damage";
        }
        let Some(ability_id) = damage.ability.map(|ability| ability.0) else {
            return "missing_damage_id";
        };
        if self
            .runtime
            .highland_blood
            .excluded_provider_owned_damage_ids
            .contains(&ability_id)
        {
            return "excluded_provider_owned_damage";
        }
        let Some(property) = damage.packet.property else {
            return "missing_damage_property";
        };
        if !(1..=8).contains(&property) {
            return "non_elemental_damage_property";
        }

        let recipient_actor_id = damage.source.actor_id.0;
        let recipient_entity_uuid = damage.source.entity_uuid.0;
        let state_actor_id = self.fatal_spiral_state_actor_id(damage);
        let providers = self.fatal_spiral_provider_windows_for_recipient(
            recipient_actor_id,
            recipient_entity_uuid,
            state_actor_id,
        );
        if providers.is_empty() {
            return "no_external_active_provider";
        }
        if providers.len() != 1 {
            return "multiple_external_active_providers";
        }
        let (provider_actor_id, provider_entity_uuid) = providers[0];

        let state_basis_points = state_actor_id
            .and_then(|actor_id| self.states.get(&actor_id))
            .and_then(|state| {
                (state.all_element.provider_basis_points.len() == 1)
                    .then(|| state.all_element.provider_basis_points.iter().next())
                    .flatten()
            })
            .and_then(|(&state_provider, &basis_points)| {
                (state_provider == provider_actor_id).then_some(basis_points)
            });
        if self
            .fatal_spiral_ambiguous_provider_entities
            .contains(&provider_entity_uuid)
        {
            return "ambiguous_provider_magnitude";
        }
        let Some(basis_points) = state_basis_points.or_else(|| {
            self.fatal_spiral_provider_basis_points_by_entity_uuid
                .get(&provider_entity_uuid)
                .copied()
        }) else {
            return "missing_exact_provider_magnitude";
        };
        if !self
            .runtime
            .highland_blood
            .packet_proven_raw_deltas
            .contains(&basis_points)
        {
            return "unproven_recipient_delta";
        }

        "damage_stage_unproven"
    }

    pub fn fatal_spiral_audit_detail(&self, damage: &rlogs_events::DamageEvent) -> String {
        let recipient_actor_id = damage.source.actor_id.0;
        let recipient_entity_uuid = damage.source.entity_uuid.0;
        let state_actor_id = self.fatal_spiral_state_actor_id(damage);
        let providers = self.fatal_spiral_provider_windows_for_recipient(
            recipient_actor_id,
            recipient_entity_uuid,
            state_actor_id,
        );
        let state_providers = state_actor_id
            .and_then(|actor_id| self.states.get(&actor_id))
            .map(|state| state.all_element.provider_basis_points.clone())
            .unwrap_or_default();
        let state_family = state_actor_id
            .and_then(|actor_id| self.states.get(&actor_id))
            .map(|state| {
                (
                    state.all_element.current_value,
                    state.all_element.total_value,
                    state.all_element.add_value,
                    state.all_element.extra_add_value,
                    state.all_element.percent_value,
                    state.all_element.extra_percent_value,
                )
            });
        let cached_provider_basis_points = providers
            .iter()
            .map(|&(provider_actor_id, provider_entity_uuid)| {
                (
                    provider_actor_id,
                    provider_entity_uuid,
                    self.fatal_spiral_provider_basis_points_by_entity_uuid
                        .get(&provider_entity_uuid)
                        .copied(),
                    self.fatal_spiral_ambiguous_provider_entities
                        .contains(&provider_entity_uuid),
                )
            })
            .collect::<Vec<_>>();
        format!(
            "ability={:?} property={:?} source={} source_entity={} state_actor={state_actor_id:?} target={} window_providers={providers:?} cached_provider_basis_points={cached_provider_basis_points:?} state_provider_basis_points={state_providers:?} all_element_family={state_family:?} amount={}",
            damage.ability.map(|ability| ability.0),
            damage.packet.property,
            recipient_actor_id,
            damage.source.entity_uuid.0,
            damage.target.actor_id.0,
            damage.amount,
        )
    }

    fn fatal_spiral_state_actor_id(&self, damage: &rlogs_events::DamageEvent) -> Option<u64> {
        let recipient_actor_id = damage.source.actor_id.0;
        if self.states.contains_key(&recipient_actor_id) {
            return Some(recipient_actor_id);
        }
        let entity_uuid = damage.source.entity_uuid.0;
        (entity_uuid != 0)
            .then(|| {
                self.attribute_state_actor_by_entity_uuid
                    .get(&entity_uuid)
                    .copied()
            })
            .flatten()
    }

    fn fatal_spiral_provider_windows_for_recipient(
        &self,
        recipient_actor_id: u64,
        recipient_entity_uuid: i64,
        state_actor_id: Option<u64>,
    ) -> Vec<(u64, i64)> {
        let mut providers = self
            .fatal_spiral_windows
            .iter()
            .filter(|key| {
                key.target_actor_id == recipient_actor_id
                    || state_actor_id == Some(key.target_actor_id)
                    || (recipient_entity_uuid != 0
                        && key.target_entity_uuid == recipient_entity_uuid)
            })
            .map(|key| {
                (
                    self.resolve_owner_actor_id(key.provider_actor_id),
                    key.provider_entity_uuid,
                )
            })
            .filter(|(provider_actor_id, _)| *provider_actor_id != recipient_actor_id)
            .filter(|(provider_actor_id, _)| self.active_players.contains(provider_actor_id))
            .collect::<Vec<_>>();
        providers.sort_unstable();
        providers.dedup();
        providers
    }

    fn resolve_owner_actor_id(&self, actor_id: u64) -> u64 {
        self.actor_ancestry
            .resolve_actor_id_at(actor_id, self.latest_observed_micros)
    }

    fn expire_target_vulnerability_windows(&mut self, observed_micros: u64) {
        self.target_vulnerability_windows.retain(|_, window| {
            window
                .expires_at_observed_micros
                .is_none_or(|expires_at| expires_at > observed_micros)
        });
    }

    fn clear_actor(&mut self, actor_id: u64) {
        self.unresolved_status_windows
            .retain(|window| window.target_actor_id != actor_id);
        self.states.remove(&actor_id);
        self.staged_states.remove(&actor_id);
        self.team_luck_critical_ever_observed.remove(&actor_id);
        self.team_luck_lucky_ever_observed.remove(&actor_id);
        self.team_luck_critical_cleared_by_snapshot
            .remove(&actor_id);
        self.team_luck_lucky_cleared_by_snapshot.remove(&actor_id);
        if let Some(entity_uuid) = self.attribute_state_entity_uuid_by_actor.remove(&actor_id) {
            if self.attribute_state_actor_by_entity_uuid.get(&entity_uuid) == Some(&actor_id) {
                self.attribute_state_actor_by_entity_uuid
                    .remove(&entity_uuid);
            }
        }
        self.effect_windows
            .retain(|key, _| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.team_luck_windows
            .retain(|key| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.functional_amp_windows
            .retain(|key| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.functional_amp_transition_wires.remove(&actor_id);
        self.mechanical_power_windows
            .retain(|key| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.mechanical_power_transition_wires.remove(&actor_id);
        self.mechanical_power_primary_transition_witnesses
            .retain(|key, _| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.harmony_grace_windows
            .retain(|key| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.harmony_grace_transition_wires.remove(&actor_id);
        self.harmony_grace_primary_transition_witnesses
            .retain(|key, _| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.thunderwind_windows
            .retain(|key, _| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.thunderwind_child_targets.remove(&actor_id);
        self.thunderwind_transition_wires.remove(&actor_id);
        self.full_bloom_targets.remove(&actor_id);
        self.inspiration_windows
            .retain(|key, _| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.inspiration_transition_wires.remove(&actor_id);
        self.inspiration_snapshot_targets.remove(&actor_id);
        self.fatal_spiral_windows
            .retain(|key| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.fatal_spiral_transitions.remove(&actor_id);
        self.fatal_spiral_snapshot_targets.remove(&actor_id);
        self.target_vulnerability_windows
            .retain(|key, _| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.target_vulnerability_transitions
            .retain(|key| key.target_actor_id != actor_id && key.provider_actor_id != actor_id);
        self.formula_attributes_by_actor.remove(&actor_id);
        self.formula_statuses
            .retain(|key, _| key.target_actor_id != actor_id && key.source_actor_id != actor_id);
        self.entity_type_by_actor.remove(&actor_id);
        self.summon_config_by_actor.remove(&actor_id);
        self.class_id_by_actor.remove(&actor_id);
        self.observed_ability_ids_by_actor.remove(&actor_id);
    }

    fn clear_run_state(&mut self) {
        self.current_wire = None;
        self.states.clear();
        self.staged_states.clear();
        self.attribute_state_actor_by_entity_uuid.clear();
        self.attribute_state_entity_uuid_by_actor.clear();
        self.team_luck_critical_ever_observed.clear();
        self.team_luck_lucky_ever_observed.clear();
        self.team_luck_critical_cleared_by_snapshot.clear();
        self.team_luck_lucky_cleared_by_snapshot.clear();
        self.effect_windows.clear();
        self.team_luck_windows.clear();
        self.team_luck_transition_wire = None;
        self.functional_amp_windows.clear();
        self.functional_amp_transition_wires.clear();
        self.mechanical_power_windows.clear();
        self.mechanical_power_transition_wires.clear();
        self.mechanical_power_primary_transition_witnesses.clear();
        self.harmony_grace_windows.clear();
        self.harmony_grace_transition_wires.clear();
        self.harmony_grace_primary_transition_witnesses.clear();
        self.thunderwind_windows.clear();
        self.thunderwind_child_targets.clear();
        self.thunderwind_transition_wires.clear();
        self.full_bloom_targets.clear();
        self.inspiration_windows.clear();
        self.inspiration_transition_wires.clear();
        self.inspiration_snapshot_targets.clear();
        self.fatal_spiral_windows.clear();
        self.fatal_spiral_transitions.clear();
        self.fatal_spiral_snapshot_targets.clear();
        self.fatal_spiral_provider_basis_points_by_entity_uuid
            .clear();
        self.fatal_spiral_ambiguous_provider_entities.clear();
        self.target_vulnerability_windows.clear();
        self.target_vulnerability_transitions.clear();
        self.formula_attributes_by_actor.clear();
        self.formula_statuses.clear();
        self.unresolved_status_windows.clear();
        self.actor_ancestry.clear();
        self.entity_type_by_actor.clear();
        self.summon_config_by_actor.clear();
        self.observed_ability_ids_by_actor.clear();
    }

    fn clear_state(&mut self) {
        self.clear_run_state();
        self.class_id_by_actor.clear();
        self.active_players.clear();
    }
}

fn attack_lane_matches(lane: PrimaryAttackLane, stat: OffensiveStatKind) -> bool {
    matches!(
        (lane, stat),
        (
            PrimaryAttackLane::PhysicalAttack,
            OffensiveStatKind::PhysicalAttack
        ) | (
            PrimaryAttackLane::MagicalAttack,
            OffensiveStatKind::MagicalAttack
        )
    )
}

fn fixed_point_stage_term(value: i64, raw_percent: i64) -> Option<i64> {
    let product = i128::from(value).checked_mul(i128::from(raw_percent))?;
    i64::try_from(product.checked_div(i128::from(BPSR_FIXED_POINT_SCALE))?).ok()
}

fn fixed_point_stage_term_nearest(value: i64, raw_percent: i64) -> Option<i64> {
    if value < 0 || raw_percent < 0 {
        return None;
    }
    let product = i128::from(value).checked_mul(i128::from(raw_percent))?;
    let half = i128::from(BPSR_FIXED_POINT_SCALE / 2);
    i64::try_from(
        product
            .checked_add(half)?
            .checked_div(i128::from(BPSR_FIXED_POINT_SCALE))?,
    )
    .ok()
}

#[cfg(test)]
fn fixed_point_stage_term_nearest_non_tie(value: i64, raw_percent: i64) -> Option<i64> {
    if value < 0 || raw_percent < 0 {
        return None;
    }
    let scale = i128::from(BPSR_FIXED_POINT_SCALE);
    let product = i128::from(value).checked_mul(i128::from(raw_percent))?;
    if product.checked_rem(scale)? == scale / 2 {
        return None;
    }
    let half = scale / 2;
    i64::try_from(product.checked_add(half)?.checked_div(scale)?).ok()
}

fn attack_family_for_lane(state: &ActorHpState, lane: PrimaryAttackLane) -> &AttackFamilyState {
    match lane {
        PrimaryAttackLane::PhysicalAttack => &state.physical_attack,
        PrimaryAttackLane::MagicalAttack => &state.magical_attack,
    }
}

#[allow(clippy::too_many_arguments)]
fn exact_attack_family_stage_contribution(
    observed_micros: u64,
    effect_id: i64,
    provider_actor_id: u64,
    recipient_actor_id: u64,
    observed_damage: i64,
    family: &AttackFamilyState,
    provider_base_add: i64,
    provider_raw_percent: i64,
    selected: SelectedDamageStage,
) -> Option<ExactRationalDamageContributionEvent> {
    if observed_damage <= 0
        || effect_id <= 0
        || provider_actor_id == recipient_actor_id
        || (provider_base_add <= 0) == (provider_raw_percent <= 0)
    {
        // Exactly one packet-proven provider component must be selected.
        // A zero-component or mixed-component call would make the stage
        // boundary ambiguous and could double count an attribute cross-term.
        return None;
    }
    let current_attack = family.final_value?;
    let provider_attack_marginal =
        exact_packet_attack_provider_marginal(family, provider_base_add, provider_raw_percent)?;
    let (numerator, denominator) = exact_external_attack_coefficient_stage_fraction(
        observed_damage,
        PacketDamageScriptFamily::StandardAttack,
        current_attack,
        provider_attack_marginal,
        selected.coefficient_basis_points,
        selected.fixed_parameter,
    )?;
    Some(ExactRationalDamageContributionEvent {
        observed_micros,
        effect_id,
        provider_actor_id,
        recipient_actor_id,
        numerator,
        denominator,
        observed_damage,
        included: true,
    })
}

fn exact_packet_attack_provider_marginal(
    family: &AttackFamilyState,
    provider_base_add: i64,
    provider_raw_percent: i64,
) -> Option<i64> {
    if (provider_base_add <= 0) == (provider_raw_percent <= 0) {
        return None;
    }
    let current_attack = family.final_value?;
    let intermediate = family.intermediate_value?;
    let base_add = family.base_add?;
    let extra_add = family.extra_add?;
    let raw_percent = family.raw_percent?;
    if packet_attribute_family_value(base_add, raw_percent, 0)? != intermediate
        || packet_attribute_family_value(base_add, raw_percent, extra_add)? != current_attack
    {
        return None;
    }
    packet_attribute_family_provider_marginal(
        base_add,
        raw_percent,
        extra_add,
        provider_base_add,
        provider_raw_percent,
        0,
    )
}

fn exact_primary_to_attack_provider_base_add_from_witness(
    primary_current: i64,
    primary_provider_marginal: i64,
    conversion_numerator: i64,
    conversion_denominator: i64,
) -> Option<i64> {
    if primary_current <= 0 || primary_provider_marginal <= 0 {
        return None;
    }
    let primary_without_provider = primary_current.checked_sub(primary_provider_marginal)?;
    let current_attack_component = checked_positive_floor_ratio(
        primary_current,
        conversion_numerator,
        conversion_denominator,
    )?;
    let attack_component_without_provider = checked_positive_floor_ratio(
        primary_without_provider,
        conversion_numerator,
        conversion_denominator,
    )?;
    current_attack_component.checked_sub(attack_component_without_provider)
}

fn attribute_family_rounding_name(rounding: AttributeFamilyRounding) -> &'static str {
    match rounding {
        AttributeFamilyRounding::Floor => "floor",
        AttributeFamilyRounding::NearestNonTie => "nearest_non_tie",
    }
}

#[allow(clippy::too_many_arguments)]
fn exact_team_luck_accounting_fraction(
    observed_damage: i64,
    critical: bool,
    lucky: bool,
    critical_damage_raw: i64,
    lucky_damage_raw: i64,
    critical_raw_delta: i64,
    lucky_raw_delta: i64,
    combined_critical_lucky_enabled: bool,
    critical_damage_factor_interpretation: CriticalDamageFactorInterpretation,
) -> Option<(i128, i128)> {
    if observed_damage <= 0 || (!critical && !lucky) {
        return None;
    }
    // A combined critical+Lucky packet requires a proven ordering for the two
    // integer floor stages. No externally supplied Team Luck sample currently
    // exercises that path, so retain the damage without projecting it.
    if critical && lucky && !combined_critical_lucky_enabled {
        return None;
    }
    // No combined-outcome algorithm is present in the runtime pack yet. The
    // validation gate rejects `true`, so reaching this branch is impossible
    // until a future generic ordered-stage implementation is promoted.
    if critical && lucky {
        return None;
    }
    if critical {
        exact_external_critical_damage_fraction(
            observed_damage,
            critical_damage_raw,
            critical_raw_delta,
            critical_damage_factor_interpretation,
        )
    } else {
        exact_external_lucky_damage_fraction(observed_damage, lucky_damage_raw, lucky_raw_delta)
    }
}

fn scale_later_rational_marginal_after_many(
    earlier: &[ExactRationalDamageContributionEvent],
    mut later: ExactRationalDamageContributionEvent,
) -> Option<ExactRationalDamageContributionEvent> {
    if earlier.is_empty() || later.numerator <= 0 || later.denominator <= 0 {
        return None;
    }
    let mut sum_numerator = 0_i128;
    let mut sum_denominator = 1_i128;
    for contribution in earlier {
        if contribution.observed_damage != later.observed_damage
            || contribution.recipient_actor_id != later.recipient_actor_id
            || contribution.numerator <= 0
            || contribution.denominator <= 0
        {
            return None;
        }
        let shared = greatest_common_divisor(sum_denominator, contribution.denominator);
        let left_scale = contribution.denominator.checked_div(shared)?;
        let right_scale = sum_denominator.checked_div(shared)?;
        sum_numerator = sum_numerator
            .checked_mul(left_scale)?
            .checked_add(contribution.numerator.checked_mul(right_scale)?)?;
        sum_denominator = sum_denominator.checked_mul(left_scale)?;
        let reduce = greatest_common_divisor(sum_numerator, sum_denominator);
        sum_numerator = sum_numerator.checked_div(reduce)?;
        sum_denominator = sum_denominator.checked_div(reduce)?;
    }
    let observed_scaled = i128::from(later.observed_damage).checked_mul(sum_denominator)?;
    let remaining_scaled = observed_scaled.checked_sub(sum_numerator)?;
    if remaining_scaled <= 0 {
        return None;
    }
    let numerator = later.numerator.checked_mul(remaining_scaled)?;
    let denominator = later.denominator.checked_mul(observed_scaled)?;
    let divisor = greatest_common_divisor(numerator, denominator);
    later.numerator = numerator.checked_div(divisor)?;
    later.denominator = denominator.checked_div(divisor)?;
    Some(later)
}

fn greatest_common_divisor(mut left: i128, mut right: i128) -> i128 {
    left = left.abs();
    right = right.abs();
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn checked_positive_floor_ratio(value: i64, numerator: i64, denominator: i64) -> Option<i64> {
    if value < 0 || numerator <= 0 || denominator <= 0 {
        return None;
    }
    let scaled = i128::from(value).checked_mul(i128::from(numerator))?;
    i64::try_from(scaled.checked_div(i128::from(denominator))?).ok()
}

impl ExactDamageContributionProjector for BpsrStateDamageContributionProjector {
    fn enabled(&self) -> bool {
        self.runtime_applicable
    }

    fn formula_identity(&self) -> Option<&str> {
        Some(state_damage_contribution_formula_identity())
    }

    fn status(&self) -> String {
        match (
            self.observed_deployment_id.as_deref(),
            self.observed_client_build.as_deref(),
            self.observed_protocol_pack_digest.as_deref(),
        ) {
            (Some(deployment_id), Some(client_build), Some(protocol_pack_digest))
                if runtime_matches_event_identity(
                    self.runtime,
                    deployment_id,
                    client_build,
                    protocol_pack_digest,
                ) && self.runtime.has_any_runtime_transfer_enabled() =>
            {
                "partial_packet_proven_rules".into()
            }
            (Some(deployment_id), Some(client_build), Some(protocol_pack_digest))
                if runtime_matches_event_identity(
                    self.runtime,
                    deployment_id,
                    client_build,
                    protocol_pack_digest,
                ) =>
            {
                format!(
                    "formula_pack_blocked: formula={}/{}; blockers={}",
                    self.runtime.deployment_id,
                    self.runtime.game_build,
                    self.runtime.promotion_blocker_status_detail()
                )
            }
            (Some(deployment_id), Some(client_build), Some(protocol_pack_digest))
                if deployment_id == self.runtime.deployment_id
                    && client_build == self.runtime.game_build
                    && self.runtime.warns_on_build_mismatch() =>
            {
                format!(
                    "formula_pack_incompatible: formula={}/{}@{}; game={}/{}@{}; exact protocol-pack attribution required",
                    self.runtime.deployment_id,
                    self.runtime.game_build,
                    self.runtime.protocol_pack_digest,
                    deployment_id,
                    client_build,
                    protocol_pack_digest,
                )
            }
            (Some(deployment_id), Some(client_build), Some(_))
                if deployment_id == self.runtime.deployment_id
                    && self.runtime.warns_on_build_mismatch() =>
            {
                format!(
                    "formula_pack_incompatible: formula={}/{}; game={}/{}; exact-build attribution required",
                    self.runtime.deployment_id,
                    self.runtime.game_build,
                    deployment_id,
                    client_build
                )
            }
            (Some(deployment_id), Some(client_build), Some(_)) => format!(
                "formula_pack_incompatible: formula={}/{}; game={}/{}",
                self.runtime.deployment_id, self.runtime.game_build, deployment_id, client_build
            ),
            _ => "waiting_for_client_build".into(),
        }
    }

    fn reset(&mut self) {
        self.clear_state();
        self.runtime_applicable = false;
        self.observed_deployment_id = None;
        self.observed_client_build = None;
        self.observed_protocol_pack_digest = None;
    }

    fn observe(
        &mut self,
        envelope: &EventEnvelope,
        output: &mut Vec<ExactDamageContributionEvent>,
        rational_output: &mut Vec<ExactRationalDamageContributionEvent>,
    ) {
        self.observe_timeline(envelope, output, rational_output);
    }
}

fn runtime_matches_event_identity(
    runtime: &RdpsRuntimeConfig,
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
) -> bool {
    deployment_id == runtime.deployment_id
        && client_build == runtime.game_build
        && protocol_pack_digest == runtime.protocol_pack_digest
}

fn wire_key(envelope: &EventEnvelope) -> Option<WireKey> {
    match envelope.provenance.source {
        EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } => Some(WireKey {
            connection_id,
            stream_id,
            capture_sequence,
        }),
        _ => None,
    }
}

fn update_attack_family_attribute(
    family: &mut AttackFamilyState,
    config: AttackFamilyRuntimeConfig,
    attribute_id: i32,
    value: i64,
) {
    if attribute_id == config.final_attribute_id {
        family.final_value = Some(value);
    } else if attribute_id == config.intermediate_attribute_id {
        family.intermediate_value = Some(value);
    } else if attribute_id == config.base_add_attribute_id {
        family.base_add = Some(value);
    } else if attribute_id == config.extra_add_attribute_id {
        family.extra_add = Some(value);
    } else if attribute_id == config.raw_percent_attribute_id {
        family.raw_percent = Some(value);
        family.raw_percent_packet_observed = true;
    }
}

fn reconcile_external_percent_family(
    previous_percent: Option<i64>,
    family: &mut AttackFamilyState,
    desired: &BTreeMap<u64, i64>,
) {
    let (Some(previous), Some(current)) = (previous_percent, family.raw_percent) else {
        return;
    };
    if previous == current {
        return;
    }
    let current_total = family.provider_raw_percent.values().copied().sum::<i64>();
    let desired_total = desired.values().copied().sum::<i64>();
    if desired_total.checked_sub(current_total) == current.checked_sub(previous) {
        family.provider_raw_percent.clone_from(desired);
    } else {
        // An unrelated loadout or effect changed the same family. Retain every
        // packet event but invalidate external decomposition until a later
        // exact transition re-establishes it.
        family.provider_raw_percent.clear();
    }
}

fn exact_primary_stat_transition_witness(
    wire: Option<WireKey>,
    previous: &AttackFamilyState,
    current: &AttackFamilyState,
    desired: &BTreeMap<u64, i64>,
) -> Option<(u64, PrimaryStatTransitionWitness)> {
    let wire = wire?;
    if current.provider_raw_percent != *desired || desired.is_empty() {
        return None;
    }
    let (
        Some(previous_base),
        Some(current_base),
        Some(previous_extra),
        Some(current_extra),
        Some(previous_raw),
        Some(current_raw),
        Some(previous_intermediate),
        Some(current_intermediate),
        Some(previous_final),
        Some(current_final),
    ) = (
        previous.base_add,
        current.base_add,
        previous.extra_add,
        current.extra_add,
        previous.raw_percent,
        current.raw_percent,
        previous.intermediate_value,
        current.intermediate_value,
        previous.final_value,
        current.final_value,
    )
    else {
        return None;
    };
    if previous_base != current_base || previous_extra != current_extra {
        return None;
    }

    let mut added_provider = None;
    let mut provider_delta_total = 0_i64;
    for (&provider_actor_id, &current_value) in desired {
        let previous_value = previous
            .provider_raw_percent
            .get(&provider_actor_id)
            .copied()
            .unwrap_or_default();
        let delta = current_value.checked_sub(previous_value)?;
        if delta < 0 {
            return None;
        }
        if delta > 0 {
            if added_provider.replace((provider_actor_id, delta)).is_some() {
                return None;
            }
            provider_delta_total = provider_delta_total.checked_add(delta)?;
        }
    }
    if previous
        .provider_raw_percent
        .keys()
        .any(|provider| !desired.contains_key(provider))
    {
        return None;
    }
    let (provider_actor_id, provider_raw_percent) = added_provider?;
    if provider_delta_total != provider_raw_percent
        || current_raw.checked_sub(previous_raw)? != provider_raw_percent
    {
        return None;
    }
    let provider_primary_marginal = current_intermediate.checked_sub(previous_intermediate)?;
    if provider_primary_marginal <= 0
        || current_final.checked_sub(previous_final)? != provider_primary_marginal
    {
        return None;
    }
    Some((
        provider_actor_id,
        PrimaryStatTransitionWitness {
            wire,
            instance_id: None,
            base_add: current_base,
            active_raw_percent: current_raw,
            provider_raw_percent,
            provider_primary_marginal,
        },
    ))
}

/// Re-evaluates a packet-proven provider raw-percent delta against the current
/// complete primary family. This is used only by the bounded tier-0 audit after
/// another additive primary-percent effect changes the recipient while the
/// same Mechanical Power lifecycle remains active.
fn rebase_primary_stat_transition_witness(
    current: &AttackFamilyState,
    mut witness: PrimaryStatTransitionWitness,
) -> Option<PrimaryStatTransitionWitness> {
    let base_add = current.base_add?;
    let raw_percent = current.raw_percent?;
    let extra_add = current.extra_add?;
    let intermediate = current.intermediate_value?;
    let final_value = current.final_value?;
    let provider_removed_raw_percent = raw_percent.checked_sub(witness.provider_raw_percent)?;
    if provider_removed_raw_percent < 0
        || packet_attribute_family_value(base_add, raw_percent, 0)? != intermediate
        || packet_attribute_family_value(base_add, raw_percent, extra_add)? != final_value
    {
        return None;
    }
    let provider_removed_final =
        packet_attribute_family_value(base_add, provider_removed_raw_percent, extra_add)?;
    let provider_primary_marginal = final_value.checked_sub(provider_removed_final)?;
    if provider_primary_marginal <= 0 {
        return None;
    }
    witness.base_add = base_add;
    witness.active_raw_percent = raw_percent;
    witness.provider_primary_marginal = provider_primary_marginal;
    Some(witness)
}

fn complete_exact_raw_percent(family: &mut AttackFamilyState) {
    if family.raw_percent_packet_observed {
        return;
    }
    let (Some(base_add), Some(intermediate)) = (family.base_add, family.intermediate_value) else {
        family.raw_percent = None;
        return;
    };
    if base_add <= 0 || intermediate < 0 {
        family.raw_percent = None;
        return;
    }
    if family.raw_percent.is_some_and(|raw_percent| {
        packet_attribute_family_value(base_add, raw_percent, 0) == Some(intermediate)
    }) {
        return;
    }
    family.raw_percent = None;
    let scale = i128::from(BPSR_FIXED_POINT_SCALE);
    let base = i128::from(base_add);
    let Some(lower_numerator) = i128::from(intermediate).checked_mul(scale) else {
        return;
    };
    let Some(upper_numerator) = i128::from(intermediate)
        .checked_add(1)
        .and_then(|value| value.checked_mul(scale))
    else {
        return;
    };
    let Some(lower) =
        div_ceil_non_negative(lower_numerator, base).and_then(|value| value.checked_sub(scale))
    else {
        return;
    };
    let Some(upper) = div_ceil_non_negative(upper_numerator, base)
        .and_then(|value| value.checked_sub(1))
        .and_then(|value| value.checked_sub(scale))
    else {
        return;
    };
    if lower != upper || lower < 0 {
        return;
    }
    let Ok(raw_percent) = i64::try_from(lower) else {
        return;
    };
    if packet_attribute_family_value(base_add, raw_percent, 0) == Some(intermediate) {
        family.raw_percent = Some(raw_percent);
    }
}

fn div_ceil_non_negative(numerator: i128, denominator: i128) -> Option<i128> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    numerator
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)
}

fn complete_exact_extra_add(family: &mut AttackFamilyState) {
    if family.extra_add.is_some() {
        return;
    }
    let Some(extra_add) = family
        .final_value
        .zip(family.intermediate_value)
        .and_then(|(final_value, intermediate)| final_value.checked_sub(intermediate))
        .filter(|value| *value >= 0)
    else {
        return;
    };
    // This is an algebraic completion of the packet family identity
    // `final = intermediate + extra_add`, not a default-zero assumption.
    family.extra_add = Some(extra_add);
}

fn reconcile_external_percent_family_from_exact_prior(
    previous: &AttackFamilyState,
    current: &mut AttackFamilyState,
    desired: &BTreeMap<u64, i64>,
) {
    if previous.raw_percent.is_some()
        || !current.provider_raw_percent.is_empty()
        || desired.is_empty()
        || previous.base_add != current.base_add
    {
        return;
    }
    let Some(desired_total) = desired.values().try_fold(0_i64, |total, value| {
        (*value > 0).then_some(())?;
        total.checked_add(*value)
    }) else {
        return;
    };
    let Some(previous_percent) = current
        .raw_percent
        .and_then(|value| value.checked_sub(desired_total))
        .filter(|value| *value >= 0)
    else {
        return;
    };
    let (Some(base_add), Some(intermediate), Some(final_value)) = (
        previous.base_add,
        previous.intermediate_value,
        previous.final_value,
    ) else {
        return;
    };
    let Some(extra_add) = final_value
        .checked_sub(intermediate)
        .filter(|value| *value >= 0)
    else {
        return;
    };
    if packet_attribute_family_value(base_add, previous_percent, 0) == Some(intermediate)
        && packet_attribute_family_value(base_add, previous_percent, extra_add) == Some(final_value)
    {
        current.provider_raw_percent.clone_from(desired);
    }
}

fn reconcile_inspiration_state(
    previous: Option<&ActorHpState>,
    next: &mut ActorHpState,
    desired: &BTreeMap<u64, bool>,
    vectors: &[InspirationVectorRuntimeConfig],
) {
    let Some(previous) = previous else {
        next.inspiration_providers.clear();
        return;
    };
    let current = &previous.inspiration_providers;
    let current_modes = current
        .iter()
        .map(|(provider, state)| (*provider, state.provider_full_bloom))
        .collect::<BTreeMap<_, _>>();

    if &current_modes == desired {
        next.inspiration_providers.clone_from(current);
        let stable_vector_lanes = previous.primary_raw_add == next.primary_raw_add
            && previous.critical_chance_raw_add == next.critical_chance_raw_add
            && previous.critical_chance_raw == next.critical_chance_raw
            && previous.lucky_chance_raw_add == next.lucky_chance_raw_add
            && previous.lucky_chance_raw == next.lucky_chance_raw
            && previous.mastery_raw_add == next.mastery_raw_add
            && previous.mastery_raw == next.mastery_raw
            && previous.versatility_raw_add == next.versatility_raw_add
            && previous.versatility_raw == next.versatility_raw
            && previous.external_damage_raw == next.external_damage_raw;
        if !stable_vector_lanes {
            next.inspiration_providers.clear();
            return;
        }
        for state in next.inspiration_providers.values_mut() {
            if option_delta(
                previous.physical_attack.base_add,
                next.physical_attack.base_add,
            ) != Some(0)
            {
                state.physical_attack_base_add_delta = None;
            }
            if option_delta(
                previous.magical_attack.base_add,
                next.magical_attack.base_add,
            ) != Some(0)
            {
                state.magical_attack_base_add_delta = None;
            }
            if option_delta(
                previous.haste_percent_basis_points,
                next.haste_percent_basis_points,
            ) != Some(0)
            {
                state.haste_delta = None;
            }
            if let Some(property_change) =
                option_delta(previous.property_damage_raw, next.property_damage_raw)
                && property_change != 0
            {
                let expected = vector_for_mode(vectors, state.provider_full_bloom)
                    .map(|vector| vector.property_damage_raw_delta);
                if state.property_damage_delta.is_none() && Some(property_change) == expected {
                    // The derived property lane can serialize after Mastery.
                    // Promote it only when its own later packet transition is
                    // exact for the one already-proven provider window.
                    state.property_damage_delta = expected;
                } else {
                    state.property_damage_delta = None;
                }
            }
        }
        return;
    }

    let transition = match (current.len(), desired.len()) {
        (0, 1) => {
            let (&provider_actor_id, &provider_full_bloom) =
                desired.first_key_value().expect("one desired provider");
            vector_for_mode(vectors, provider_full_bloom).and_then(|vector| {
                inspiration_vector_transition(previous, next, vector, 1).map(|mut state| {
                    state.provider_full_bloom = provider_full_bloom;
                    (Some(provider_actor_id), state)
                })
            })
        }
        (1, 0) => {
            let (&provider_actor_id, state) =
                current.first_key_value().expect("one current provider");
            inspiration_state_removal_matches(previous, next, *state)
                .then_some((Some(provider_actor_id), *state))
        }
        (1, 1)
            if current.first_key_value().map(|entry| entry.0)
                == desired.first_key_value().map(|entry| entry.0) =>
        {
            let (&provider_actor_id, old_state) =
                current.first_key_value().expect("one current provider");
            let provider_full_bloom = *desired
                .get(&provider_actor_id)
                .expect("same desired provider");
            vector_for_mode(vectors, provider_full_bloom).and_then(|vector| {
                inspiration_mode_transition(previous, next, *old_state, vector).map(|mut state| {
                    state.provider_full_bloom = provider_full_bloom;
                    (Some(provider_actor_id), state)
                })
            })
        }
        _ => None,
    };

    next.inspiration_providers.clear();
    let Some((provider, state)) = transition else {
        return;
    };
    if desired.is_empty() {
        return;
    }
    if let Some(provider) = provider {
        next.inspiration_providers.insert(provider, state);
    }
}

fn vector_for_mode(
    vectors: &[InspirationVectorRuntimeConfig],
    provider_full_bloom: bool,
) -> Option<InspirationVectorRuntimeConfig> {
    vectors
        .iter()
        .copied()
        .find(|vector| vector.provider_full_bloom == provider_full_bloom)
}

fn inspiration_vector_transition(
    previous: &ActorHpState,
    next: &ActorHpState,
    vector: InspirationVectorRuntimeConfig,
    direction: i64,
) -> Option<InspirationProviderState> {
    let primary_delta = vector.primary_raw_add_delta.checked_mul(direction)?;
    let secondary_delta = vector.secondary_raw_add_delta.checked_mul(direction)?;
    let external_damage_delta = vector.external_damage_delta.checked_mul(direction)?;
    if !previous
        .primary_raw_add
        .iter()
        .zip(next.primary_raw_add.iter())
        .all(|(previous, next)| option_delta(*previous, *next) == Some(primary_delta))
        || option_delta(
            previous.critical_chance_raw_add,
            next.critical_chance_raw_add,
        ) != Some(secondary_delta)
        || option_delta(previous.critical_chance_raw, next.critical_chance_raw)
            != Some(secondary_delta)
        || option_delta(previous.lucky_chance_raw_add, next.lucky_chance_raw_add)
            != Some(secondary_delta)
        || option_delta(previous.lucky_chance_raw, next.lucky_chance_raw) != Some(secondary_delta)
        || option_delta(previous.mastery_raw_add, next.mastery_raw_add) != Some(secondary_delta)
        || option_delta(previous.mastery_raw, next.mastery_raw) != Some(secondary_delta)
        || option_delta(previous.versatility_raw_add, next.versatility_raw_add)
            != Some(secondary_delta)
        || option_delta(previous.versatility_raw, next.versatility_raw) != Some(secondary_delta)
        || option_delta(previous.external_damage_raw, next.external_damage_raw)
            != Some(external_damage_delta)
    {
        return None;
    }
    let physical_attack_base_add_delta = positive_directional_delta(
        previous.physical_attack.base_add,
        next.physical_attack.base_add,
        direction,
    );
    let magical_attack_base_add_delta = positive_directional_delta(
        previous.magical_attack.base_add,
        next.magical_attack.base_add,
        direction,
    );
    if physical_attack_base_add_delta.is_none() && magical_attack_base_add_delta.is_none() {
        return None;
    }
    let haste_delta = positive_directional_delta(
        previous.haste_percent_basis_points,
        next.haste_percent_basis_points,
        direction,
    );
    let property_damage_delta = positive_directional_delta(
        previous.property_damage_raw,
        next.property_damage_raw,
        direction,
    )
    .filter(|delta| *delta == vector.property_damage_raw_delta);
    Some(InspirationProviderState {
        provider_full_bloom: vector.provider_full_bloom,
        primary_raw_add_delta: vector.primary_raw_add_delta,
        secondary_raw_add_delta: vector.secondary_raw_add_delta,
        physical_attack_base_add_delta,
        magical_attack_base_add_delta,
        external_damage_delta: vector.external_damage_delta,
        property_damage_delta,
        haste_delta,
    })
}

fn inspiration_state_removal_matches(
    previous: &ActorHpState,
    next: &ActorHpState,
    state: InspirationProviderState,
) -> bool {
    previous
        .primary_raw_add
        .iter()
        .zip(next.primary_raw_add.iter())
        .all(|(previous, next)| {
            option_delta(*previous, *next) == state.primary_raw_add_delta.checked_neg()
        })
        && option_delta(
            previous.critical_chance_raw_add,
            next.critical_chance_raw_add,
        ) == state.secondary_raw_add_delta.checked_neg()
        && option_delta(previous.critical_chance_raw, next.critical_chance_raw)
            == state.secondary_raw_add_delta.checked_neg()
        && option_delta(previous.lucky_chance_raw_add, next.lucky_chance_raw_add)
            == state.secondary_raw_add_delta.checked_neg()
        && option_delta(previous.lucky_chance_raw, next.lucky_chance_raw)
            == state.secondary_raw_add_delta.checked_neg()
        && option_delta(previous.mastery_raw_add, next.mastery_raw_add)
            == state.secondary_raw_add_delta.checked_neg()
        && option_delta(previous.mastery_raw, next.mastery_raw)
            == state.secondary_raw_add_delta.checked_neg()
        && option_delta(previous.versatility_raw_add, next.versatility_raw_add)
            == state.secondary_raw_add_delta.checked_neg()
        && option_delta(previous.versatility_raw, next.versatility_raw)
            == state.secondary_raw_add_delta.checked_neg()
        && option_delta(previous.external_damage_raw, next.external_damage_raw)
            == state.external_damage_delta.checked_neg()
        && optional_component_removal_matches(
            previous.physical_attack.base_add,
            next.physical_attack.base_add,
            state.physical_attack_base_add_delta,
        )
        && optional_component_removal_matches(
            previous.magical_attack.base_add,
            next.magical_attack.base_add,
            state.magical_attack_base_add_delta,
        )
        && optional_component_removal_matches(
            previous.haste_percent_basis_points,
            next.haste_percent_basis_points,
            state.haste_delta,
        )
        && optional_component_removal_matches(
            previous.property_damage_raw,
            next.property_damage_raw,
            state.property_damage_delta,
        )
}

fn inspiration_mode_transition(
    previous: &ActorHpState,
    next: &ActorHpState,
    old: InspirationProviderState,
    new_vector: InspirationVectorRuntimeConfig,
) -> Option<InspirationProviderState> {
    let primary_change = new_vector
        .primary_raw_add_delta
        .checked_sub(old.primary_raw_add_delta)?;
    let secondary_change = new_vector
        .secondary_raw_add_delta
        .checked_sub(old.secondary_raw_add_delta)?;
    let external_damage_change = new_vector
        .external_damage_delta
        .checked_sub(old.external_damage_delta)?;
    if !previous
        .primary_raw_add
        .iter()
        .zip(next.primary_raw_add.iter())
        .all(|(previous, next)| option_delta(*previous, *next) == Some(primary_change))
        || option_delta(
            previous.critical_chance_raw_add,
            next.critical_chance_raw_add,
        ) != Some(secondary_change)
        || option_delta(previous.critical_chance_raw, next.critical_chance_raw)
            != Some(secondary_change)
        || option_delta(previous.lucky_chance_raw_add, next.lucky_chance_raw_add)
            != Some(secondary_change)
        || option_delta(previous.lucky_chance_raw, next.lucky_chance_raw) != Some(secondary_change)
        || option_delta(previous.mastery_raw_add, next.mastery_raw_add) != Some(secondary_change)
        || option_delta(previous.mastery_raw, next.mastery_raw) != Some(secondary_change)
        || option_delta(previous.versatility_raw_add, next.versatility_raw_add)
            != Some(secondary_change)
        || option_delta(previous.versatility_raw, next.versatility_raw) != Some(secondary_change)
        || option_delta(previous.external_damage_raw, next.external_damage_raw)
            != Some(external_damage_change)
    {
        return None;
    }
    Some(InspirationProviderState {
        provider_full_bloom: new_vector.provider_full_bloom,
        primary_raw_add_delta: new_vector.primary_raw_add_delta,
        secondary_raw_add_delta: new_vector.secondary_raw_add_delta,
        physical_attack_base_add_delta: adjusted_positive_component(
            old.physical_attack_base_add_delta,
            option_delta(
                previous.physical_attack.base_add,
                next.physical_attack.base_add,
            ),
        ),
        magical_attack_base_add_delta: adjusted_positive_component(
            old.magical_attack_base_add_delta,
            option_delta(
                previous.magical_attack.base_add,
                next.magical_attack.base_add,
            ),
        ),
        external_damage_delta: new_vector.external_damage_delta,
        property_damage_delta: adjusted_positive_component(
            old.property_damage_delta,
            option_delta(previous.property_damage_raw, next.property_damage_raw),
        )
        .filter(|delta| *delta == new_vector.property_damage_raw_delta),
        haste_delta: adjusted_positive_component(
            old.haste_delta,
            option_delta(
                previous.haste_percent_basis_points,
                next.haste_percent_basis_points,
            ),
        ),
    })
}

fn option_delta(previous: Option<i64>, next: Option<i64>) -> Option<i64> {
    next?.checked_sub(previous?)
}

fn fixed_point_family_component_deltas(
    previous: &FixedPointFamilyState,
    next: &FixedPointFamilyState,
) -> Option<[i64; 6]> {
    Some([
        option_delta(previous.current_value, next.current_value)?,
        option_delta(previous.total_value, next.total_value)?,
        option_delta(previous.add_value, next.add_value)?,
        unchanged_or_observed_delta(previous.extra_add_value, next.extra_add_value)?,
        unchanged_or_observed_delta(previous.percent_value, next.percent_value)?,
        unchanged_or_observed_delta(previous.extra_percent_value, next.extra_percent_value)?,
    ])
}

fn unchanged_or_observed_delta(previous: Option<i64>, next: Option<i64>) -> Option<i64> {
    match (previous, next) {
        (Some(previous), Some(next)) => next.checked_sub(previous),
        // Entity-attribute delta packets omit unchanged family components. An
        // absent value on both sides therefore proves a zero transition; a
        // one-sided value remains ambiguous and must fail closed.
        (None, None) => Some(0),
        (None, Some(_)) | (Some(_), None) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn exact_inspiration_occurrence_fraction(
    observed_damage: i64,
    critical: bool,
    lucky: bool,
    current_critical_chance_raw: i64,
    current_lucky_chance_raw: i64,
    provider_chance_raw_delta: i64,
    current_critical_damage_raw: i64,
    critical_damage_factor_interpretation: CriticalDamageFactorInterpretation,
) -> Option<(i128, i128)> {
    match (critical, lucky) {
        (true, false) => exact_external_critical_chance_fraction(
            observed_damage,
            current_critical_chance_raw,
            provider_chance_raw_delta,
            current_critical_damage_raw,
            critical_damage_factor_interpretation,
        ),
        (false, true) => exact_external_lucky_chance_fraction(
            observed_damage,
            current_lucky_chance_raw,
            provider_chance_raw_delta,
        ),
        (true, true) => exact_external_combined_critical_lucky_chance_fraction(
            observed_damage,
            current_critical_chance_raw,
            provider_chance_raw_delta,
            current_lucky_chance_raw,
            provider_chance_raw_delta,
            current_critical_damage_raw,
            critical_damage_factor_interpretation,
        ),
        (false, false) => None,
    }
}

fn positive_directional_delta(
    previous: Option<i64>,
    next: Option<i64>,
    direction: i64,
) -> Option<i64> {
    let delta = option_delta(previous, next)?.checked_mul(direction)?;
    (delta > 0).then_some(delta)
}

fn optional_component_removal_matches(
    previous: Option<i64>,
    next: Option<i64>,
    component: Option<i64>,
) -> bool {
    match component {
        Some(component) => option_delta(previous, next) == component.checked_neg(),
        None => true,
    }
}

fn adjusted_positive_component(previous: Option<i64>, change: Option<i64>) -> Option<i64> {
    let adjusted = previous?.checked_add(change?)?;
    (adjusted > 0).then_some(adjusted)
}

fn integer_attribute(attribute: &EntityAttribute) -> Option<i64> {
    if let Some(EntityAttributeValue::Integer(value)) = attribute.decoded {
        return Some(value);
    }
    match decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value) {
        Some(EntityAttributeValue::Integer(value)) => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> &'static RdpsRuntimeConfig {
        rdps_runtime_config().expect("bundled rDPS runtime pack should validate")
    }

    fn test_entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: rlogs_events::ActorId(actor_id),
            entity_uuid: rlogs_events::EntityUuid(entity_uuid),
        }
    }

    fn test_ancestry(entities: &[(u64, i64)]) -> ActorAncestryResolver {
        let mut resolver = ActorAncestryResolver::default();
        for &(actor_id, entity_uuid) in entities {
            resolver.observe_entity(test_entity(actor_id, entity_uuid));
        }
        resolver
    }

    fn integer_test_attribute(attribute_id: i32, value: i64) -> EntityAttribute {
        EntityAttribute {
            attribute_id,
            decoded: Some(EntityAttributeValue::Integer(value)),
            raw_value: Vec::new(),
        }
    }

    fn critical_test_damage(
        source: rlogs_events::EntityRef,
        amount: i64,
    ) -> rlogs_events::DamageEvent {
        rlogs_events::DamageEvent {
            source,
            direct_source: None,
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(17),
                entity_uuid: rlogs_events::EntityUuid(170),
            },
            ability: Some(rlogs_events::AbilityId(2_203_291)),
            amount,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(7),
            damage_source: None,
            damage_type: Some(2),
            flags: rlogs_events::DamageFlags {
                critical: Some(true),
                lucky: Some(false),
                ..rlogs_events::DamageFlags::default()
            },
            packet: rlogs_events::DamagePacketDetail::default(),
        }
    }

    fn team_luck_fraction(
        observed_damage: i64,
        critical: bool,
        lucky: bool,
        critical_multiplier: i64,
        lucky_multiplier: i64,
    ) -> Option<(i128, i128)> {
        let config = &runtime().team_luck;
        exact_team_luck_accounting_fraction(
            observed_damage,
            critical,
            lucky,
            critical_multiplier,
            lucky_multiplier,
            config.critical_raw_delta,
            config.lucky_raw_delta,
            config.combined_critical_lucky_enabled,
            CriticalDamageFactorInterpretation::AdditiveBonus,
        )
    }

    #[test]
    fn catalog_is_current_build_and_formula_exact() {
        assert_eq!(
            runtime().critical_damage_factor_interpretation,
            CriticalDamageFactorInterpretation::AdditiveBonus,
        );
        assert!(
            runtime()
                .critical_damage_factor_interpretation
                .is_resolved()
        );
        assert!(!runtime().runtime_promotion_allowed());
        let formula_identity = state_damage_contribution_formula_identity();
        assert!(formula_identity.starts_with("sha256:"));
        assert_eq!(formula_identity.len(), 71);
        assert_eq!(
            BpsrStateDamageContributionProjector::default().formula_identity(),
            Some(formula_identity)
        );

        let catalog = state_rdps_catalog().unwrap();
        assert!(catalog.rules.is_empty());
        let candidate = &catalog.candidate_rules[0];
        assert!(!candidate.runtime_eligible);
        assert_eq!(
            candidate.proof_state,
            "historical-packet-formula-current-static-lineage-only"
        );
        let rule = &candidate.rule;
        assert_eq!(rule.effect_id, 2_404_261);
        assert_eq!(rule.raw_percent_per_stack, 250);
        assert_eq!(rule.maximum_stacks, 4);
        assert_eq!(rule.enabled_provider_raw_percent_values, vec![500]);
        assert_eq!(rule.final_attribute_id, 11_320);
        assert_eq!(rule.ability_id, 2_206_290);
        assert_eq!(rule.state_multiplier, 3);
        assert_eq!(rule.constant_offset, -1);

        let target_catalog = target_vulnerability_rdps_catalog().unwrap();
        assert_eq!(target_catalog.rule_indices_for_ability(2_203_291), &[0]);
        assert!(
            target_catalog
                .rule_indices_for_ability(2_203_292)
                .is_empty()
        );
        assert_eq!(
            target_catalog.rule_indices_for_damage(2_203_291, Some(7), Some(true), Some(false),),
            &[0],
        );
        assert_eq!(
            target_catalog.rule_indices_for_damage(2_031_102, Some(3), Some(true), Some(true),),
            &[1],
        );
        assert!(
            target_catalog
                .rule_indices_for_damage(2_203_291, Some(7), Some(false), Some(false))
                .is_empty()
        );
        assert_eq!(target_catalog.effect_ids, HashSet::from([55_228]));
        let target_rule = &target_catalog.rules[0];
        assert_eq!(target_rule.effect_id, 55_228);
        assert_eq!(target_rule.ability_id, 2_203_291);
        assert_eq!(target_rule.hit_event_id, 7);
        assert_eq!(target_rule.damage_attr_id, 2_220_329_107);
        assert_eq!(target_rule.active_factor(), Some(16_600));
        assert_eq!(target_rule.provider_raw_delta, 1_000);
        assert_eq!(
            target_rule.fixed_point_rounding(),
            Some(PositiveFixedPointRounding::Floor)
        );
        assert_eq!(
            target_rule.required_critical,
            RequiredCriticalObservation::ReportedTrue
        );
        assert!(!target_rule.required_lucky);
        let paired_rule = &target_catalog.rules[1];
        assert_eq!(paired_rule.effect_id, 55_228);
        assert_eq!(paired_rule.ability_id, 2_031_102);
        assert_eq!(paired_rule.hit_event_id, 3);
        assert_eq!(paired_rule.damage_attr_id, 2_203_110_203);
        assert_eq!(paired_rule.active_factor(), None);
        assert_eq!(paired_rule.active_observed_damage, Some(272_418));
        assert_eq!(paired_rule.inactive_observed_damage, Some(258_416));
        assert_eq!(paired_rule.required_source_class_id, Some(2));
        assert!(paired_rule.runtime_eligible);
        assert_eq!(
            paired_rule.required_critical,
            RequiredCriticalObservation::ReportedTrue
        );
        assert!(paired_rule.required_lucky);
        let unreported_critical_rule = &target_catalog.rules[2];
        assert_eq!(
            unreported_critical_rule.required_critical,
            RequiredCriticalObservation::Unreported
        );
        assert_eq!(
            unreported_critical_rule.active_observed_damage,
            Some(114_422)
        );
        assert_eq!(
            unreported_critical_rule.inactive_observed_damage,
            Some(108_540)
        );
        assert!(unreported_critical_rule.runtime_eligible);
        assert_eq!(
            proven_state_damage_contribution_effect_ids().unwrap(),
            vec![55_228, 2_110_140, 2_110_143, 2_302_121, 3_003_052],
            "the exact packet-pair vulnerability, observed Mechanical Power component, dormant Functional Amp component, Team Luck Lucky component, and class-scoped Harmony proportional rule are production promoted"
        );
        assert_eq!(
            target_vulnerability_candidate_effect_ids().unwrap(),
            vec![55_228],
            "offline candidate identity must remain explicit and separate from proven production rules"
        );
    }

    #[test]
    fn target_vulnerability_formula_shapes_fail_closed() {
        let mixed: TargetVulnerabilityRdpsRule = serde_json::from_str(
            r#"{
                "effect_id": 55228,
                "ability_id": 2031102,
                "hit_event_id": 3,
                "damage_attr_id": 2203110203,
                "projection": "exact_rational_observed_output",
                "inactive_factor": 18455,
                "active_factor": 19455,
                "provider_raw_delta": 1000,
                "integer_projection": "sum_exact_then_half_up_per_effect_provider_recipient",
                "required_critical": "reported_true",
                "required_lucky": true
            }"#,
        )
        .unwrap();
        assert!(!mixed.is_valid());

        let unknown = serde_json::from_str::<TargetVulnerabilityRdpsRule>(
            r#"{
                "effect_id": 55228,
                "ability_id": 2031102,
                "hit_event_id": 3,
                "damage_attr_id": 2203110203,
                "projection": "exact_rational_observed_output",
                "active_factor": 19455,
                "provider_raw_delta": 1000,
                "integer_projection": "sum_exact_then_half_up_per_effect_provider_recipient",
                "required_critical": "reported_true",
                "required_lucky": true,
                "unreviewed_formula_field": 1
            }"#,
        );
        assert!(unknown.is_err());
    }

    #[test]
    fn current_build_interpretation_enables_exact_critical_damage_but_not_unproven_dependents() {
        let interpretation = runtime().critical_damage_factor_interpretation;
        assert_eq!(
            interpretation,
            CriticalDamageFactorInterpretation::AdditiveBonus
        );
        assert_eq!(
            exact_team_luck_accounting_fraction(
                100_000,
                true,
                false,
                10_128,
                0,
                runtime().team_luck.critical_raw_delta,
                runtime().team_luck.lucky_raw_delta,
                runtime().team_luck.combined_critical_lucky_enabled,
                interpretation,
            ),
            Some((1_625_000, 629)),
        );
        assert!(
            exact_inspiration_occurrence_fraction(
                100_000,
                true,
                false,
                7_416,
                800,
                300,
                10_128,
                interpretation,
            )
            .is_some(),
            "the shared factor interpretation is resolved even though Inspiration still has independent runtime promotion gates",
        );
    }

    #[test]
    fn team_luck_live_projection_accepts_exact_single_outcome_routes_and_keeps_combined_closed() {
        let current_wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let mut projector = BpsrStateDamageContributionProjector {
            current_wire: Some(current_wire),
            states: HashMap::from([(
                4,
                ActorHpState {
                    critical_damage_raw: Some(10_128),
                    lucky_damage_raw: Some(4_540),
                    ..ActorHpState::default()
                },
            )]),
            active_players: HashSet::from([2, 4]),
            team_luck_windows: HashSet::from([TeamLuckWindowKey {
                target_actor_id: 4,
                target_entity_uuid: 40,
                provider_actor_id: 2,
                provider_entity_uuid: 20,
                instance_id: Some(11),
            }]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let mut damage = rlogs_events::DamageEvent {
            source: test_entity(4, 40),
            direct_source: None,
            target: test_entity(17, 170),
            ability: Some(rlogs_events::AbilityId(2_031_101)),
            amount: 273_931,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(3),
            damage_source: None,
            damage_type: Some(2),
            flags: rlogs_events::DamageFlags {
                critical: Some(false),
                lucky: Some(true),
                ..rlogs_events::DamageFlags::default()
            },
            packet: rlogs_events::DamagePacketDetail::default(),
        };

        assert_eq!(
            projector.team_luck_decision(123, &damage),
            Ok(ExactRationalDamageContributionEvent {
                observed_micros: 123,
                effect_id: runtime().team_luck.effect_id,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                numerator: 4_656_827,
                denominator: 227,
                observed_damage: 273_931,
                included: true,
            })
        );
        assert_eq!(damage.amount, 273_931, "ordinary damage is immutable");

        damage.hit_event_id = Some(4);
        assert_eq!(
            projector.team_luck_decision(123, &damage),
            Err("lucky_damage_route_unproven")
        );

        damage.hit_event_id = Some(3);
        damage.flags.critical = Some(true);
        damage.flags.lucky = Some(false);
        damage.amount = 58_708;
        projector
            .states
            .get_mut(&4)
            .expect("recipient state")
            .critical_damage_raw = Some(11_586);
        assert_eq!(
            projector.team_luck_decision(123, &damage),
            Ok(ExactRationalDamageContributionEvent {
                observed_micros: 123,
                effect_id: runtime().team_luck.effect_id,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                numerator: 15_264_080,
                denominator: 10_793,
                observed_damage: 58_708,
                included: true,
            })
        );

        damage.flags.lucky = Some(true);
        assert_eq!(
            projector.team_luck_decision(123, &damage),
            Err("damage_counterfactual_unproven"),
            "combined Crit+Lucky ordering remains fail-closed"
        );

        damage.flags.lucky = Some(false);
        projector
            .states
            .get_mut(&4)
            .expect("recipient state")
            .critical_damage_raw = None;
        assert_eq!(
            projector.team_luck_decision(123, &damage),
            Err("critical_damage_state_never_observed"),
            "remote or otherwise absent recipient critical state must not receive invented credit"
        );
        projector
            .states
            .get_mut(&4)
            .expect("recipient state")
            .critical_damage_raw = Some(10_128);

        projector.team_luck_windows.clear();
        let mut status = rlogs_events::StatusEvent {
            source: Some(test_entity(2, 20)),
            target: test_entity(4, 40),
            effect: rlogs_events::StatusEffectId(runtime().team_luck.effect_id),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(11)),
            origin: Some(rlogs_events::StatusOrigin {
                source_type_id: runtime().team_luck.source_type_id,
                source_config_id: runtime().team_luck.source_config_id,
            }),
            state: StatusState::Applied,
            stacks: Some(1),
            duration_millis: Some(15_000),
            level: Some(1),
            part_id: None,
            count: Some(-1),
            created_at_millis: None,
        };
        projector.observe_status(&status, 123);
        assert_eq!(projector.team_luck_windows.len(), 1);

        projector.team_luck_windows.clear();
        status.origin = Some(rlogs_events::StatusOrigin {
            source_type_id: runtime().team_luck.source_type_id,
            source_config_id: runtime().team_luck.source_config_id + 1,
        });
        projector.observe_status(&status, 123);
        assert!(projector.team_luck_windows.is_empty());
    }

    #[test]
    fn team_luck_live_projection_joins_rotated_actor_ids_only_by_exact_entity_uuid() {
        let mut projector = BpsrStateDamageContributionProjector {
            current_wire: Some(WireKey {
                connection_id: 1,
                stream_id: 2,
                capture_sequence: 3,
            }),
            states: HashMap::from([(
                400,
                ActorHpState {
                    lucky_damage_raw: Some(4_540),
                    ..ActorHpState::default()
                },
            )]),
            attribute_state_actor_by_entity_uuid: HashMap::from([(40, 400)]),
            attribute_state_entity_uuid_by_actor: HashMap::from([(400, 40)]),
            active_players: HashSet::from([2, 4, 400]),
            team_luck_windows: HashSet::from([TeamLuckWindowKey {
                target_actor_id: 400,
                target_entity_uuid: 40,
                provider_actor_id: 2,
                provider_entity_uuid: 20,
                instance_id: Some(11),
            }]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let mut damage = rlogs_events::DamageEvent {
            source: test_entity(4, 40),
            direct_source: None,
            target: test_entity(17, 170),
            ability: Some(rlogs_events::AbilityId(2_031_101)),
            amount: 273_931,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(3),
            damage_source: None,
            damage_type: Some(2),
            flags: rlogs_events::DamageFlags {
                critical: Some(false),
                lucky: Some(true),
                ..rlogs_events::DamageFlags::default()
            },
            packet: rlogs_events::DamagePacketDetail::default(),
        };

        assert_eq!(
            projector.team_luck_decision(123, &damage),
            Ok(ExactRationalDamageContributionEvent {
                observed_micros: 123,
                effect_id: runtime().team_luck.effect_id,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                numerator: 4_656_827,
                denominator: 227,
                observed_damage: 273_931,
                included: true,
            })
        );

        projector.team_luck_windows = HashSet::from([TeamLuckWindowKey {
            target_actor_id: 4,
            target_entity_uuid: 41,
            provider_actor_id: 2,
            provider_entity_uuid: 20,
            instance_id: Some(12),
        }]);
        damage.source = test_entity(4, 41);
        assert_eq!(
            projector.team_luck_decision(123, &damage),
            Err("recipient_state_never_observed"),
            "an actor-id collision must not reuse another entity's state"
        );
    }

    #[test]
    fn remote_rdps_never_requires_a_character_build_snapshot() {
        assert_eq!(
            remote_rdps_evidence_policy(),
            RemoteRdpsEvidencePolicy {
                build_snapshot_required: false,
                character_level_required: false,
                exact_equipment_required: false,
                exact_factor_tree_required: false,
                provider_recipient_window_required: true,
                applied_runtime_magnitude_required: true,
                exact_counterfactual_formula_required: true,
                retain_damage_when_unresolved: true,
            }
        );
    }

    #[test]
    fn fatal_spiral_removal_learns_exact_provider_magnitude_by_entity_uuid() {
        let mut projector = BpsrStateDamageContributionProjector {
            states: HashMap::from([(
                6,
                ActorHpState {
                    all_element: FixedPointFamilyState {
                        current_value: Some(1_237),
                        total_value: Some(1_237),
                        add_value: Some(1_237),
                        extra_add_value: Some(0),
                        percent_value: Some(0),
                        extra_percent_value: Some(0),
                        ..FixedPointFamilyState::default()
                    },
                    ..ActorHpState::default()
                },
            )]),
            staged_states: HashMap::from([(
                6,
                ActorHpState {
                    all_element: FixedPointFamilyState {
                        current_value: Some(237),
                        total_value: Some(237),
                        add_value: Some(237),
                        extra_add_value: Some(0),
                        percent_value: Some(0),
                        extra_percent_value: Some(0),
                        ..FixedPointFamilyState::default()
                    },
                    ..ActorHpState::default()
                },
            )]),
            fatal_spiral_transitions: HashMap::from([(
                6,
                vec![FixedPointFamilyTransition {
                    provider_actor_id: 3,
                    provider_entity_uuid: 30,
                    active: false,
                }],
            )]),
            active_players: HashSet::from([3, 6]),
            ..BpsrStateDamageContributionProjector::default()
        };

        projector.reconcile_fatal_spiral_staged_states();

        assert_eq!(
            projector
                .fatal_spiral_provider_basis_points_by_entity_uuid
                .get(&30),
            Some(&1_000)
        );
        assert!(
            !projector
                .fatal_spiral_ambiguous_provider_entities
                .contains(&30)
        );
    }

    #[test]
    fn fatal_spiral_delta_omits_unchanged_optional_components() {
        let mut projector = BpsrStateDamageContributionProjector {
            states: HashMap::from([(
                6,
                ActorHpState {
                    all_element: FixedPointFamilyState {
                        current_value: Some(1_237),
                        total_value: Some(1_237),
                        add_value: Some(1_237),
                        ..FixedPointFamilyState::default()
                    },
                    ..ActorHpState::default()
                },
            )]),
            staged_states: HashMap::from([(
                6,
                ActorHpState {
                    all_element: FixedPointFamilyState {
                        current_value: Some(237),
                        total_value: Some(237),
                        add_value: Some(237),
                        ..FixedPointFamilyState::default()
                    },
                    ..ActorHpState::default()
                },
            )]),
            fatal_spiral_transitions: HashMap::from([(
                6,
                vec![FixedPointFamilyTransition {
                    provider_actor_id: 3,
                    provider_entity_uuid: 30,
                    active: false,
                }],
            )]),
            active_players: HashSet::from([3, 6]),
            ..BpsrStateDamageContributionProjector::default()
        };

        projector.reconcile_fatal_spiral_staged_states();

        assert_eq!(
            projector
                .fatal_spiral_provider_basis_points_by_entity_uuid
                .get(&30),
            Some(&1_000)
        );
    }

    #[test]
    fn fatal_spiral_remote_recipient_reuses_packet_proven_provider_magnitude() {
        let mut projector = BpsrStateDamageContributionProjector {
            fatal_spiral_windows: HashSet::from([FatalSpiralWindowKey {
                target_actor_id: 4,
                target_entity_uuid: 40,
                provider_actor_id: 3,
                provider_entity_uuid: 30,
                instance_id: Some(11),
            }]),
            fatal_spiral_provider_basis_points_by_entity_uuid: HashMap::from([(30, 1_000)]),
            active_players: HashSet::from([3, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        projector.runtime_applicable = true;
        let mut damage = critical_test_damage(test_entity(4, 40), 1_000_000);
        damage.ability = Some(rlogs_events::AbilityId(2_233));
        damage.packet.property = Some(1);

        assert_eq!(
            projector.fatal_spiral_audit_gate(&damage),
            "damage_stage_unproven"
        );
        assert!(
            projector
                .fatal_spiral_audit_detail(&damage)
                .contains("(3, 30, Some(1000), false)")
        );
    }

    #[test]
    fn fatal_spiral_conflicting_provider_magnitudes_are_never_guessed() {
        let mut projector = BpsrStateDamageContributionProjector::default();
        projector.learn_fatal_spiral_provider_basis_points(30, 1_000);
        projector.learn_fatal_spiral_provider_basis_points(30, 800);

        assert!(
            !projector
                .fatal_spiral_provider_basis_points_by_entity_uuid
                .contains_key(&30)
        );
        assert!(
            projector
                .fatal_spiral_ambiguous_provider_entities
                .contains(&30)
        );
    }

    #[test]
    fn authored_build_and_protocol_identity_distinguish_exact_from_provisional_use() {
        assert!(runtime_matches_event_identity(
            runtime(),
            "global",
            "24687926",
            runtime().protocol_pack_digest.as_str(),
        ));
        assert!(!runtime_matches_event_identity(
            runtime(),
            "cn",
            "24687926",
            runtime().protocol_pack_digest.as_str(),
        ));
        assert!(!runtime_matches_event_identity(
            runtime(),
            "global",
            "next-build",
            runtime().protocol_pack_digest.as_str(),
        ));
        assert!(!runtime_matches_event_identity(
            runtime(),
            "global",
            "24687926",
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ));
    }

    #[test]
    fn same_deployment_hotfix_disables_rdps_and_requires_an_exact_build() {
        let projector = BpsrStateDamageContributionProjector {
            observed_deployment_id: Some(runtime().deployment_id.clone()),
            observed_client_build: Some("hotfix-after-formula-pack".into()),
            observed_protocol_pack_digest: Some(runtime().protocol_pack_digest.clone()),
            runtime_applicable: false,
            ..BpsrStateDamageContributionProjector::default()
        };

        assert!(!projector.enabled());
        assert!(
            rdps_runtime_config_for("global", "hotfix-after-formula-pack")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            projector.status(),
            "formula_pack_incompatible: formula=global/24687926; game=global/hotfix-after-formula-pack; exact-build attribution required"
        );
    }

    #[test]
    fn exact_current_build_retains_formula_identity_and_only_proven_partial_credit() {
        let projector = BpsrStateDamageContributionProjector {
            observed_deployment_id: Some(runtime().deployment_id.clone()),
            observed_client_build: Some(runtime().game_build.clone()),
            observed_protocol_pack_digest: Some(runtime().protocol_pack_digest.clone()),
            ..BpsrStateDamageContributionProjector::default()
        };

        assert!(!runtime().runtime_promotion_allowed());
        assert!(
            state_damage_contribution_target_matches(
                "global",
                "24687926",
                runtime().protocol_pack_digest.as_str(),
            )
            .unwrap()
        );
        assert!(
            state_damage_contribution_formula_target_matches(
                "global",
                "24687926",
                runtime().protocol_pack_digest.as_str(),
            )
            .unwrap()
        );
        let prior_pack_digest =
            "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b";
        assert!(
            state_damage_contribution_target_matches("global", "24687926", prior_pack_digest,)
                .unwrap(),
            "current-build history keeps its exact prior decoder identity"
        );
        let prior_pack_runtime =
            rdps_runtime_config_for_identity("global", "24687926", prior_pack_digest)
                .unwrap()
                .expect("the exact current-build prior-pack identity should be replayable");
        assert_eq!(prior_pack_runtime.protocol_pack_digest, prior_pack_digest);
        assert!(
            prior_pack_runtime
                .effect_runtime_transfer_enabled(prior_pack_runtime.team_luck.effect_id)
        );
        assert!(
            !state_damage_contribution_formula_target_matches(
                "global",
                "24687926",
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            )
            .unwrap()
        );
        assert_eq!(
            proven_state_damage_contribution_effect_ids().unwrap(),
            vec![55_228, 2_110_140, 2_110_143, 2_302_121, 3_003_052]
        );
        assert_eq!(projector.status(), "partial_packet_proven_rules");
        assert_eq!(
            runtime().promotion_blockers(),
            [
                "canonical-replay-conservation",
                "party-support-formula-frontier",
            ]
        );
    }

    #[test]
    fn exact_build_with_a_different_protocol_pack_fails_closed() {
        let projector = BpsrStateDamageContributionProjector {
            observed_deployment_id: Some(runtime().deployment_id.clone()),
            observed_client_build: Some(runtime().game_build.clone()),
            observed_protocol_pack_digest: Some(
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into(),
            ),
            ..BpsrStateDamageContributionProjector::default()
        };

        assert!(!projector.enabled());
        assert_eq!(
            projector.status(),
            format!(
                "formula_pack_incompatible: formula=global/24687926@{}; game=global/24687926@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff; exact protocol-pack attribution required",
                runtime().protocol_pack_digest,
            )
        );
    }

    #[test]
    fn unresolved_status_target_blocks_both_buff_and_vulnerability_attribution_paths() {
        let mut projector = BpsrStateDamageContributionProjector::default();
        projector
            .unresolved_status_windows
            .insert(UnresolvedStatusWindowKey {
                target_actor_id: 4,
                instance_id: Some(99),
            });

        let sourced_by_affected_actor = critical_test_damage(test_entity(4, 40), 1_000);
        assert!(projector.damage_has_unresolved_status_confounder(&sourced_by_affected_actor));
        assert_eq!(
            projector.mechanical_power_audit_gate(&sourced_by_affected_actor),
            "unresolved_status_confounder"
        );

        let mut aimed_at_affected_actor = critical_test_damage(test_entity(6, 60), 1_000);
        aimed_at_affected_actor.target = test_entity(4, 40);
        assert!(projector.damage_has_unresolved_status_confounder(&aimed_at_affected_actor));
        assert_eq!(
            projector.mechanical_power_audit_gate(&aimed_at_affected_actor),
            "unresolved_status_confounder"
        );

        projector.clear_actor(4);
        assert!(!projector.damage_has_unresolved_status_confounder(&sourced_by_affected_actor));
        assert!(!projector.damage_has_unresolved_status_confounder(&aimed_at_affected_actor));
    }

    #[test]
    fn unresolved_status_terminal_clears_only_its_exact_instance() {
        let mut projector = BpsrStateDamageContributionProjector::default();
        let mut status = rlogs_events::UnresolvedStatusEvent {
            source: None,
            target: test_entity(4, 40),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(99)),
            state: Some(StatusState::Applied),
            wire_event_type: Some(1),
            wire_logic_type: None,
            reason: rlogs_events::UnresolvedStatusReason::MissingEffectId,
            raw_payload: Vec::new(),
        };
        let damage = critical_test_damage(test_entity(4, 40), 1_000);

        projector.observe_unresolved_status(&status);
        assert!(projector.damage_has_unresolved_status_confounder(&damage));

        status.state = Some(StatusState::Removed);
        status.wire_event_type = Some(2);
        projector.observe_unresolved_status(&status);
        assert!(!projector.damage_has_unresolved_status_confounder(&damage));

        status.instance_id = None;
        status.state = Some(StatusState::Applied);
        projector.observe_unresolved_status(&status);
        status.state = Some(StatusState::Removed);
        projector.observe_unresolved_status(&status);
        assert!(projector.damage_has_unresolved_status_confounder(&damage));
    }

    #[test]
    fn offline_candidate_audits_never_enable_the_production_projector() {
        let inspiration = BpsrStateDamageContributionProjector::new_inspiration_candidate_audit()
            .expect("inspiration audit projector should validate");
        assert!(inspiration.inspiration_candidate_audit_enabled);
        assert!(!inspiration.harmony_grace_candidate_audit_enabled);
        assert!(!inspiration.target_vulnerability_candidate_audit_enabled);
        assert!(!inspiration.enabled());

        let harmony = BpsrStateDamageContributionProjector::new_harmony_grace_candidate_audit()
            .expect("Harmony Grace audit projector should validate");
        assert!(!harmony.inspiration_candidate_audit_enabled);
        assert!(harmony.harmony_grace_candidate_audit_enabled);
        assert!(!harmony.target_vulnerability_candidate_audit_enabled);
        assert!(!harmony.enabled());

        let mechanical =
            BpsrStateDamageContributionProjector::new_mechanical_power_candidate_audit()
                .expect("Mechanical Power audit projector should validate");
        assert!(!mechanical.inspiration_candidate_audit_enabled);
        assert!(!mechanical.harmony_grace_candidate_audit_enabled);
        assert!(mechanical.mechanical_power_candidate_audit_enabled);
        assert!(!mechanical.target_vulnerability_candidate_audit_enabled);
        assert!(!mechanical.enabled());

        let vulnerability =
            BpsrStateDamageContributionProjector::new_target_vulnerability_candidate_audit()
                .expect("target-vulnerability audit projector should validate");
        assert!(!vulnerability.inspiration_candidate_audit_enabled);
        assert!(!vulnerability.harmony_grace_candidate_audit_enabled);
        assert!(vulnerability.target_vulnerability_candidate_audit_enabled);
        assert!(!vulnerability.enabled());
    }

    #[test]
    fn direct_child_status_provider_resolves_to_its_player_owner() {
        let mut projector = BpsrStateDamageContributionProjector {
            active_players: HashSet::from([4]),
            actor_ancestry: test_ancestry(&[(1_144, 11_440), (4, 40)]),
            latest_observed_micros: 1,
            target_vulnerability_windows: HashMap::from([(
                TargetVulnerabilityWindowKey {
                    target_actor_id: 17,
                    provider_actor_id: 1_144,
                    effect_id: 55_228,
                    instance_id: Some(1_581),
                },
                TargetVulnerabilityWindow {
                    expires_at_observed_micros: Some(11_000_000),
                },
            )]),
            ..BpsrStateDamageContributionProjector::default()
        };
        projector.actor_ancestry.observe_relation(
            1,
            test_entity(1_144, 11_440),
            test_entity(4, 40),
            ActorOwnershipEvidence::AttributedCombatSource,
        );
        assert_eq!(projector.resolve_owner_actor_id(1_144), 4);
        assert!(projector.active_players.contains(&4));

        projector.expire_target_vulnerability_windows(11_000_000);
        assert!(projector.target_vulnerability_windows.is_empty());
    }

    fn target_vulnerability_test_status(
        provider_actor_id: u64,
        target_actor_id: u64,
        state: StatusState,
    ) -> rlogs_events::StatusEvent {
        rlogs_events::StatusEvent {
            source: Some(test_entity(
                provider_actor_id,
                i64::try_from(provider_actor_id).unwrap() * 10,
            )),
            target: test_entity(
                target_actor_id,
                i64::try_from(target_actor_id).unwrap() * 10,
            ),
            effect: rlogs_events::StatusEffectId(55_228),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(1_581)),
            origin: Some(rlogs_events::StatusOrigin {
                source_type_id: 1,
                source_config_id: 2_295,
            }),
            state,
            stacks: Some(1),
            level: Some(1),
            part_id: None,
            count: None,
            created_at_millis: None,
            duration_millis: Some(10_000),
        }
    }

    fn target_vulnerability_test_envelope(damage: rlogs_events::DamageEvent) -> EventEnvelope {
        let time = rlogs_events::EventTime {
            observed_micros: 123,
            game_time_millis: None,
        };
        let provenance = rlogs_events::EventProvenance::manual("unit test");
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "target-vulnerability-test".into(),
            sequence: 1,
            region: rlogs_events::RegionContext {
                identity: rlogs_events::RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: None,
                },
                client_build: "24687926".into(),
                protocol_pack_digest: "test".into(),
                evidence: Vec::new(),
            },
            time,
            provenance: provenance.clone(),
            sensitivity: rlogs_events::EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(rlogs_events::TimelineEvent {
                sequence: 1,
                time,
                provenance,
                kind: TimelineEventKind::Damage(damage),
            }),
        }
    }

    #[test]
    fn same_wire_target_vulnerability_preserves_exact_external_provider() {
        let rule = &target_vulnerability_rdps_catalog().unwrap().rules[0];
        let mut projector = BpsrStateDamageContributionProjector {
            active_players: HashSet::from([2, 4]),
            latest_observed_micros: 123,
            ..BpsrStateDamageContributionProjector::default()
        };
        let status = target_vulnerability_test_status(2, 17, StatusState::Applied);
        projector.observe_target_vulnerability_status(&status, 100);
        let mut removal = status.clone();
        removal.state = StatusState::Removed;
        projector.observe_target_vulnerability_status(&removal, 100);
        assert!(projector.target_vulnerability_windows.is_empty());

        let damage = critical_test_damage(test_entity(4, 40), 90_015);
        let envelope = target_vulnerability_test_envelope(damage.clone());
        let contribution = projector
            .target_vulnerability_exact_contribution(&envelope, &damage, rule)
            .expect("same-wire provider is exact packet evidence");
        assert_eq!(contribution.provider_actor_id, 2);
        assert_eq!(contribution.recipient_actor_id, 4);
        assert_eq!(contribution.amount, 5_423);
        assert_eq!(
            projector.target_vulnerability_audit_gate(&damage),
            "candidate_same_wire_provider"
        );
    }

    #[test]
    fn exact_paired_target_vulnerability_transfers_rdps_without_mutating_damage() {
        let mut rule = target_vulnerability_rdps_catalog().unwrap().rules[1].clone();
        let mut projector = BpsrStateDamageContributionProjector {
            active_players: HashSet::from([2, 4]),
            latest_observed_micros: 123,
            class_id_by_actor: HashMap::from([(4, 2)]),
            formula_attributes_by_actor: HashMap::from([
                (4, BTreeMap::from([(11_320, 309_156), (11_321, 309_153)])),
                (17, BTreeMap::from([(455, 1)])),
            ]),
            ..BpsrStateDamageContributionProjector::default()
        };
        projector.observe_target_vulnerability_status(
            &target_vulnerability_test_status(2, 17, StatusState::Applied),
            100,
        );
        rule.required_source_context_sha256 =
            projector.formula_context_sha256(4, rule.effect_id, &rule.ignored_context_effect_ids);
        rule.allowed_target_context_sha256 = vec![
            projector
                .formula_context_sha256(17, rule.effect_id, &rule.ignored_context_effect_ids)
                .unwrap(),
        ];
        let mut damage = critical_test_damage(test_entity(4, 40), 272_418);
        damage.ability = Some(rlogs_events::AbilityId(2_031_102));
        damage.hit_event_id = Some(3);
        damage.flags.lucky = Some(true);
        let envelope = target_vulnerability_test_envelope(damage.clone());

        let contribution = projector
            .target_vulnerability_exact_contribution(&envelope, &damage, &rule)
            .expect("the exact packet pair and formula context are complete");
        assert_eq!(
            contribution,
            ExactDamageContributionEvent {
                observed_micros: 123,
                effect_id: 55_228,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                amount: 14_002,
                observed_damage: 272_418,
                included: true,
            }
        );
        assert_eq!(
            damage.amount, 272_418,
            "ordinary damage must remain unchanged"
        );

        projector
            .formula_attributes_by_actor
            .get_mut(&17)
            .unwrap()
            .insert(455, 2);
        assert_eq!(
            projector.target_vulnerability_exact_contribution(&envelope, &damage, &rule),
            None,
            "a changed target formula context must fail closed",
        );
    }

    #[test]
    fn exact_paired_unreported_critical_branch_is_distinct_from_reported_false() {
        let mut rule = target_vulnerability_rdps_catalog().unwrap().rules[2].clone();
        let mut projector = BpsrStateDamageContributionProjector {
            active_players: HashSet::from([2, 4]),
            latest_observed_micros: 123,
            class_id_by_actor: HashMap::from([(4, 2)]),
            formula_attributes_by_actor: HashMap::from([
                (4, BTreeMap::from([(11_320, 309_156), (11_321, 309_153)])),
                (17, BTreeMap::from([(455, 1)])),
            ]),
            ..BpsrStateDamageContributionProjector::default()
        };
        projector.observe_target_vulnerability_status(
            &target_vulnerability_test_status(2, 17, StatusState::Applied),
            100,
        );
        rule.required_source_context_sha256 =
            projector.formula_context_sha256(4, rule.effect_id, &rule.ignored_context_effect_ids);
        rule.allowed_target_context_sha256 = vec![
            projector
                .formula_context_sha256(17, rule.effect_id, &rule.ignored_context_effect_ids)
                .unwrap(),
        ];

        let mut damage = critical_test_damage(test_entity(4, 40), 114_422);
        damage.ability = Some(rlogs_events::AbilityId(2_031_102));
        damage.hit_event_id = Some(3);
        damage.flags.critical = None;
        damage.flags.lucky = Some(true);
        let envelope = target_vulnerability_test_envelope(damage.clone());
        let contribution = projector
            .target_vulnerability_exact_contribution(&envelope, &damage, &rule)
            .expect("the exact unreported-critical packet pair should match");
        assert_eq!(contribution.amount, 5_882);
        assert_eq!(
            damage.amount, 114_422,
            "ordinary damage must remain unchanged"
        );

        damage.flags.critical = Some(false);
        assert_eq!(
            projector.target_vulnerability_exact_contribution(&envelope, &damage, &rule),
            None,
            "reported false must not alias the packet's unreported critical state",
        );
    }

    #[test]
    fn same_wire_target_vulnerability_is_scoped_to_exact_target_and_external_provider() {
        let mut projector = BpsrStateDamageContributionProjector {
            active_players: HashSet::from([2, 4]),
            latest_observed_micros: 123,
            ..BpsrStateDamageContributionProjector::default()
        };
        projector.observe_target_vulnerability_status(
            &target_vulnerability_test_status(2, 18, StatusState::Applied),
            100,
        );
        let damage = critical_test_damage(test_entity(4, 40), 90_015);
        assert_eq!(
            projector.target_vulnerability_audit_gate(&damage),
            "no_external_active_provider"
        );

        projector.observe_target_vulnerability_status(
            &target_vulnerability_test_status(4, 17, StatusState::Applied),
            100,
        );
        assert_eq!(
            projector.target_vulnerability_audit_gate(&damage),
            "no_external_active_provider"
        );
    }

    #[test]
    fn same_wire_target_vulnerability_refuses_multiple_external_providers() {
        let mut projector = BpsrStateDamageContributionProjector {
            active_players: HashSet::from([2, 3, 4]),
            latest_observed_micros: 123,
            ..BpsrStateDamageContributionProjector::default()
        };
        for provider_actor_id in [2, 3] {
            projector.observe_target_vulnerability_status(
                &target_vulnerability_test_status(provider_actor_id, 17, StatusState::Applied),
                100,
            );
        }
        let damage = critical_test_damage(test_entity(4, 40), 90_015);
        assert_eq!(
            projector.target_vulnerability_audit_gate(&damage),
            "multiple_external_active_providers"
        );
    }

    #[test]
    fn same_wire_target_vulnerability_evidence_expires_on_wire_advance() {
        let mut projector = BpsrStateDamageContributionProjector {
            active_players: HashSet::from([2, 4]),
            current_wire: Some(WireKey {
                connection_id: 1,
                stream_id: 2,
                capture_sequence: 3,
            }),
            ..BpsrStateDamageContributionProjector::default()
        };
        projector.observe_target_vulnerability_status(
            &target_vulnerability_test_status(2, 17, StatusState::Applied),
            100,
        );
        assert!(!projector.target_vulnerability_transitions.is_empty());
        projector.advance_wire(WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 4,
        });
        assert!(projector.target_vulnerability_transitions.is_empty());
    }

    #[test]
    fn thunderwind_resolved_candidate_does_not_bypass_its_runtime_transfer_gate() {
        let config = &runtime().thunderwind;
        let wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let player = rlogs_events::EntityRef {
            actor_id: rlogs_events::ActorId(2),
            entity_uuid: rlogs_events::EntityUuid(20),
        };
        let recipient = rlogs_events::EntityRef {
            actor_id: rlogs_events::ActorId(4),
            entity_uuid: rlogs_events::EntityUuid(40),
        };
        let proxy = rlogs_events::EntityRef {
            actor_id: rlogs_events::ActorId(1_144),
            entity_uuid: rlogs_events::EntityUuid(11_440),
        };
        let mut projector = BpsrStateDamageContributionProjector {
            current_wire: Some(wire),
            states: HashMap::from([(
                4,
                ActorHpState {
                    critical_chance_raw: Some(10_000),
                    critical_damage_raw: Some(5_000),
                    ..ActorHpState::default()
                },
            )]),
            staged_states: HashMap::from([(
                4,
                ActorHpState {
                    critical_chance_raw: Some(11_852),
                    critical_damage_raw: Some(5_566),
                    ..ActorHpState::default()
                },
            )]),
            actor_ancestry: test_ancestry(&[(2, 20), (4, 40), (1_144, 11_440)]),
            latest_observed_micros: 1,
            entity_type_by_actor: HashMap::from([(
                1_144,
                i32::try_from(config.summon_entity_type_id).unwrap(),
            )]),
            active_players: HashSet::from([2, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let proxy_attributes = [
            integer_test_attribute(config.summon_config_attribute_id, config.summon_config_id),
            integer_test_attribute(config.summon_owner_attribute_ids[0], 20),
            integer_test_attribute(config.summon_owner_attribute_ids[1], 20),
        ];
        projector.observe_canonical_ownership(
            1_144,
            11_440,
            Some(&rlogs_events::ActorOwnershipUpdate::Confirmed {
                owner_entity_uuid: rlogs_events::EntityUuid(20),
            }),
            1,
        );
        projector.observe_thunderwind_proxy_attributes(1_144, &proxy_attributes);
        projector.observe_thunderwind_status(
            &rlogs_events::StatusEvent {
                source: Some(proxy),
                target: recipient,
                effect: rlogs_events::StatusEffectId(config.effect_id),
                instance_id: Some(rlogs_events::StatusEffectInstanceId(77)),
                origin: None,
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: Some(20_000),
                level: Some(1),
                part_id: None,
                count: None,
                created_at_millis: None,
            },
            1,
        );
        projector.observe_thunderwind_child_status(&rlogs_events::StatusEvent {
            source: Some(recipient),
            target: recipient,
            effect: rlogs_events::StatusEffectId(config.child_effect_id),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(78)),
            origin: Some(rlogs_events::StatusOrigin {
                source_type_id: 1,
                source_config_id: config.child_source_config_id,
            }),
            state: StatusState::Applied,
            stacks: Some(1),
            duration_millis: Some(20_000),
            level: Some(1),
            part_id: None,
            count: None,
            created_at_millis: None,
        });
        projector.reconcile_thunderwind_staged_states();

        let state = projector.staged_states.get(&4).unwrap();
        assert_eq!(
            state.thunderwind_providers,
            BTreeMap::from([(
                player.actor_id.0,
                ThunderwindProviderState {
                    critical_chance_raw_delta: 1_852,
                    critical_damage_raw_delta: 566,
                },
            )])
        );

        projector.states.insert(4, state.clone());
        projector.thunderwind_transition_wires.clear();
        let damage = critical_test_damage(recipient, 100_000);
        let (numerator, denominator) = exact_external_critical_chance_and_damage_fraction(
            damage.amount,
            11_852,
            1_852,
            5_566,
            566,
            CriticalDamageFactorInterpretation::AdditiveBonus,
        )
        .unwrap();
        assert!(
            numerator > 0 && denominator > 0,
            "offline additive candidate remains auditable"
        );
        assert_eq!(projector.thunderwind_contribution(123, &damage), None);
        assert_eq!(
            projector.thunderwind_audit_gate(&damage),
            "emitted",
            "the candidate remains arithmetically auditable while production transfer stays disabled",
        );
    }

    #[test]
    fn thunderwind_wrong_delta_self_provider_and_missing_child_fail_closed() {
        let vector = runtime().thunderwind.packet_proven_vectors[0];
        let mut projector = BpsrStateDamageContributionProjector {
            states: HashMap::from([(
                4,
                ActorHpState {
                    critical_chance_raw: Some(10_000),
                    critical_damage_raw: Some(5_000),
                    ..ActorHpState::default()
                },
            )]),
            staged_states: HashMap::from([(
                4,
                ActorHpState {
                    critical_chance_raw: Some(11_851),
                    critical_damage_raw: Some(5_566),
                    ..ActorHpState::default()
                },
            )]),
            thunderwind_windows: HashMap::from([(
                EffectWindowKey {
                    target_actor_id: 4,
                    provider_actor_id: 1_144,
                    instance_id: Some(1),
                },
                ThunderwindWindow { source_level: 1 },
            )]),
            actor_ancestry: test_ancestry(&[(1_144, 11_440), (4, 40)]),
            latest_observed_micros: 1,
            active_players: HashSet::from([4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        projector.actor_ancestry.observe_relation(
            1,
            test_entity(1_144, 11_440),
            test_entity(4, 40),
            ActorOwnershipEvidence::ConfirmedEntityAttributes,
        );

        projector.reconcile_thunderwind_staged_states();
        assert!(
            projector
                .staged_states
                .get(&4)
                .unwrap()
                .thunderwind_providers
                .is_empty(),
            "a parent without the packet-proven hidden child cannot attribute"
        );

        projector.thunderwind_child_targets.insert(4);
        projector.reconcile_thunderwind_staged_states();
        assert!(
            projector
                .staged_states
                .get(&4)
                .unwrap()
                .thunderwind_providers
                .is_empty(),
            "an off-by-one critical transition cannot attribute"
        );

        projector.states.insert(
            4,
            ActorHpState {
                critical_chance_raw: Some(11_852),
                critical_damage_raw: Some(5_566),
                thunderwind_providers: BTreeMap::from([(
                    4,
                    ThunderwindProviderState {
                        critical_chance_raw_delta: vector.critical_chance_raw_delta,
                        critical_damage_raw_delta: vector.critical_damage_raw_delta,
                    },
                )]),
                ..ActorHpState::default()
            },
        );
        let recipient = rlogs_events::EntityRef {
            actor_id: rlogs_events::ActorId(4),
            entity_uuid: rlogs_events::EntityUuid(40),
        };
        assert_eq!(
            projector.thunderwind_contribution(123, &critical_test_damage(recipient, 100_000)),
            None,
            "self-supplied Thunderwind remains personal DPS"
        );
    }

    #[test]
    fn run_entry_keeps_actor_kind_identity_observed_before_the_boundary() {
        let mut projector = BpsrStateDamageContributionProjector {
            current_wire: Some(WireKey {
                connection_id: 1,
                stream_id: 2,
                capture_sequence: 3,
            }),
            states: HashMap::from([(4, ActorHpState::default())]),
            team_luck_windows: HashSet::from([TeamLuckWindowKey {
                target_actor_id: 4,
                target_entity_uuid: 40,
                provider_actor_id: 2,
                provider_entity_uuid: 20,
                instance_id: Some(1),
            }]),
            actor_ancestry: test_ancestry(&[(1_144, 11_440), (4, 40)]),
            latest_observed_micros: 1,
            active_players: HashSet::from([2, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        projector.actor_ancestry.observe_relation(
            1,
            test_entity(1_144, 11_440),
            test_entity(4, 40),
            ActorOwnershipEvidence::AttributedCombatSource,
        );

        projector.clear_run_state();

        assert!(projector.current_wire.is_none());
        assert!(projector.states.is_empty());
        assert!(projector.team_luck_windows.is_empty());
        assert_eq!(projector.resolve_owner_actor_id(1_144), 1_144);
        assert_eq!(projector.active_players, HashSet::from([2, 4]));

        projector.clear_state();
        assert!(projector.active_players.is_empty());
    }

    #[test]
    fn authoritative_attribute_snapshot_drops_stale_formula_state_and_decodes_zero() {
        let rule = &state_rdps_catalog().unwrap().candidate_rules[0].rule;
        let mut projector = BpsrStateDamageContributionProjector {
            states: HashMap::from([(
                4,
                ActorHpState {
                    current_value: Some(700_000),
                    final_value: Some(912_334),
                    base_value: Some(473_072),
                    extra_add: Some(20_000),
                    raw_percent: Some(2_700),
                    provider_raw_percent: BTreeMap::from([(2, 500)]),
                    ..ActorHpState::default()
                },
            )]),
            ..BpsrStateDamageContributionProjector::default()
        };

        projector.observe_attributes(
            4,
            40,
            EntityAttributeUpdateKind::Snapshot,
            None,
            &[EntityAttribute {
                attribute_id: rule.percentage_attribute_id,
                decoded: None,
                raw_value: Vec::new(),
            }],
            1,
        );

        let state = projector.staged_states.get(&4).unwrap();
        assert_eq!(state.raw_percent, Some(0));
        assert_eq!(state.current_value, None);
        assert_eq!(state.final_value, None);
        assert_eq!(state.base_value, None);
        assert_eq!(state.extra_add, None);
        assert!(state.provider_raw_percent.is_empty());
    }

    #[test]
    fn hp_snapshot_and_delta_retain_current_and_complete_max_hp_family() {
        let rule = &state_rdps_catalog().unwrap().candidate_rules[0].rule;
        let mut projector = BpsrStateDamageContributionProjector::default();

        projector.observe_attributes(
            4,
            40,
            EntityAttributeUpdateKind::Snapshot,
            None,
            &[
                integer_test_attribute(ATTR_CURRENT_HP, 700_000),
                integer_test_attribute(rule.final_attribute_id, 900_000),
                integer_test_attribute(rule.base_attribute_id, 500_000),
                integer_test_attribute(ATTR_MAX_HP_EXTRA_ADD, 25_000),
            ],
            1,
        );

        let snapshot = projector.staged_states.get(&4).unwrap();
        assert_eq!(snapshot.current_value, Some(700_000));
        assert_eq!(snapshot.final_value, Some(900_000));
        assert_eq!(snapshot.base_value, Some(500_000));
        assert_eq!(snapshot.extra_add, Some(25_000));

        projector.observe_attributes(
            4,
            40,
            EntityAttributeUpdateKind::Delta,
            None,
            &[
                integer_test_attribute(ATTR_CURRENT_HP, 650_000),
                integer_test_attribute(ATTR_MAX_HP_EXTRA_ADD, 30_000),
            ],
            2,
        );

        let delta = projector.staged_states.get(&4).unwrap();
        assert_eq!(delta.current_value, Some(650_000));
        assert_eq!(delta.final_value, Some(900_000));
        assert_eq!(delta.base_value, Some(500_000));
        assert_eq!(delta.extra_add, Some(30_000));
    }

    #[test]
    fn exact_packet_state_emits_only_external_marginal() {
        let projector = BpsrStateDamageContributionProjector {
            states: HashMap::from([(
                4,
                ActorHpState {
                    final_value: Some(912_334),
                    base_value: Some(473_072),
                    raw_percent: Some(2_700),
                    intermediate_value: Some(600_807),
                    raw_extra_percent: Some(5_185),
                    provider_raw_percent: BTreeMap::from([(2, 500)]),
                    ..ActorHpState::default()
                },
            )]),
            active_players: HashSet::from([2, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        assert!(projector.class_id_by_actor.is_empty());
        assert!(projector.observed_ability_ids_by_actor.is_empty());
        let rule = &state_rdps_catalog().unwrap().candidate_rules[0].rule;
        assert_eq!(
            projector.exact_damage_marginal(rule, 4, 2_737_001),
            Some((2, 107_757))
        );
        assert_eq!(projector.exact_damage_marginal(rule, 4, 2_737_002), None);
    }

    #[test]
    fn self_supplied_hp_and_unproven_multi_provider_state_never_transfer() {
        let rule = &state_rdps_catalog().unwrap().candidate_rules[0].rule;
        for providers in [
            BTreeMap::from([(4, 500)]),
            BTreeMap::from([(2, 250), (3, 250)]),
        ] {
            let projector = BpsrStateDamageContributionProjector {
                states: HashMap::from([(
                    4,
                    ActorHpState {
                        final_value: Some(912_334),
                        base_value: Some(473_072),
                        raw_percent: Some(2_700),
                        intermediate_value: Some(600_807),
                        raw_extra_percent: Some(5_185),
                        provider_raw_percent: providers,
                        ..ActorHpState::default()
                    },
                )]),
                active_players: HashSet::from([2, 3, 4]),
                ..BpsrStateDamageContributionProjector::default()
            };
            assert_eq!(projector.exact_damage_marginal(rule, 4, 2_737_001), None);
        }
    }

    #[test]
    fn functional_amp_replays_packet_attribute_and_damage_stages_exactly() {
        let family = AttackFamilyState {
            final_value: Some(5_569),
            intermediate_value: Some(5_359),
            base_add: Some(4_620),
            extra_add: Some(210),
            raw_percent: Some(1_600),
            raw_percent_packet_observed: true,
            provider_raw_percent: BTreeMap::from([(
                2,
                runtime().functional_amp.attack_percent_raw_delta,
            )]),
        };
        let wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let mut projector = BpsrStateDamageContributionProjector {
            current_wire: Some(wire),
            states: HashMap::from([(
                4,
                ActorHpState {
                    physical_attack: family,
                    ..ActorHpState::default()
                },
            )]),
            functional_amp_windows: HashSet::from([FunctionalAmpWindowKey {
                target_actor_id: 4,
                target_entity_uuid: 40,
                provider_actor_id: 2,
                provider_entity_uuid: 20,
                instance_id: Some(11),
            }]),
            actor_ancestry: test_ancestry(&[(2, 20), (4, 40)]),
            active_players: HashSet::from([2, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let damage = rlogs_events::DamageEvent {
            source: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            direct_source: None,
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(17),
                entity_uuid: rlogs_events::EntityUuid(170),
            },
            ability: Some(rlogs_events::AbilityId(2_203_291)),
            amount: 100_000,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(7),
            damage_source: None,
            damage_type: Some(2),
            flags: rlogs_events::DamageFlags::default(),
            packet: rlogs_events::DamagePacketDetail {
                owner_level: Some(2),
                owner_stage: Some(1),
                ..rlogs_events::DamagePacketDetail::default()
            },
        };

        let observed_windows = std::mem::take(&mut projector.functional_amp_windows);
        assert_eq!(
            projector.functional_amp_contribution(123, &damage),
            None,
            "an armed rule must remain dormant without an observed provider-recipient lifecycle"
        );
        projector.functional_amp_windows = observed_windows;

        assert_eq!(
            projector.functional_amp_contribution(123, &damage),
            Some(ExactRationalDamageContributionEvent {
                observed_micros: 123,
                effect_id: runtime().functional_amp.effect_id,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                numerator: 13_000_000,
                denominator: 4_361,
                observed_damage: 100_000,
                included: true,
            })
        );

        projector.actor_ancestry.observe_entity(test_entity(9, 20));
        assert_eq!(
            projector.functional_amp_contribution(123, &damage),
            None,
            "a recycled provider actor cannot inherit the prior entity's aura credit"
        );
        projector.actor_ancestry.observe_entity(test_entity(2, 20));
        let mut recycled_recipient = damage.clone();
        recycled_recipient.source.entity_uuid = rlogs_events::EntityUuid(41);
        assert_eq!(
            projector.functional_amp_contribution(123, &recycled_recipient),
            None,
            "a recycled recipient actor cannot inherit the prior entity's aura window"
        );

        projector.functional_amp_transition_wires.insert(3, wire);
        assert!(
            projector
                .functional_amp_contribution(123, &damage)
                .is_some(),
            "a same-wire refresh for another recipient cannot suppress stable actor 4"
        );
        projector.functional_amp_transition_wires.insert(4, wire);
        assert_eq!(
            projector.functional_amp_contribution(123, &damage),
            None,
            "actor 4 is excluded only on its own packet-transition wire"
        );
    }

    #[test]
    fn harmony_grace_primary_marginal_requires_an_uncontaminated_packet_transition() {
        let wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let desired = BTreeMap::from([(2, 200)]);
        let previous = AttackFamilyState {
            final_value: Some(8_301),
            intermediate_value: Some(8_301),
            base_add: Some(6_976),
            extra_add: Some(0),
            raw_percent: Some(1_900),
            raw_percent_packet_observed: true,
            provider_raw_percent: BTreeMap::new(),
        };
        let current = AttackFamilyState {
            final_value: Some(8_441),
            intermediate_value: Some(8_441),
            base_add: Some(6_976),
            extra_add: Some(0),
            raw_percent: Some(2_100),
            raw_percent_packet_observed: true,
            provider_raw_percent: desired.clone(),
        };
        let (provider, witness) =
            exact_primary_stat_transition_witness(Some(wire), &previous, &current, &desired)
                .expect("captured +200 -> +140 transition should be exact");
        assert_eq!(provider, 2);
        assert_eq!(witness.provider_primary_marginal, 140);
        assert_eq!(witness.active_raw_percent, 2_100);

        let contaminated = AttackFamilyState {
            base_add: Some(6_841),
            ..current
        };
        assert_eq!(
            exact_primary_stat_transition_witness(Some(wire), &previous, &contaminated, &desired,),
            None,
            "a simultaneous base change must not become a provider witness"
        );
    }

    #[test]
    fn harmony_grace_replays_primary_conversion_attack_and_damage_stages_exactly() {
        assert_eq!(fixed_point_stage_term(6_976, 11_700), Some(8_161));
        assert_eq!(fixed_point_stage_term_nearest(6_976, 11_700), Some(8_162));
        assert_eq!(
            fixed_point_stage_term_nearest_non_tie(6_976, 11_700),
            Some(8_162)
        );
        assert_eq!(fixed_point_stage_term_nearest_non_tie(5, 11_000), None);
        let recipient_rule = runtime()
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == 11)
            .unwrap();
        let wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let desired = BTreeMap::from([(2, recipient_rule.primary_percent_raw_delta)]);
        let mut projector = BpsrStateDamageContributionProjector {
            harmony_grace_candidate_audit_enabled: true,
            current_wire: Some(wire),
            states: HashMap::from([(
                4,
                ActorHpState {
                    physical_attack: AttackFamilyState {
                        final_value: Some(9_145),
                        intermediate_value: Some(9_145),
                        base_add: Some(6_981),
                        extra_add: Some(0),
                        raw_percent: Some(3_100),
                        raw_percent_packet_observed: true,
                        provider_raw_percent: BTreeMap::new(),
                    },
                    harmony_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            final_value: Some(11_301),
                            intermediate_value: Some(8_476),
                            base_add: Some(6_120),
                            extra_add: Some(2_825),
                            raw_percent: Some(3_850),
                            raw_percent_packet_observed: true,
                            provider_raw_percent: desired,
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            class_id_by_actor: HashMap::from([(4, recipient_rule.recipient_class_id)]),
            harmony_grace_windows: HashSet::from([
                EffectWindowKey {
                    target_actor_id: 4,
                    provider_actor_id: 2,
                    instance_id: Some(11),
                },
                EffectWindowKey {
                    target_actor_id: 169,
                    provider_actor_id: 2,
                    instance_id: None,
                },
            ]),
            harmony_grace_primary_transition_witnesses: HashMap::from([(
                EffectWindowKey {
                    target_actor_id: 4,
                    provider_actor_id: 2,
                    instance_id: Some(11),
                },
                HashSet::from([PrimaryStatTransitionWitness {
                    wire,
                    instance_id: Some(11),
                    base_add: 6_120,
                    active_raw_percent: 3_850,
                    provider_raw_percent: 200,
                    provider_primary_marginal: 122,
                }]),
            )]),
            active_players: HashSet::from([2, 4]),
            observed_ability_ids_by_actor: HashMap::from([(4, HashSet::from([2_233]))]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let damage = rlogs_events::DamageEvent {
            source: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            direct_source: None,
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(17),
                entity_uuid: rlogs_events::EntityUuid(170),
            },
            ability: Some(rlogs_events::AbilityId(2_203_521)),
            amount: 70_543,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(5),
            damage_source: None,
            damage_type: Some(2),
            flags: rlogs_events::DamageFlags::default(),
            packet: rlogs_events::DamagePacketDetail::default(),
        };

        assert_eq!(
            projector.harmony_grace_contribution(123, &damage),
            Some(ExactRationalDamageContributionEvent {
                observed_micros: 123,
                effect_id: runtime().harmony_grace.effect_id,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                numerator: 211_629,
                denominator: 295,
                observed_damage: 70_543,
                included: true,
            })
        );
        let trace = projector
            .harmony_grace_formula_trace(&damage)
            .expect("exact Harmony Grace contribution should expose its arithmetic receipt");
        assert_eq!(trace.provider_primary_raw_percent, 200);
        assert_eq!(
            trace.primary_provider_marginal_basis,
            "same_lifecycle_packet_transition"
        );
        assert_eq!(trace.primary_transition_connection_id, wire.connection_id);
        assert_eq!(trace.primary_transition_stream_id, wire.stream_id);
        assert_eq!(
            trace.primary_transition_capture_sequence,
            wire.capture_sequence
        );
        assert_eq!(trace.primary_transition_instance_id, Some(11));
        assert_eq!(trace.primary_provider_marginal, 122);
        assert_eq!(trace.provider_attack_base_add, 71);
        assert_eq!(trace.provider_attack_marginal, 93);
        assert_eq!(trace.active_stage_body, 18_290);
        assert_eq!(trace.coefficient_stage_marginal, 186);
        assert_eq!(trace.contribution_numerator, 211_629);
        assert_eq!(trace.contribution_denominator, 295);

        projector.harmony_grace_primary_transition_witnesses.clear();
        assert_eq!(
            projector.harmony_grace_audit_gate(&damage),
            "primary_transition_witness_missing",
            "an otherwise complete damage row must fail closed without its exact lifecycle transition"
        );
        assert_eq!(projector.harmony_grace_contribution(123, &damage), None);
    }

    #[test]
    fn mechanical_power_tier0_candidate_uses_only_the_observed_750_delta() {
        let production = BpsrStateDamageContributionProjector::new().unwrap();
        assert_eq!(
            production.mechanical_power_candidate_primary_percent_override,
            None
        );
        assert_eq!(
            production.runtime.mechanical_power.recipient_rules[0].primary_percent_raw_delta,
            1_500
        );
        assert_eq!(
            production
                .runtime
                .mechanical_power
                .production_primary_percent_raw_delta(11),
            Some(750),
            "production must use only the exact packet-observed tier-0 transition"
        );

        let candidate =
            BpsrStateDamageContributionProjector::new_mechanical_power_tier0_candidate_audit()
                .unwrap();
        assert_eq!(
            candidate.mechanical_power_candidate_primary_percent_override,
            Some(750)
        );
        assert_eq!(
            candidate.runtime.mechanical_power.recipient_rules[0].primary_percent_raw_delta, 1_500,
            "the audit override must not mutate the versioned production rule"
        );
        assert!(!candidate.enabled());
    }

    #[test]
    fn mechanical_power_tier0_rebases_the_proven_delta_after_an_additive_change() {
        let witness = PrimaryStatTransitionWitness {
            wire: WireKey {
                connection_id: 1,
                stream_id: 2,
                capture_sequence: 3,
            },
            instance_id: Some(4),
            base_add: 10_000,
            active_raw_percent: 750,
            provider_raw_percent: 750,
            provider_primary_marginal: 750,
        };
        let current = AttackFamilyState {
            final_value: Some(11_050),
            intermediate_value: Some(10_950),
            base_add: Some(10_000),
            extra_add: Some(100),
            raw_percent: Some(950),
            ..AttackFamilyState::default()
        };

        let rebased = rebase_primary_stat_transition_witness(&current, witness).unwrap();
        assert_eq!(rebased.active_raw_percent, 950);
        assert_eq!(rebased.provider_primary_marginal, 750);
        assert_eq!(rebased.instance_id, Some(4));
    }

    #[test]
    fn mechanical_power_cross_effect_overlap_stays_fail_closed() {
        assert_eq!(
            attack_contribution_overlap_policy(false, false, true, false),
            AttackContributionOverlapPolicy::Single
        );
        assert_eq!(
            attack_contribution_overlap_policy(false, true, true, false),
            AttackContributionOverlapPolicy::Suppress
        );
        assert_eq!(
            attack_contribution_overlap_policy(true, false, true, false),
            AttackContributionOverlapPolicy::Suppress
        );
        assert_eq!(
            attack_contribution_overlap_policy(false, false, true, true),
            AttackContributionOverlapPolicy::Suppress
        );
        assert_eq!(
            attack_contribution_overlap_policy(true, true, false, false),
            AttackContributionOverlapPolicy::HarmonyFunctionalAmp
        );
    }

    #[test]
    fn attack_family_completes_only_a_unique_packet_implied_raw_percent() {
        let mut exact = AttackFamilyState {
            intermediate_value: Some(6_978),
            base_add: Some(6_016),
            ..AttackFamilyState::default()
        };
        complete_exact_raw_percent(&mut exact);
        assert_eq!(exact.raw_percent, Some(1_600));
        assert!(!exact.raw_percent_packet_observed);

        exact.base_add = Some(7_431);
        exact.intermediate_value = Some(8_620);
        complete_exact_raw_percent(&mut exact);
        assert_eq!(
            exact.raw_percent,
            Some(1_601),
            "a prior inferred value must refresh only when the later packet family has one exact solution"
        );

        let mut ambiguous = AttackFamilyState {
            intermediate_value: Some(1),
            base_add: Some(1),
            ..AttackFamilyState::default()
        };
        complete_exact_raw_percent(&mut ambiguous);
        assert_eq!(ambiguous.raw_percent, None);

        let mut explicit = AttackFamilyState {
            intermediate_value: Some(6_978),
            base_add: Some(6_016),
            raw_percent: Some(1_601),
            raw_percent_packet_observed: true,
            ..AttackFamilyState::default()
        };
        complete_exact_raw_percent(&mut explicit);
        assert_eq!(explicit.raw_percent, Some(1_601));
    }

    #[test]
    fn mechanical_power_replays_marksman_primary_conversion_and_attack_stage_exactly() {
        let recipient_rule = &runtime().mechanical_power.recipient_rules[0];
        let desired = BTreeMap::from([(2, recipient_rule.primary_percent_raw_delta)]);
        let wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let mut projector = BpsrStateDamageContributionProjector {
            mechanical_power_candidate_audit_enabled: true,
            mechanical_power_candidate_primary_percent_override: Some(
                recipient_rule.primary_percent_raw_delta,
            ),
            current_wire: Some(wire),
            states: HashMap::from([(
                4,
                ActorHpState {
                    physical_attack: AttackFamilyState {
                        final_value: Some(9_137),
                        intermediate_value: Some(9_137),
                        base_add: Some(6_975),
                        extra_add: Some(0),
                        raw_percent: Some(3_100),
                        raw_percent_packet_observed: true,
                        provider_raw_percent: BTreeMap::new(),
                    },
                    mechanical_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            final_value: Some(11_383),
                            intermediate_value: Some(8_323),
                            base_add: Some(6_120),
                            extra_add: Some(3_060),
                            raw_percent: Some(3_600),
                            raw_percent_packet_observed: true,
                            provider_raw_percent: desired.clone(),
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            mechanical_power_windows: HashSet::from([EffectWindowKey {
                target_actor_id: 4,
                provider_actor_id: 2,
                instance_id: Some(11),
            }]),
            mechanical_power_primary_transition_witnesses: HashMap::from([(
                EffectWindowKey {
                    target_actor_id: 4,
                    provider_actor_id: 2,
                    instance_id: Some(11),
                },
                HashSet::from([PrimaryStatTransitionWitness {
                    wire,
                    instance_id: Some(11),
                    base_add: 6_120,
                    active_raw_percent: 3_600,
                    provider_raw_percent: recipient_rule.primary_percent_raw_delta,
                    provider_primary_marginal: 918,
                }]),
            )]),
            class_id_by_actor: HashMap::from([(4, recipient_rule.recipient_class_id)]),
            active_players: HashSet::from([2, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let damage = rlogs_events::DamageEvent {
            source: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            direct_source: None,
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(17),
                entity_uuid: rlogs_events::EntityUuid(170),
            },
            ability: Some(rlogs_events::AbilityId(2_203_521)),
            amount: 70_543,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(5),
            damage_source: None,
            damage_type: Some(2),
            flags: rlogs_events::DamageFlags::default(),
            packet: rlogs_events::DamagePacketDetail::default(),
        };

        let contribution = projector
            .mechanical_power_contribution(123, &damage)
            .expect("the exact captured Mechanical Power chain should conserve");
        assert_eq!(contribution.effect_id, runtime().mechanical_power.effect_id);
        assert_eq!(contribution.provider_actor_id, 2);
        assert_eq!(contribution.recipient_actor_id, 4);
        assert!(contribution.numerator > 0);
        assert!(
            contribution.numerator
                < i128::from(contribution.observed_damage) * contribution.denominator
        );

        let active_windows = projector.mechanical_power_windows.clone();
        projector.mechanical_power_windows.clear();
        let mut unrelated_nonstandard_damage = damage.clone();
        unrelated_nonstandard_damage.ability = Some(rlogs_events::AbilityId(2_002_441));
        unrelated_nonstandard_damage.hit_event_id = Some(2);
        assert_eq!(
            projector.mechanical_power_audit_gate(&unrelated_nonstandard_damage),
            "provider_window_missing",
            "an action outside the provider lifecycle must not become a formula proof obligation"
        );
        projector.mechanical_power_windows = active_windows;

        projector
            .mechanical_power_primary_transition_witnesses
            .clear();
        assert_eq!(
            projector.mechanical_power_audit_gate(&damage),
            "primary_transition_witness_missing",
            "an otherwise complete Mechanical Power row must fail closed without its exact lifecycle transition"
        );
        projector
            .mechanical_power_primary_transition_witnesses
            .insert(
                EffectWindowKey {
                    target_actor_id: 4,
                    provider_actor_id: 2,
                    instance_id: Some(11),
                },
                HashSet::from([PrimaryStatTransitionWitness {
                    wire,
                    instance_id: Some(11),
                    base_add: 6_120,
                    active_raw_percent: 3_600,
                    provider_raw_percent: recipient_rule.primary_percent_raw_delta,
                    provider_primary_marginal: 918,
                }]),
            );

        projector.mechanical_power_transition_wires.insert(4, wire);
        assert_eq!(projector.mechanical_power_contribution(123, &damage), None);
        projector.mechanical_power_transition_wires.clear();
        projector.mechanical_power_windows = HashSet::from([EffectWindowKey {
            target_actor_id: 4,
            provider_actor_id: 4,
            instance_id: Some(11),
        }]);
        projector
            .states
            .get_mut(&4)
            .unwrap()
            .mechanical_primary_by_class
            .get_mut(&recipient_rule.recipient_class_id)
            .unwrap()
            .provider_raw_percent = BTreeMap::from([(4, recipient_rule.primary_percent_raw_delta)]);
        assert_eq!(
            projector.mechanical_power_contribution(123, &damage),
            None,
            "self-supplied Mechanical Power remains personal DPS"
        );
    }

    #[test]
    fn mechanical_power_requires_the_exact_recipient_percent_transition() {
        let recipient_rule = &runtime().mechanical_power.recipient_rules[0];
        let desired = BTreeMap::from([(2, recipient_rule.primary_percent_raw_delta)]);
        let mut exact = AttackFamilyState {
            raw_percent: Some(4_000),
            ..AttackFamilyState::default()
        };
        reconcile_external_percent_family(Some(2_500), &mut exact, &desired);
        assert_eq!(exact.provider_raw_percent, desired);

        let mut contaminated = AttackFamilyState {
            raw_percent: Some(4_050),
            ..AttackFamilyState::default()
        };
        reconcile_external_percent_family(Some(2_500), &mut contaminated, &desired);
        assert!(
            contaminated.provider_raw_percent.is_empty(),
            "the observed +1550 transition cannot be relabeled as the proven +1500 provider"
        );
    }

    #[test]
    fn harmony_and_functional_amp_use_adjacent_attack_stages_once() {
        let recipient_rule = runtime()
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == 11)
            .unwrap();
        let harmony_desired = BTreeMap::from([(2, recipient_rule.primary_percent_raw_delta)]);
        let functional_desired =
            BTreeMap::from([(3, runtime().functional_amp.attack_percent_raw_delta)]);
        let projector = BpsrStateDamageContributionProjector {
            harmony_grace_candidate_audit_enabled: true,
            states: HashMap::from([(
                4,
                ActorHpState {
                    physical_attack: AttackFamilyState {
                        final_value: Some(9_145),
                        intermediate_value: Some(9_145),
                        base_add: Some(6_981),
                        extra_add: Some(0),
                        raw_percent: Some(3_100),
                        raw_percent_packet_observed: true,
                        provider_raw_percent: functional_desired,
                    },
                    harmony_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            final_value: Some(11_301),
                            intermediate_value: Some(8_476),
                            base_add: Some(6_120),
                            extra_add: Some(2_825),
                            raw_percent: Some(3_850),
                            raw_percent_packet_observed: true,
                            provider_raw_percent: harmony_desired,
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            class_id_by_actor: HashMap::from([(4, recipient_rule.recipient_class_id)]),
            functional_amp_windows: HashSet::from([FunctionalAmpWindowKey {
                target_actor_id: 4,
                target_entity_uuid: 40,
                provider_actor_id: 3,
                provider_entity_uuid: 30,
                instance_id: Some(12),
            }]),
            harmony_grace_windows: HashSet::from([EffectWindowKey {
                target_actor_id: 4,
                provider_actor_id: 2,
                instance_id: Some(11),
            }]),
            harmony_grace_primary_transition_witnesses: HashMap::from([(
                EffectWindowKey {
                    target_actor_id: 4,
                    provider_actor_id: 2,
                    instance_id: Some(11),
                },
                HashSet::from([PrimaryStatTransitionWitness {
                    wire: WireKey {
                        connection_id: 1,
                        stream_id: 2,
                        capture_sequence: 3,
                    },
                    instance_id: Some(11),
                    base_add: 6_120,
                    active_raw_percent: 3_850,
                    provider_raw_percent: 200,
                    provider_primary_marginal: 122,
                }]),
            )]),
            active_players: HashSet::from([2, 3, 4]),
            actor_ancestry: test_ancestry(&[(2, 20), (3, 30), (4, 40)]),
            observed_ability_ids_by_actor: HashMap::from([(4, HashSet::from([2_233]))]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let damage = rlogs_events::DamageEvent {
            source: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            direct_source: None,
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(17),
                entity_uuid: rlogs_events::EntityUuid(170),
            },
            ability: Some(rlogs_events::AbilityId(2_203_521)),
            amount: 70_543,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(5),
            damage_source: None,
            damage_type: Some(2),
            flags: rlogs_events::DamageFlags::default(),
            packet: rlogs_events::DamagePacketDetail::default(),
        };
        let functional = projector
            .functional_amp_contribution(123, &damage)
            .expect("packet-proven Functional Amp");
        let harmony = projector
            .harmony_grace_contribution(123, &damage)
            .expect("packet-proven Harmony Grace");
        let combined = projector
            .combined_harmony_functional_amp_contributions(123, &damage, functional, harmony)
            .expect("ordered combined Attack stages");

        assert_eq!(combined[0].effect_id, runtime().harmony_grace.effect_id);
        assert_eq!(combined[0].provider_actor_id, 2);
        assert_eq!(combined[1].effect_id, runtime().functional_amp.effect_id);
        assert_eq!(combined[1].provider_actor_id, 3);
        let selected = select_damage_stage(2_203_521, Some(5), None, None, None).unwrap();
        assert_eq!(
            (combined[0].numerator, combined[0].denominator),
            exact_external_attack_ordered_stage_fraction(
                damage.amount,
                PacketDamageScriptFamily::StandardAttack,
                9_145,
                9_145,
                9_052,
                selected.coefficient_basis_points,
                selected.fixed_parameter,
            )
            .unwrap()
        );
        assert_eq!(
            (combined[1].numerator, combined[1].denominator),
            exact_external_attack_ordered_stage_fraction(
                damage.amount,
                PacketDamageScriptFamily::StandardAttack,
                9_145,
                9_052,
                8_803,
                selected.coefficient_basis_points,
                selected.fixed_parameter,
            )
            .unwrap()
        );
        let combined_numerator = combined[0]
            .numerator
            .checked_mul(combined[1].denominator)
            .unwrap()
            .checked_add(
                combined[1]
                    .numerator
                    .checked_mul(combined[0].denominator)
                    .unwrap(),
            )
            .unwrap();
        let combined_denominator = combined[0]
            .denominator
            .checked_mul(combined[1].denominator)
            .unwrap();
        let direct = exact_external_attack_ordered_stage_fraction(
            damage.amount,
            PacketDamageScriptFamily::StandardAttack,
            9_145,
            9_145,
            8_803,
            selected.coefficient_basis_points,
            selected.fixed_parameter,
        )
        .unwrap();
        assert_eq!(
            combined_numerator * direct.1,
            direct.0 * combined_denominator
        );
    }

    #[test]
    fn harmony_grace_rejects_self_provider_missing_decomposition_and_wrong_attack_lane() {
        let recipient_rule = runtime()
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == 11)
            .unwrap();
        let mut projector = BpsrStateDamageContributionProjector {
            states: HashMap::from([(
                4,
                ActorHpState {
                    physical_attack: AttackFamilyState {
                        final_value: Some(9_145),
                        intermediate_value: Some(9_145),
                        base_add: Some(6_981),
                        extra_add: Some(0),
                        raw_percent: Some(3_100),
                        ..AttackFamilyState::default()
                    },
                    harmony_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            final_value: Some(11_301),
                            intermediate_value: Some(8_476),
                            base_add: Some(6_120),
                            extra_add: Some(2_825),
                            raw_percent: Some(3_850),
                            ..AttackFamilyState::default()
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            class_id_by_actor: HashMap::from([(4, recipient_rule.recipient_class_id)]),
            harmony_grace_windows: HashSet::from([EffectWindowKey {
                target_actor_id: 4,
                provider_actor_id: 2,
                instance_id: Some(11),
            }]),
            active_players: HashSet::from([2, 4]),
            observed_ability_ids_by_actor: HashMap::from([(4, HashSet::from([2_233]))]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let mut damage = rlogs_events::DamageEvent {
            source: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            direct_source: None,
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(17),
                entity_uuid: rlogs_events::EntityUuid(170),
            },
            ability: Some(rlogs_events::AbilityId(2_203_521)),
            amount: 70_543,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(5),
            damage_source: None,
            damage_type: Some(2),
            flags: rlogs_events::DamageFlags::default(),
            packet: rlogs_events::DamagePacketDetail::default(),
        };
        assert_eq!(projector.harmony_grace_contribution(123, &damage), None);

        projector
            .states
            .get_mut(&4)
            .unwrap()
            .harmony_primary_by_class
            .get_mut(&recipient_rule.recipient_class_id)
            .unwrap()
            .provider_raw_percent = BTreeMap::from([(2, 200)]);
        projector.harmony_grace_windows = HashSet::from([EffectWindowKey {
            target_actor_id: 4,
            provider_actor_id: 4,
            instance_id: Some(11),
        }]);
        projector
            .states
            .get_mut(&4)
            .unwrap()
            .harmony_primary_by_class
            .get_mut(&recipient_rule.recipient_class_id)
            .unwrap()
            .provider_raw_percent = BTreeMap::from([(4, 200)]);
        assert_eq!(projector.harmony_grace_contribution(123, &damage), None);

        damage.ability = Some(rlogs_events::AbilityId(1_607));
        projector
            .observed_ability_ids_by_actor
            .insert(4, HashSet::from([1_607, 1_608, 1_612]));
        projector.harmony_grace_windows = HashSet::from([EffectWindowKey {
            target_actor_id: 4,
            provider_actor_id: 2,
            instance_id: Some(11),
        }]);
        projector
            .states
            .get_mut(&4)
            .unwrap()
            .harmony_primary_by_class
            .get_mut(&recipient_rule.recipient_class_id)
            .unwrap()
            .provider_raw_percent = BTreeMap::from([(2, 200)]);
        assert_eq!(projector.harmony_grace_contribution(123, &damage), None);
    }

    #[test]
    fn inspiration_base_add_uses_the_same_conserved_attack_stage_without_becoming_percent() {
        let family = AttackFamilyState {
            final_value: Some(5_823),
            intermediate_value: Some(5_613),
            base_add: Some(4_839),
            extra_add: Some(210),
            raw_percent: Some(1_600),
            raw_percent_packet_observed: true,
            provider_raw_percent: BTreeMap::new(),
        };
        let selected = SelectedDamageStage {
            damage_attr_id: 2_220_329_107,
            offensive_stat: OffensiveStatKind::PhysicalAttack,
            coefficient_basis_points: 20_000,
            fixed_parameter: 0,
        };
        assert_eq!(
            exact_attack_family_stage_contribution(
                123,
                runtime().inspiration.effect_id,
                2,
                4,
                100_000,
                &family,
                360,
                0,
                selected,
            ),
            Some(ExactRationalDamageContributionEvent {
                observed_micros: 123,
                effect_id: runtime().inspiration.effect_id,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                numerator: 41_800_000,
                denominator: 5_823,
                observed_damage: 100_000,
                included: true,
            })
        );
        assert_eq!(
            exact_attack_family_stage_contribution(
                123,
                runtime().inspiration.effect_id,
                2,
                4,
                100_000,
                &family,
                360,
                300,
                selected,
            ),
            None,
            "base-add and raw-percent components cannot be collapsed into one guessed input"
        );
    }

    #[test]
    fn functional_amp_decomposition_requires_the_exact_packet_delta() {
        let desired = BTreeMap::from([(2, runtime().functional_amp.attack_percent_raw_delta)]);
        let mut family = AttackFamilyState {
            raw_percent: Some(1_600),
            ..AttackFamilyState::default()
        };
        reconcile_external_percent_family(Some(1_240), &mut family, &desired);
        assert_eq!(family.provider_raw_percent, desired);

        family.raw_percent = Some(1_980);
        reconcile_external_percent_family(Some(1_600), &mut family, &BTreeMap::new());
        assert!(family.provider_raw_percent.is_empty());
    }

    #[test]
    fn inspiration_occurrence_dispatch_preserves_each_proven_chance_stage() {
        let observed_damage = 987_654;
        let critical_chance = 7_416;
        let lucky_chance = 800;
        let provider_delta = 300;
        let critical_damage = 15_000;

        assert_eq!(
            exact_inspiration_occurrence_fraction(
                observed_damage,
                true,
                false,
                critical_chance,
                lucky_chance,
                provider_delta,
                critical_damage,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            exact_external_critical_chance_fraction(
                observed_damage,
                critical_chance,
                provider_delta,
                critical_damage,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            )
        );
        assert_eq!(
            exact_inspiration_occurrence_fraction(
                observed_damage,
                false,
                true,
                critical_chance,
                lucky_chance,
                provider_delta,
                critical_damage,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            exact_external_lucky_chance_fraction(observed_damage, lucky_chance, provider_delta,)
        );
        assert_eq!(
            exact_inspiration_occurrence_fraction(
                observed_damage,
                true,
                true,
                critical_chance,
                lucky_chance,
                provider_delta,
                critical_damage,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            exact_external_combined_critical_lucky_chance_fraction(
                observed_damage,
                critical_chance,
                provider_delta,
                lucky_chance,
                provider_delta,
                critical_damage,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            )
        );
        assert_eq!(
            exact_inspiration_occurrence_fraction(
                observed_damage,
                false,
                false,
                critical_chance,
                lucky_chance,
                provider_delta,
                critical_damage,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            None
        );
    }

    #[test]
    fn inspiration_normal_vector_uses_the_exact_recipient_packet_transition() {
        let previous = ActorHpState {
            primary_raw_add: [Some(477), Some(477), Some(6_076), Some(38_833)],
            critical_chance_raw: Some(7_116),
            critical_chance_raw_add: Some(6_116),
            lucky_chance_raw: Some(500),
            lucky_chance_raw_add: Some(500),
            mastery_raw: Some(600),
            mastery_raw_add: Some(600),
            versatility_raw: Some(1_644),
            versatility_raw_add: Some(1_644),
            external_damage_raw: Some(575),
            property_damage_raw: Some(12_858),
            haste_percent_basis_points: Some(3_918),
            physical_attack: AttackFamilyState {
                base_add: Some(4_479),
                ..AttackFamilyState::default()
            },
            ..ActorHpState::default()
        };
        let mut next = ActorHpState {
            primary_raw_add: [Some(1_017), Some(1_017), Some(6_616), Some(39_373)],
            critical_chance_raw: Some(7_416),
            critical_chance_raw_add: Some(6_416),
            lucky_chance_raw: Some(800),
            lucky_chance_raw_add: Some(800),
            mastery_raw: Some(900),
            mastery_raw_add: Some(900),
            versatility_raw: Some(1_944),
            versatility_raw_add: Some(1_944),
            external_damage_raw: Some(680),
            property_damage_raw: Some(13_038),
            haste_percent_basis_points: Some(4_300),
            physical_attack: AttackFamilyState {
                base_add: Some(4_839),
                ..AttackFamilyState::default()
            },
            ..ActorHpState::default()
        };
        reconcile_inspiration_state(
            Some(&previous),
            &mut next,
            &BTreeMap::from([(2, false)]),
            &runtime().inspiration.packet_proven_vectors,
        );
        assert_eq!(
            next.inspiration_providers,
            BTreeMap::from([(
                2,
                InspirationProviderState {
                    provider_full_bloom: false,
                    primary_raw_add_delta: 540,
                    secondary_raw_add_delta: 300,
                    physical_attack_base_add_delta: Some(360),
                    magical_attack_base_add_delta: None,
                    external_damage_delta: 105,
                    property_damage_delta: Some(180),
                    haste_delta: Some(382),
                }
            )])
        );
    }

    #[test]
    fn inspiration_delayed_property_lane_requires_its_own_exact_transition() {
        let provider = InspirationProviderState {
            provider_full_bloom: false,
            primary_raw_add_delta: 540,
            secondary_raw_add_delta: 300,
            physical_attack_base_add_delta: Some(360),
            magical_attack_base_add_delta: None,
            external_damage_delta: 105,
            property_damage_delta: None,
            haste_delta: Some(382),
        };
        let previous = ActorHpState {
            property_damage_raw: Some(12_858),
            inspiration_providers: BTreeMap::from([(2, provider)]),
            ..ActorHpState::default()
        };
        let mut next = previous.clone();
        next.property_damage_raw = Some(13_038);
        reconcile_inspiration_state(
            Some(&previous),
            &mut next,
            &BTreeMap::from([(2, false)]),
            &runtime().inspiration.packet_proven_vectors,
        );
        assert_eq!(
            next.inspiration_providers
                .get(&2)
                .unwrap()
                .property_damage_delta,
            Some(180)
        );

        let mut unexplained = next.clone();
        unexplained.property_damage_raw = Some(13_219);
        reconcile_inspiration_state(
            Some(&next),
            &mut unexplained,
            &BTreeMap::from([(2, false)]),
            &runtime().inspiration.packet_proven_vectors,
        );
        assert_eq!(
            unexplained
                .inspiration_providers
                .get(&2)
                .unwrap()
                .property_damage_delta,
            None,
            "an unrelated property change must invalidate attribution"
        );
    }

    #[test]
    fn inspiration_removal_waits_for_and_requires_the_reverse_packet_vector() {
        let provider = InspirationProviderState {
            provider_full_bloom: false,
            primary_raw_add_delta: 540,
            secondary_raw_add_delta: 300,
            physical_attack_base_add_delta: Some(360),
            magical_attack_base_add_delta: None,
            external_damage_delta: 105,
            property_damage_delta: Some(180),
            haste_delta: Some(382),
        };
        let previous = ActorHpState {
            primary_raw_add: [Some(1_017), Some(1_017), Some(6_616), Some(39_373)],
            critical_chance_raw: Some(7_416),
            critical_chance_raw_add: Some(6_416),
            lucky_chance_raw: Some(800),
            lucky_chance_raw_add: Some(800),
            mastery_raw: Some(900),
            mastery_raw_add: Some(900),
            versatility_raw: Some(1_944),
            versatility_raw_add: Some(1_944),
            external_damage_raw: Some(680),
            property_damage_raw: Some(13_038),
            haste_percent_basis_points: Some(4_300),
            physical_attack: AttackFamilyState {
                base_add: Some(4_839),
                ..AttackFamilyState::default()
            },
            inspiration_providers: BTreeMap::from([(2, provider)]),
            ..ActorHpState::default()
        };

        let mut before_attribute_removal = previous.clone();
        assert!(!inspiration_state_removal_matches(
            &previous,
            &before_attribute_removal,
            provider
        ));
        reconcile_inspiration_state(
            Some(&previous),
            &mut before_attribute_removal,
            &BTreeMap::new(),
            &runtime().inspiration.packet_proven_vectors,
        );
        assert!(before_attribute_removal.inspiration_providers.is_empty());

        let mut reversed = ActorHpState {
            primary_raw_add: [Some(477), Some(477), Some(6_076), Some(38_833)],
            critical_chance_raw: Some(7_116),
            critical_chance_raw_add: Some(6_116),
            lucky_chance_raw: Some(500),
            lucky_chance_raw_add: Some(500),
            mastery_raw: Some(600),
            mastery_raw_add: Some(600),
            versatility_raw: Some(1_644),
            versatility_raw_add: Some(1_644),
            external_damage_raw: Some(575),
            property_damage_raw: Some(12_858),
            haste_percent_basis_points: Some(3_918),
            physical_attack: AttackFamilyState {
                base_add: Some(4_479),
                ..AttackFamilyState::default()
            },
            inspiration_providers: BTreeMap::from([(2, provider)]),
            ..ActorHpState::default()
        };
        assert!(inspiration_state_removal_matches(
            &previous, &reversed, provider
        ));
        reconcile_inspiration_state(
            Some(&previous),
            &mut reversed,
            &BTreeMap::new(),
            &runtime().inspiration.packet_proven_vectors,
        );
        assert!(reversed.inspiration_providers.is_empty());
    }

    #[test]
    fn functional_amp_positive_consumed_stack_remains_active_until_removal() {
        let mut projector = BpsrStateDamageContributionProjector::default();
        let provider = rlogs_events::EntityRef {
            actor_id: rlogs_events::ActorId(2),
            entity_uuid: rlogs_events::EntityUuid(20),
        };
        let target = rlogs_events::EntityRef {
            actor_id: rlogs_events::ActorId(4),
            entity_uuid: rlogs_events::EntityUuid(40),
        };
        let mut status = rlogs_events::StatusEvent {
            source: Some(provider),
            target,
            effect: rlogs_events::StatusEffectId(runtime().functional_amp.effect_id),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(11)),
            origin: Some(rlogs_events::StatusOrigin {
                source_type_id: 1,
                source_config_id: runtime().functional_amp.source_config_id,
            }),
            state: StatusState::Consumed,
            stacks: Some(1),
            level: Some(1),
            part_id: None,
            count: Some(-1),
            created_at_millis: None,
            duration_millis: Some(1_000),
        };

        projector.observe_functional_amp_status(&status);
        assert_eq!(
            projector.desired_functional_amp_provider_percentages(4, 40),
            BTreeMap::from([((2, 20), runtime().functional_amp.attack_percent_raw_delta,)])
        );

        status.state = StatusState::Removed;
        status.stacks = Some(0);
        projector.observe_functional_amp_status(&status);
        assert!(
            projector
                .desired_functional_amp_provider_percentages(4, 40)
                .is_empty()
        );
    }

    #[test]
    fn mechanical_power_requires_absent_origin_and_reconciles_the_captured_marksman_transition() {
        let recipient_rule = &runtime().mechanical_power.recipient_rules[0];
        let candidate_primary_percent_raw_delta = 750;
        let mut projector = BpsrStateDamageContributionProjector {
            mechanical_power_candidate_audit_enabled: true,
            mechanical_power_candidate_primary_percent_override: Some(
                candidate_primary_percent_raw_delta,
            ),
            states: HashMap::from([(
                4,
                ActorHpState {
                    mechanical_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            final_value: Some(9_062),
                            intermediate_value: Some(7_650),
                            base_add: Some(6_120),
                            extra_add: Some(1_412),
                            raw_percent: Some(2_500),
                            raw_percent_packet_observed: true,
                            provider_raw_percent: BTreeMap::new(),
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            class_id_by_actor: HashMap::from([(4, recipient_rule.recipient_class_id)]),
            active_players: HashSet::from([2, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let mut status = rlogs_events::StatusEvent {
            source: Some(rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(2),
                entity_uuid: rlogs_events::EntityUuid(20),
            }),
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            effect: rlogs_events::StatusEffectId(runtime().mechanical_power.effect_id),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(11)),
            origin: Some(rlogs_events::StatusOrigin {
                source_type_id: 1,
                source_config_id: runtime().mechanical_power.source_config_id,
            }),
            state: StatusState::Applied,
            stacks: Some(1),
            level: Some(5),
            part_id: None,
            count: None,
            created_at_millis: None,
            duration_millis: Some(runtime().mechanical_power.duration_millis),
        };

        projector.observe_status(&status, 123);
        assert!(projector.mechanical_power_windows.is_empty());
        status.origin = None;
        projector.observe_status(&status, 123);
        assert_eq!(
            projector.desired_mechanical_power_provider_percentages(4, recipient_rule),
            BTreeMap::from([(2, candidate_primary_percent_raw_delta)])
        );

        projector.observe_attributes(
            4,
            40,
            EntityAttributeUpdateKind::Delta,
            None,
            &[
                integer_test_attribute(11_030, 9_521),
                integer_test_attribute(11_031, 8_109),
                integer_test_attribute(11_032, 6_120),
                integer_test_attribute(11_033, 1_412),
                integer_test_attribute(11_034, 3_250),
            ],
            123,
        );
        let primary = projector
            .staged_states
            .get(&4)
            .unwrap()
            .mechanical_primary_by_class
            .get(&recipient_rule.recipient_class_id)
            .unwrap();
        assert_eq!(
            primary.provider_raw_percent,
            BTreeMap::from([(2, candidate_primary_percent_raw_delta)])
        );
    }

    #[test]
    fn harmony_grace_requires_the_exact_current_build_packet_origin() {
        let recipient_rule = runtime()
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == 11)
            .unwrap();
        let mut projector = BpsrStateDamageContributionProjector::default();
        let mut status = rlogs_events::StatusEvent {
            source: Some(rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(2),
                entity_uuid: rlogs_events::EntityUuid(20),
            }),
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            effect: rlogs_events::StatusEffectId(runtime().harmony_grace.effect_id),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(11)),
            origin: Some(rlogs_events::StatusOrigin {
                source_type_id: 1,
                source_config_id: 3_003_053,
            }),
            state: StatusState::Applied,
            stacks: Some(1),
            level: None,
            part_id: None,
            count: None,
            created_at_millis: None,
            duration_millis: Some(8_000),
        };

        projector.observe_status(&status, 123);
        assert_eq!(
            projector.desired_harmony_grace_provider_percentages(4, recipient_rule),
            BTreeMap::from([(2, recipient_rule.primary_percent_raw_delta)])
        );

        projector.harmony_grace_windows.clear();
        status.origin = None;
        projector.observe_status(&status, 123);
        assert!(projector.harmony_grace_windows.is_empty());

        status.origin = Some(rlogs_events::StatusOrigin {
            source_type_id: 1,
            source_config_id: 3_003_054,
        });
        projector.observe_status(&status, 123);
        assert!(projector.harmony_grace_windows.is_empty());
    }

    #[test]
    fn mechanical_power_reconciles_attributes_that_precede_status_on_the_same_wire() {
        let recipient_rule = runtime()
            .mechanical_power
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == 11)
            .unwrap();
        let candidate_primary_percent_raw_delta = 750;
        let first_wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let inactive_primary = AttackFamilyState {
            final_value: Some(10_465),
            intermediate_value: Some(7_405),
            base_add: Some(6_120),
            extra_add: Some(3_060),
            raw_percent: Some(2_100),
            raw_percent_packet_observed: true,
            provider_raw_percent: BTreeMap::new(),
        };
        let active_primary = AttackFamilyState {
            final_value: Some(10_924),
            intermediate_value: Some(7_864),
            base_add: Some(6_120),
            extra_add: Some(3_060),
            raw_percent: Some(2_850),
            raw_percent_packet_observed: true,
            provider_raw_percent: BTreeMap::new(),
        };
        let mut projector = BpsrStateDamageContributionProjector {
            mechanical_power_candidate_audit_enabled: true,
            mechanical_power_candidate_primary_percent_override: Some(
                candidate_primary_percent_raw_delta,
            ),
            current_wire: Some(first_wire),
            states: HashMap::from([(
                4,
                ActorHpState {
                    mechanical_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        inactive_primary.clone(),
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            staged_states: HashMap::from([(
                4,
                ActorHpState {
                    mechanical_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        active_primary,
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            class_id_by_actor: HashMap::from([(4, recipient_rule.recipient_class_id)]),
            active_players: HashSet::from([2, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let mut status = rlogs_events::StatusEvent {
            source: Some(rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(2),
                entity_uuid: rlogs_events::EntityUuid(20),
            }),
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            effect: rlogs_events::StatusEffectId(runtime().mechanical_power.effect_id),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(11)),
            origin: None,
            state: StatusState::Applied,
            stacks: Some(1),
            level: Some(5),
            part_id: None,
            count: None,
            created_at_millis: None,
            duration_millis: Some(runtime().mechanical_power.duration_millis),
        };

        projector.observe_mechanical_power_status(&status);
        let desired = BTreeMap::from([(2, candidate_primary_percent_raw_delta)]);
        assert_eq!(
            projector.staged_states[&4].mechanical_primary_by_class
                [&recipient_rule.recipient_class_id]
                .provider_raw_percent,
            desired
        );
        let window = EffectWindowKey {
            target_actor_id: 4,
            provider_actor_id: 2,
            instance_id: Some(11),
        };
        let witness = projector.mechanical_power_primary_transition_witnesses[&window]
            .iter()
            .next()
            .unwrap();
        assert_eq!(witness.wire, first_wire);
        assert_eq!(witness.provider_primary_marginal, 459);

        let second_wire = WireKey {
            capture_sequence: 4,
            ..first_wire
        };
        projector.advance_wire(second_wire);
        assert_eq!(
            projector.states[&4].mechanical_primary_by_class[&recipient_rule.recipient_class_id]
                .provider_raw_percent,
            desired
        );

        projector.staged_states.insert(
            4,
            ActorHpState {
                mechanical_primary_by_class: BTreeMap::from([(
                    recipient_rule.recipient_class_id,
                    inactive_primary,
                )]),
                ..ActorHpState::default()
            },
        );
        status.state = StatusState::Removed;
        status.stacks = Some(0);
        projector.observe_mechanical_power_status(&status);
        assert!(
            projector.staged_states[&4].mechanical_primary_by_class
                [&recipient_rule.recipient_class_id]
                .provider_raw_percent
                .is_empty()
        );
        assert!(
            !projector
                .mechanical_power_primary_transition_witnesses
                .contains_key(&window),
            "closing the lifecycle must remove its transition witness"
        );
    }

    #[test]
    fn harmony_grace_reconciles_attributes_that_precede_status_on_the_same_wire() {
        let old_runtime = rdps_runtime_config_for("global", "24252055")
            .expect("old exact runtime lookup should validate")
            .expect("old exact runtime should remain replayable");
        let recipient_rule = old_runtime
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == 11)
            .unwrap();
        let first_wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let mut projector = BpsrStateDamageContributionProjector {
            runtime: old_runtime,
            current_wire: Some(first_wire),
            states: HashMap::from([(
                4,
                ActorHpState {
                    harmony_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            raw_percent: Some(1_500),
                            ..AttackFamilyState::default()
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            staged_states: HashMap::from([(
                4,
                ActorHpState {
                    harmony_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            raw_percent: Some(1_700),
                            ..AttackFamilyState::default()
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            class_id_by_actor: HashMap::from([(4, recipient_rule.recipient_class_id)]),
            active_players: HashSet::from([2, 4]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let mut status = rlogs_events::StatusEvent {
            source: Some(rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(2),
                entity_uuid: rlogs_events::EntityUuid(20),
            }),
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            effect: rlogs_events::StatusEffectId(old_runtime.harmony_grace.effect_id),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(11)),
            origin: None,
            state: StatusState::Applied,
            stacks: Some(1),
            level: None,
            part_id: None,
            count: None,
            created_at_millis: None,
            duration_millis: Some(8_000),
        };

        projector.observe_harmony_grace_status(&status);
        assert_eq!(
            projector.staged_states[&4].harmony_primary_by_class
                [&recipient_rule.recipient_class_id]
                .provider_raw_percent,
            BTreeMap::from([(2, recipient_rule.primary_percent_raw_delta)])
        );

        let second_wire = WireKey {
            capture_sequence: 4,
            ..first_wire
        };
        projector.advance_wire(second_wire);
        assert_eq!(
            projector.states[&4].harmony_primary_by_class[&recipient_rule.recipient_class_id]
                .provider_raw_percent,
            BTreeMap::from([(2, recipient_rule.primary_percent_raw_delta)])
        );

        projector.staged_states.insert(
            4,
            ActorHpState {
                harmony_primary_by_class: BTreeMap::from([(
                    recipient_rule.recipient_class_id,
                    AttackFamilyState {
                        raw_percent: Some(1_500),
                        provider_raw_percent: BTreeMap::from([(
                            2,
                            recipient_rule.primary_percent_raw_delta,
                        )]),
                        ..AttackFamilyState::default()
                    },
                )]),
                ..ActorHpState::default()
            },
        );
        status.state = StatusState::Removed;
        status.stacks = Some(0);
        projector.observe_harmony_grace_status(&status);
        assert!(
            projector.staged_states[&4].harmony_primary_by_class
                [&recipient_rule.recipient_class_id]
                .provider_raw_percent
                .is_empty(),
            "the exact same-wire inverse transition must remove provider ownership"
        );
    }

    #[test]
    fn harmony_grace_consumed_closes_the_provider_window_even_when_stack_echo_is_one() {
        let old_runtime = rdps_runtime_config_for("global", "24252055")
            .expect("old exact runtime lookup should validate")
            .expect("old exact runtime should remain replayable");
        let recipient_rule = old_runtime
            .harmony_grace
            .recipient_rules
            .iter()
            .find(|rule| rule.recipient_class_id == 11)
            .unwrap();
        let wire = WireKey {
            connection_id: 1,
            stream_id: 2,
            capture_sequence: 3,
        };
        let mut projector = BpsrStateDamageContributionProjector {
            runtime: old_runtime,
            current_wire: Some(wire),
            class_id_by_actor: HashMap::from([(4, recipient_rule.recipient_class_id)]),
            active_players: HashSet::from([2, 4]),
            harmony_grace_windows: HashSet::from([EffectWindowKey {
                target_actor_id: 4,
                provider_actor_id: 2,
                instance_id: Some(11),
            }]),
            states: HashMap::from([(
                4,
                ActorHpState {
                    harmony_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            raw_percent: Some(1_700),
                            provider_raw_percent: BTreeMap::from([(
                                2,
                                recipient_rule.primary_percent_raw_delta,
                            )]),
                            ..AttackFamilyState::default()
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            staged_states: HashMap::from([(
                4,
                ActorHpState {
                    harmony_primary_by_class: BTreeMap::from([(
                        recipient_rule.recipient_class_id,
                        AttackFamilyState {
                            raw_percent: Some(1_500),
                            provider_raw_percent: BTreeMap::from([(
                                2,
                                recipient_rule.primary_percent_raw_delta,
                            )]),
                            ..AttackFamilyState::default()
                        },
                    )]),
                    ..ActorHpState::default()
                },
            )]),
            ..BpsrStateDamageContributionProjector::default()
        };
        let status = rlogs_events::StatusEvent {
            source: Some(rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(2),
                entity_uuid: rlogs_events::EntityUuid(20),
            }),
            target: rlogs_events::EntityRef {
                actor_id: rlogs_events::ActorId(4),
                entity_uuid: rlogs_events::EntityUuid(40),
            },
            effect: rlogs_events::StatusEffectId(old_runtime.harmony_grace.effect_id),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(11)),
            origin: None,
            state: StatusState::Consumed,
            stacks: Some(1),
            level: None,
            part_id: None,
            count: None,
            created_at_millis: None,
            duration_millis: Some(8_000),
        };

        projector.observe_harmony_grace_status(&status);

        assert!(projector.harmony_grace_windows.is_empty());
        assert!(
            projector.staged_states[&4].harmony_primary_by_class
                [&recipient_rule.recipient_class_id]
                .provider_raw_percent
                .is_empty(),
            "Consumed is the observed closure packet; stacks=1 is an echoed pre-consumption count"
        );
    }

    #[test]
    fn harmony_grace_bootstraps_only_from_an_exact_reversible_prior_family() {
        let previous = AttackFamilyState {
            final_value: Some(7_038),
            intermediate_value: Some(7_038),
            base_add: Some(6_120),
            extra_add: None,
            raw_percent: None,
            raw_percent_packet_observed: false,
            provider_raw_percent: BTreeMap::new(),
        };
        let desired = BTreeMap::from([(2, 200)]);
        let mut current = AttackFamilyState {
            final_value: Some(7_856),
            intermediate_value: Some(7_160),
            base_add: Some(6_120),
            extra_add: Some(696),
            raw_percent: Some(1_700),
            raw_percent_packet_observed: true,
            provider_raw_percent: BTreeMap::new(),
        };

        reconcile_external_percent_family_from_exact_prior(&previous, &mut current, &desired);
        assert_eq!(current.provider_raw_percent, desired);

        current.raw_percent = Some(1_800);
        current.provider_raw_percent.clear();
        reconcile_external_percent_family_from_exact_prior(&previous, &mut current, &desired);
        assert!(current.provider_raw_percent.is_empty());

        let mut omitted_zero = AttackFamilyState {
            final_value: Some(5_906),
            intermediate_value: Some(5_906),
            ..AttackFamilyState::default()
        };
        complete_exact_extra_add(&mut omitted_zero);
        assert_eq!(omitted_zero.extra_add, Some(0));
    }

    #[test]
    fn team_luck_fraction_retains_every_proven_single_outcome_hit() {
        assert_eq!(
            team_luck_fraction(46_908, true, false, 10_128, 4_540),
            Some((762_255, 629))
        );
        assert_eq!(
            team_luck_fraction(273_931, false, true, 10_128, 4_540),
            Some((4_656_827, 227))
        );
        assert_eq!(team_luck_fraction(100, false, false, 10_128, 4_540), None);
        assert_eq!(
            team_luck_fraction(1, true, false, 10_128, 4_540),
            Some((65, 2_516)),
            "critical attribution conserves the packet-observed final through the proven proportional factor share without claiming hidden server rounding"
        );
        assert_eq!(
            team_luck_fraction(46_908, true, true, 10_128, 4_540),
            None,
            "combined critical and Lucky floor ordering is not yet proven"
        );
    }

    #[test]
    fn later_multiplier_owns_only_the_body_remaining_after_attack_stage() {
        let earlier = ExactRationalDamageContributionEvent {
            observed_micros: 1,
            effect_id: runtime().functional_amp.effect_id,
            provider_actor_id: 2,
            recipient_actor_id: 4,
            numerator: 20,
            denominator: 1,
            observed_damage: 100,
            included: true,
        };
        let later = ExactRationalDamageContributionEvent {
            observed_micros: 1,
            effect_id: runtime().team_luck.effect_id,
            provider_actor_id: 3,
            recipient_actor_id: 4,
            numerator: 50,
            denominator: 1,
            observed_damage: 100,
            included: true,
        };

        let adjusted =
            scale_later_rational_marginal_after_many(std::slice::from_ref(&earlier), later)
                .unwrap();
        assert_eq!((adjusted.numerator, adjusted.denominator), (40, 1));
        assert_eq!(earlier.numerator + adjusted.numerator, 60);

        let second_earlier = ExactRationalDamageContributionEvent {
            observed_micros: 1,
            effect_id: runtime().harmony_grace.effect_id,
            provider_actor_id: 5,
            recipient_actor_id: 4,
            numerator: 10,
            denominator: 1,
            observed_damage: 100,
            included: true,
        };
        let adjusted_after_both =
            scale_later_rational_marginal_after_many(&[earlier, second_earlier], later).unwrap();
        assert_eq!(
            (
                adjusted_after_both.numerator,
                adjusted_after_both.denominator
            ),
            (35, 1)
        );
        assert_eq!(
            earlier.numerator + second_earlier.numerator + adjusted_after_both.numerator,
            65
        );
    }
}
