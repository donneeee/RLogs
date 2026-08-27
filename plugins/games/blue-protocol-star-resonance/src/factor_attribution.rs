use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::{combat_action_presentation, status_effect_presentation};

const FACTOR_ATTRIBUTION_SCHEMA_VERSION: u16 = 2;
const FACTOR_ATTRIBUTION_GAME_BUILD: &str = "24687926";
const FACTOR_ATTRIBUTION_CANDIDATE_SOURCE_BUILD: &str = "24252055";
const FACTOR_ATTRIBUTION_SEASON_ID: u32 = 3;
const MAXIMUM_FACTOR_RULES: usize = 4_096;
const MAXIMUM_RECOUNT_PARENTS: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeFactorCategory {
    Polarity,
    Stasis,
    Inspiration,
    Reality,
    Rhapsody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeFactorStat {
    ElementalPower,
    CriticalGain,
    MasteryGain,
    MaxHealth,
    MagicDamageResistance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeFactorValueUnit {
    Flat,
    BasisPoints,
    Milliseconds,
    HasteBasisPointsPerHit,
}

/// The exact runtime surface changed by a factor stat modifier.
///
/// These values are inputs to later formulas, not automatic rDPS credit. In
/// particular, maximum-health changes must remain available because packet
/// actions may scale from current, maximum, or missing health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeFormulaInputRole {
    OutgoingElementalPower,
    CriticalGain,
    MasteryGain,
    MaximumHealthSurface,
    IncomingMagicResistance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeStatCreditPolicy {
    /// Preserve an owner-selected factor as a formula input without moving its
    /// resulting self damage to another character's rDPS.
    OwnerOnlyNoRdps,
    /// Reserved for a separately packet-proven external provider. Only the
    /// marginal replay delta may be credited.
    ExternalProviderMarginalReplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeEnergyDirection {
    Generate,
    ConsumeAtThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeEnergyResource {
    IllusionEnergy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeDamageDomain {
    IllusionBreaking,
    Illusion,
    IncomingAny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeActionRelationKind {
    DamageAmplification,
    HitCountPerHaste,
    TriggerAction,
    CooldownReduction,
    LethalDamageReduction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeActionCondition {
    Always,
    AfterTrigger,
    AtEnergyThreshold,
    OnDamageAtEnergyThreshold,
    WhileActionActive,
    OnLethalIncomingDamage,
    TriggeredInstanceOnly,
    WhileOnCooldownAtEnergyThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsychoscopeFactorEvidence {
    ExactCurrentBuildDirectAttribute,
    ExactCurrentBuildDescription,
    CurrentBuildLocalizedDescriptionRecountBridge,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PsychoscopeRecountParent {
    pub recount_group_id: i64,
    pub name: String,
    pub activation_ability_ids: Vec<i64>,
    pub observed_child_action_ids: Vec<i64>,
    pub damage_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PsychoscopeFactorStatModifier {
    pub stat: PsychoscopeFactorStat,
    pub attribute_id: Option<i64>,
    pub value: i64,
    pub unit: PsychoscopeFactorValueUnit,
    pub trigger_recount_group_id: Option<i64>,
    pub formula_input_role: PsychoscopeFormulaInputRole,
    pub credit_policy: PsychoscopeStatCreditPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PsychoscopeEnergyRelation {
    pub resource: PsychoscopeEnergyResource,
    pub direction: PsychoscopeEnergyDirection,
    pub amount: u32,
    pub threshold: Option<u32>,
    pub trigger_recount_group_id: Option<i64>,
    pub requires_damage_event: bool,
    pub once_per_action: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PsychoscopeActionRelation {
    pub kind: PsychoscopeActionRelationKind,
    pub condition: PsychoscopeActionCondition,
    pub damage_domain: Option<PsychoscopeDamageDomain>,
    pub trigger_recount_group_id: Option<i64>,
    pub target_recount_group_id: Option<i64>,
    pub value: Option<i64>,
    pub unit: Option<PsychoscopeFactorValueUnit>,
    pub duration_ms: Option<u32>,
    pub internal_cooldown_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PsychoscopeFactorRule {
    pub item_id: i64,
    pub family_id: i64,
    pub grade: u32,
    pub category: PsychoscopeFactorCategory,
    pub class_id: Option<u32>,
    pub primary_buff_id: Option<i64>,
    pub evidence: PsychoscopeFactorEvidence,
    pub stat_modifiers: Vec<PsychoscopeFactorStatModifier>,
    pub energy_relations: Vec<PsychoscopeEnergyRelation>,
    pub action_relations: Vec<PsychoscopeActionRelation>,
    pub attribution_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PsychoscopeFactorAttributionCatalog {
    schema_version: u16,
    game_build: String,
    source_game_build: String,
    runtime_rules_enabled: bool,
    current_build_identity_state: String,
    current_build_inventory: PsychoscopeCurrentBuildInventory,
    season_id: u32,
    default_policy: String,
    recount_parents: Vec<PsychoscopeRecountParent>,
    factors: Vec<PsychoscopeFactorRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PsychoscopeCurrentBuildInventory {
    path: String,
    bytes: u64,
    sha256: String,
    source_count: u64,
    row_count: u64,
    active_factor_rules: usize,
}

static FACTOR_ATTRIBUTION: OnceLock<Result<PsychoscopeFactorAttributionCatalog, String>> =
    OnceLock::new();

fn factor_attribution_catalog() -> Result<&'static PsychoscopeFactorAttributionCatalog, String> {
    FACTOR_ATTRIBUTION
        .get_or_init(|| {
            let catalog: PsychoscopeFactorAttributionCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/psychoscope-factor-attribution.v2.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR Psychoscope factor attribution is invalid: {error}")
            })?;
            validate_factor_attribution(&catalog)?;
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn validate_factor_attribution(
    catalog: &PsychoscopeFactorAttributionCatalog,
) -> Result<(), String> {
    if catalog.schema_version != FACTOR_ATTRIBUTION_SCHEMA_VERSION
        || catalog.game_build != FACTOR_ATTRIBUTION_GAME_BUILD
        || catalog.source_game_build != FACTOR_ATTRIBUTION_CANDIDATE_SOURCE_BUILD
        || catalog.runtime_rules_enabled
        || catalog.current_build_identity_state
            != "static-inventory-present-historical-candidates-only"
        || catalog.current_build_inventory.path
            != "../catalog/psychoscope-factors/current-build-static-inventory.candidate.v1.json"
        || catalog.current_build_inventory.bytes == 0
        || catalog.current_build_inventory.sha256.len() != 64
        || catalog.current_build_inventory.source_count == 0
        || catalog.current_build_inventory.row_count == 0
        || catalog.current_build_inventory.active_factor_rules != 0
        || catalog.season_id != FACTOR_ATTRIBUTION_SEASON_ID
        || catalog.default_policy != "retain_raw_actions_disable_rdps_until_formula_review"
        || catalog.recount_parents.is_empty()
        || catalog.recount_parents.len() > MAXIMUM_RECOUNT_PARENTS
        || catalog.factors.is_empty()
        || catalog.factors.len() > MAXIMUM_FACTOR_RULES
        || catalog
            .recount_parents
            .windows(2)
            .any(|pair| pair[0].recount_group_id >= pair[1].recount_group_id)
        || catalog
            .factors
            .windows(2)
            .any(|pair| pair[0].item_id >= pair[1].item_id)
    {
        return Err("bundled BPSR Psychoscope factor attribution has an unsupported shape".into());
    }

    for parent in &catalog.recount_parents {
        if parent.recount_group_id <= 0
            || parent.name.trim().is_empty()
            || parent
                .activation_ability_ids
                .iter()
                .chain(&parent.observed_child_action_ids)
                .chain(&parent.damage_ids)
                .any(|id| *id <= 0)
        {
            return Err(format!(
                "Psychoscope recount parent {} is invalid",
                parent.recount_group_id
            ));
        }
        for child_id in &parent.observed_child_action_ids {
            let Some(action) = combat_action_presentation(*child_id)? else {
                return Err(format!(
                    "Psychoscope recount parent {} references missing child action {child_id}",
                    parent.recount_group_id
                ));
            };
            if action.recount_group_id != Some(parent.recount_group_id) {
                return Err(format!(
                    "Psychoscope child action {child_id} does not resolve to recount parent {}",
                    parent.recount_group_id
                ));
            }
        }
    }

    for factor in &catalog.factors {
        if factor.item_id <= 0
            || factor.family_id <= 0
            || factor.grade == 0
            || factor.grade > 10
            || factor.class_id.is_some_and(|class_id| class_id == 0)
            || factor.attribution_enabled
            || (factor.stat_modifiers.is_empty()
                && factor.energy_relations.is_empty()
                && factor.action_relations.is_empty())
        {
            return Err(format!(
                "Psychoscope factor item {} is invalid",
                factor.item_id
            ));
        }
        if let Some(effect_id) = factor.primary_buff_id
            && status_effect_presentation(effect_id)?.is_none()
        {
            return Err(format!(
                "Psychoscope factor item {} references missing primary buff {effect_id}",
                factor.item_id
            ));
        }
        for modifier in &factor.stat_modifiers {
            let role_matches_stat = matches!(
                (modifier.stat, modifier.formula_input_role),
                (
                    PsychoscopeFactorStat::ElementalPower,
                    PsychoscopeFormulaInputRole::OutgoingElementalPower
                ) | (
                    PsychoscopeFactorStat::CriticalGain,
                    PsychoscopeFormulaInputRole::CriticalGain
                ) | (
                    PsychoscopeFactorStat::MasteryGain,
                    PsychoscopeFormulaInputRole::MasteryGain
                ) | (
                    PsychoscopeFactorStat::MaxHealth,
                    PsychoscopeFormulaInputRole::MaximumHealthSurface
                ) | (
                    PsychoscopeFactorStat::MagicDamageResistance,
                    PsychoscopeFormulaInputRole::IncomingMagicResistance
                )
            );
            if modifier.value == 0
                || modifier
                    .attribute_id
                    .is_some_and(|attribute_id| attribute_id <= 0)
                || !valid_recount_group(catalog, modifier.trigger_recount_group_id)
                || !role_matches_stat
                || modifier.credit_policy != PsychoscopeStatCreditPolicy::OwnerOnlyNoRdps
            {
                return Err(format!(
                    "Psychoscope factor item {} has an invalid stat modifier",
                    factor.item_id
                ));
            }
        }
        for relation in &factor.energy_relations {
            let threshold_is_valid = match relation.direction {
                PsychoscopeEnergyDirection::Generate => relation.threshold.is_none(),
                PsychoscopeEnergyDirection::ConsumeAtThreshold => {
                    relation.threshold == Some(relation.amount)
                }
            };
            if relation.amount == 0
                || !threshold_is_valid
                || !valid_recount_group(catalog, relation.trigger_recount_group_id)
            {
                return Err(format!(
                    "Psychoscope factor item {} has an invalid energy relation",
                    factor.item_id
                ));
            }
        }
        for relation in &factor.action_relations {
            if !valid_recount_group(catalog, relation.trigger_recount_group_id)
                || !valid_recount_group(catalog, relation.target_recount_group_id)
                || relation.value.is_some_and(|value| value <= 0)
                || relation.value.is_some() != relation.unit.is_some()
                || matches!(
                    relation.kind,
                    PsychoscopeActionRelationKind::DamageAmplification
                        | PsychoscopeActionRelationKind::HitCountPerHaste
                        | PsychoscopeActionRelationKind::CooldownReduction
                ) && (relation.target_recount_group_id.is_none() || relation.value.is_none())
                || relation.kind == PsychoscopeActionRelationKind::DamageAmplification
                    && (relation.damage_domain.is_none()
                        || relation.unit != Some(PsychoscopeFactorValueUnit::BasisPoints))
                || relation.kind == PsychoscopeActionRelationKind::HitCountPerHaste
                    && relation.unit != Some(PsychoscopeFactorValueUnit::HasteBasisPointsPerHit)
                || relation.kind == PsychoscopeActionRelationKind::CooldownReduction
                    && relation.unit != Some(PsychoscopeFactorValueUnit::Milliseconds)
                || relation.kind == PsychoscopeActionRelationKind::TriggerAction
                    && relation.target_recount_group_id.is_none()
                || relation.kind == PsychoscopeActionRelationKind::LethalDamageReduction
                    && (relation.value.is_none()
                        || relation.unit != Some(PsychoscopeFactorValueUnit::BasisPoints)
                        || relation.damage_domain != Some(PsychoscopeDamageDomain::IncomingAny)
                        || relation.internal_cooldown_ms.is_none())
            {
                return Err(format!(
                    "Psychoscope factor item {} has an invalid action relation",
                    factor.item_id
                ));
            }
        }
    }
    Ok(())
}

fn valid_recount_group(
    catalog: &PsychoscopeFactorAttributionCatalog,
    recount_group_id: Option<i64>,
) -> bool {
    recount_group_id.is_none_or(|id| {
        catalog
            .recount_parents
            .binary_search_by_key(&id, |parent| parent.recount_group_id)
            .is_ok()
    })
}

pub fn psychoscope_factor_rules() -> Result<&'static [PsychoscopeFactorRule], String> {
    Ok(&factor_attribution_catalog()?.factors)
}

pub fn psychoscope_factor_runtime_rules_enabled() -> Result<bool, String> {
    Ok(factor_attribution_catalog()?.runtime_rules_enabled)
}

pub fn psychoscope_factor_by_item_id(
    item_id: i64,
) -> Result<Option<&'static PsychoscopeFactorRule>, String> {
    let catalog = factor_attribution_catalog()?;
    Ok(catalog
        .factors
        .binary_search_by_key(&item_id, |factor| factor.item_id)
        .ok()
        .map(|index| &catalog.factors[index]))
}

pub fn psychoscope_recount_parent(
    recount_group_id: i64,
) -> Result<Option<&'static PsychoscopeRecountParent>, String> {
    let catalog = factor_attribution_catalog()?;
    Ok(catalog
        .recount_parents
        .binary_search_by_key(&recount_group_id, |parent| parent.recount_group_id)
        .ok()
        .map(|index| &catalog.recount_parents[index]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ReviewedSource {
        schema_version: u16,
        game_build: String,
        observation_id: String,
        scope: String,
        evidence: Vec<String>,
        selected_item_ids: Vec<i64>,
        reviewed_recount_group_ids: Vec<i64>,
        policy: String,
    }

    #[test]
    fn reviewed_source_is_retained_as_a_disabled_historical_candidate_bundle() {
        let source: ReviewedSource = serde_json::from_str(include_str!(
            "../game-data/catalog/psychoscope-factors/season-3/selected-marksman-factor-attribution.v1.json"
        ))
        .expect("reviewed Psychoscope factor source should parse");
        assert_eq!(source.schema_version, 1);
        assert_eq!(source.game_build, FACTOR_ATTRIBUTION_CANDIDATE_SOURCE_BUILD);
        assert_eq!(source.observation_id, "psychoscope-factor-capture-001");
        assert!(!source.scope.trim().is_empty());
        assert!(!source.evidence.is_empty());
        let runtime = factor_attribution_catalog().unwrap();
        assert_eq!(runtime.game_build, FACTOR_ATTRIBUTION_GAME_BUILD);
        assert!(!runtime.runtime_rules_enabled);
        assert_eq!(
            source.selected_item_ids,
            runtime
                .factors
                .iter()
                .map(|factor| factor.item_id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            source.reviewed_recount_group_ids,
            runtime
                .recount_parents
                .iter()
                .map(|parent| parent.recount_group_id)
                .collect::<Vec<_>>()
        );
        assert_eq!(source.policy, runtime.default_policy);
    }

    #[test]
    fn selected_capture_factor_items_have_explicit_rules() {
        let selected = [
            20020887, 20020927, 20021035, 20021025, 20020397, 20020420, 20020427, 20021155,
            20021947, 20021967, 20021394, 20020425,
        ];
        for item_id in selected {
            let factor = psychoscope_factor_by_item_id(item_id)
                .unwrap()
                .unwrap_or_else(|| panic!("selected factor item {item_id} should resolve"));
            assert!(!factor.attribution_enabled);
        }
    }

    #[test]
    fn no_buff_factors_preserve_direct_attribute_delivery() {
        let polarity = psychoscope_factor_by_item_id(20020887).unwrap().unwrap();
        assert_eq!(polarity.primary_buff_id, None);
        assert_eq!(polarity.stat_modifiers[0].attribute_id, Some(13002));
        assert_eq!(polarity.stat_modifiers[0].value, 268);

        let stasis = psychoscope_factor_by_item_id(20021025).unwrap().unwrap();
        assert_eq!(stasis.primary_buff_id, None);
        assert_eq!(
            stasis
                .stat_modifiers
                .iter()
                .map(|modifier| (modifier.attribute_id, modifier.value))
                .collect::<Vec<_>>(),
            vec![(Some(11322), 4200), (Some(11324), 94)]
        );
        assert!(stasis.stat_modifiers.iter().all(|modifier| {
            modifier.formula_input_role == PsychoscopeFormulaInputRole::MaximumHealthSurface
                && modifier.credit_policy == PsychoscopeStatCreditPolicy::OwnerOnlyNoRdps
        }));
    }

    #[test]
    fn same_family_grade_variants_keep_their_exact_magnitudes() {
        let grade_five = psychoscope_factor_by_item_id(20020425).unwrap().unwrap();
        let grade_seven = psychoscope_factor_by_item_id(20020427).unwrap().unwrap();
        assert_eq!(grade_five.family_id, 202143);
        assert_eq!(grade_seven.family_id, 202143);
        assert_eq!(grade_five.grade, 5);
        assert_eq!(grade_seven.grade, 7);
        assert_eq!(grade_five.action_relations[0].value, Some(744));
        assert_eq!(grade_seven.action_relations[0].value, Some(1006));
    }

    #[test]
    fn selected_marksman_edges_preserve_energy_and_damage_domains() {
        let x7 = psychoscope_factor_by_item_id(20020397).unwrap().unwrap();
        assert_eq!(
            x7.energy_relations[0].resource,
            PsychoscopeEnergyResource::IllusionEnergy
        );
        assert_eq!(
            x7.action_relations[0].damage_domain,
            Some(PsychoscopeDamageDomain::IllusionBreaking)
        );

        let reality_x6 = psychoscope_factor_by_item_id(20021967).unwrap().unwrap();
        assert_eq!(reality_x6.energy_relations[0].threshold, Some(360));
        assert_eq!(
            reality_x6.action_relations[1].damage_domain,
            Some(PsychoscopeDamageDomain::Illusion)
        );
    }

    #[test]
    fn factor_action_edges_use_exact_recount_parents_without_hiding_children() {
        let catalog = factor_attribution_catalog().unwrap();
        assert_eq!(
            catalog
                .recount_parents
                .iter()
                .map(|parent| parent.recount_group_id)
                .collect::<Vec<_>>(),
            vec![84, 85, 87, 88, 94, 95, 96, 97, 106]
        );
        assert_eq!(
            catalog.default_policy,
            "retain_raw_actions_disable_rdps_until_formula_review"
        );
        for parent in &catalog.recount_parents {
            for child_id in &parent.observed_child_action_ids {
                assert_eq!(
                    combat_action_presentation(*child_id)
                        .unwrap()
                        .and_then(|action| action.recount_group_id),
                    Some(parent.recount_group_id)
                );
            }
        }
    }
}
