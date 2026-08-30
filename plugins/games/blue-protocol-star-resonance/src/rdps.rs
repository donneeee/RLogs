use std::sync::OnceLock;

use rlogs_combat::{DamageContributionKind, DamageContributionRule, DamageContributionStacking};
use serde::Deserialize;

use crate::status_effect_presentation;

const RDPS_CLASSIFICATION_SCHEMA_VERSION: u16 = 1;
const RDPS_CLASSIFICATION_DEPLOYMENT_ID: &str = "global";
const RDPS_CLASSIFICATION_GAME_BUILD: &str = "24687926";
const MAXIMUM_REVIEWED_RDPS_EFFECTS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsReviewState {
    Candidate,
    Confirmed,
    NonContributing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsContributionKind {
    DirectDamageAmplification,
    Environmental,
    Haste,
    HealingSupport,
    InternalMarker,
    OffensiveStatBoost,
    StateScaling,
    TargetVulnerability,
    Mitigation,
    ResourceSupport,
    SelfOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsSourceScope {
    EffectSource,
    Owner,
    Environment,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsTargetScope {
    EffectTarget,
    SelfOnly,
    PartyMembers,
    Enemy,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsStackingRule {
    Fixed,
    StackScaled,
    RefreshOnly,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RdpsEffectRule {
    pub effect_id: i64,
    pub review_state: RdpsReviewState,
    pub contribution_kind: RdpsContributionKind,
    pub source_scope: RdpsSourceScope,
    pub target_scope: RdpsTargetScope,
    pub magnitude_basis_points: Option<u32>,
    pub stacking_rule: RdpsStackingRule,
    pub attribution_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdpsEffectLookup {
    Reviewed(&'static RdpsEffectRule),
    RetainedMappedUnclassified { effect_id: i64 },
    RetainedUnknownUnclassified { effect_id: i64 },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdpsEffectCatalog {
    schema_version: u16,
    game_build: String,
    default_policy: String,
    effects: Vec<RdpsEffectRule>,
}

static RDPS_EFFECTS: OnceLock<Result<RdpsEffectCatalog, String>> = OnceLock::new();

fn rdps_effect_catalog() -> Result<&'static RdpsEffectCatalog, String> {
    RDPS_EFFECTS
        .get_or_init(|| {
            let catalog: RdpsEffectCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/rdps-effect-classification.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR rDPS effect catalog is invalid: {error}"))?;
            validate_catalog(&catalog)?;
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn validate_catalog(catalog: &RdpsEffectCatalog) -> Result<(), String> {
    if catalog.schema_version != RDPS_CLASSIFICATION_SCHEMA_VERSION
        || catalog.game_build != RDPS_CLASSIFICATION_GAME_BUILD
        || catalog.default_policy != "retain_unclassified_event"
        || catalog.effects.is_empty()
        || catalog.effects.len() > MAXIMUM_REVIEWED_RDPS_EFFECTS
        || catalog
            .effects
            .windows(2)
            .any(|pair| pair[0].effect_id >= pair[1].effect_id)
    {
        return Err("bundled BPSR rDPS effect catalog has an unsupported shape".into());
    }

    for rule in &catalog.effects {
        if rule.effect_id <= 0 || status_effect_presentation(rule.effect_id)?.is_none() {
            return Err(format!(
                "rDPS effect {} is not present in the current-build status-effect catalog",
                rule.effect_id
            ));
        }
        if rule.review_state != RdpsReviewState::Confirmed && rule.attribution_enabled {
            return Err(format!(
                "rDPS effect {} enables attribution without confirmed review",
                rule.effect_id
            ));
        }
        if rule.attribution_enabled
            && (rule.magnitude_basis_points.is_none()
                || rule.source_scope == RdpsSourceScope::Unresolved
                || rule.target_scope == RdpsTargetScope::Unresolved
                || rule.stacking_rule == RdpsStackingRule::Unresolved)
        {
            return Err(format!(
                "rDPS effect {} enables attribution with an unresolved rule",
                rule.effect_id
            ));
        }
    }
    Ok(())
}

/// Classifies an effect without suppressing canonical timeline evidence.
///
/// An effect that has not completed review is returned as an explicit retained
/// result. Callers must continue recording and displaying the original status
/// event regardless of which lookup variant is returned.
pub fn classify_rdps_effect(effect_id: i64) -> Result<RdpsEffectLookup, String> {
    let catalog = rdps_effect_catalog()?;
    if let Ok(index) = catalog
        .effects
        .binary_search_by_key(&effect_id, |rule| rule.effect_id)
    {
        return Ok(RdpsEffectLookup::Reviewed(&catalog.effects[index]));
    }
    if status_effect_presentation(effect_id)?.is_some() {
        Ok(RdpsEffectLookup::RetainedMappedUnclassified { effect_id })
    } else {
        Ok(RdpsEffectLookup::RetainedUnknownUnclassified { effect_id })
    }
}

/// Returns only current-build rules that have enough packet and formula proof
/// to participate in deterministic damage attribution.
///
/// Candidate and partially modeled effects remain in the catalog and timeline,
/// but never enter the hot-path reducer until their provider, recipient,
/// magnitude, and stacking behavior are all confirmed.
pub fn confirmed_damage_contribution_rules() -> Result<Vec<DamageContributionRule>, String> {
    rdps_effect_catalog()?
        .effects
        .iter()
        .filter(|rule| rule.attribution_enabled)
        .map(|rule| {
            let kind = match rule.contribution_kind {
                RdpsContributionKind::DirectDamageAmplification => {
                    DamageContributionKind::DirectDamageAmplification
                }
                RdpsContributionKind::TargetVulnerability => {
                    DamageContributionKind::TargetVulnerability
                }
                unsupported => {
                    return Err(format!(
                        "rDPS effect {} enables unsupported contribution kind {unsupported:?}",
                        rule.effect_id
                    ));
                }
            };
            let stacking = match rule.stacking_rule {
                RdpsStackingRule::Fixed | RdpsStackingRule::RefreshOnly => {
                    DamageContributionStacking::Fixed
                }
                RdpsStackingRule::StackScaled => {
                    return Err(format!(
                        "rDPS effect {} is stack-scaled without a reviewed maximum",
                        rule.effect_id
                    ));
                }
                RdpsStackingRule::Unresolved => {
                    return Err(format!(
                        "rDPS effect {} enables attribution with unresolved stacking",
                        rule.effect_id
                    ));
                }
            };
            Ok(DamageContributionRule {
                effect_id: rule.effect_id,
                kind,
                magnitude_basis_points: rule
                    .magnitude_basis_points
                    .expect("enabled rules are validated to have a magnitude"),
                stacking,
            })
        })
        .collect()
}

/// Exact client build for which the bundled contribution rules have packet and
/// formula authority. Callers must fail closed for every other build.
pub fn confirmed_damage_contribution_game_build() -> &'static str {
    RDPS_CLASSIFICATION_GAME_BUILD
}

/// Exact deployment paired with the bundled contribution-rule build.
pub fn confirmed_damage_contribution_deployment_id() -> &'static str {
    RDPS_CLASSIFICATION_DEPLOYMENT_ID
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ReviewSource {
        schema_version: u16,
        game_build: String,
        rule: RdpsEffectRule,
        evidence: Vec<String>,
    }

    const REVIEW_SOURCES: [&str; 14] = [
        include_str!(
            "../game-data/catalog/rdps-effects/target-vulnerability/candidate/55228-luminary-bolt-vulnerability.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/702003-exaltation-anthem-5-percent.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/702004-exaltation-anthem-10-percent.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/702005-exaltation-anthem-20-percent.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/2202113-overhealing-cooldown.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/3056391-haste-chant.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/3057200-heroic-rhapsody-all-skill-damage.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/2110143-functional-amp.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/2202041-inspiration.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/target-vulnerability/confirmed/2203031-wounding-curse.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/target-vulnerability/confirmed/2205031-wounding-curse.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/target-vulnerability/confirmed/2110099-arcane-poison-explosion.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/self-only/confirmed/2300621-dmg-stack.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/2404261-external-maxhp-current-build.json"
        ),
    ];

    const REVIEW_BATCH_SOURCES: [&str; 33] = [
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/current-build-production-effect-classifications.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/2100154-blessing-party-damage.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/31602-inspire-party-haste.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/997510-coordinated-strike-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/997512-element-sharing-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/997514-attribute-transfer-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/997517-enhanced-synergy-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/997536-synergy-crit-field-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/997533-synergy-luck-field-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/997557-tactical-blessing-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/998542-all-class-aura.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/2204471-critical-cold-child.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/offensive-stat-boost/confirmed/promoted-stat-resonance-team-luck-children.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/997519-energy-synergy-domain-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/2110060-swift-vortex-party-haste.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/2202720-inspire-and-strengthen-composite.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/2100104-battlelust-applied-aura-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/current-build-support-and-marker-batch.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/2100212-battlelust-aura-owner-markers.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/997550-pulse-owner-and-support-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/997580-mastery-owner-and-support-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/997610-bloodwrath-owner-and-support-family.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/current-build-owner-self-and-support-batch.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/current-build-owner-local-offense-batch.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/current-build-healing-and-mitigation-role-batch.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/current-build-owner-talent-tail-batch.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/2110121-phantom-rally-owner-companion-attack.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/2110307-caprahorn-shield-mitigation.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/998341-sanctuary-aura-shield.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/party-offense-parent-markers.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/current-build-blocked-frontier-and-parent-dispositions.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/current-build-mixed-owner-and-support-dispositions.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/target-vulnerability/candidate/aoyi-target-mitigation-candidates.v1.json"
        ),
    ];

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ReviewBatchSource {
        schema_version: u16,
        game_build: String,
        rules: Vec<RdpsEffectRule>,
        evidence: Vec<String>,
    }

    #[test]
    fn reviewed_sources_match_the_compact_runtime_table() {
        let catalog = rdps_effect_catalog().unwrap();
        let mut source_rules = REVIEW_SOURCES
            .iter()
            .map(|json| serde_json::from_str::<ReviewSource>(json).unwrap())
            .filter_map(|source| {
                assert_eq!(source.schema_version, RDPS_CLASSIFICATION_SCHEMA_VERSION);
                assert!(!source.evidence.is_empty());
                (source.game_build == RDPS_CLASSIFICATION_GAME_BUILD).then_some(source.rule)
            })
            .collect::<Vec<_>>();
        for json in REVIEW_BATCH_SOURCES {
            let source = serde_json::from_str::<ReviewBatchSource>(json).unwrap();
            assert_eq!(source.schema_version, RDPS_CLASSIFICATION_SCHEMA_VERSION);
            assert!(!source.evidence.is_empty());
            if source.game_build == RDPS_CLASSIFICATION_GAME_BUILD {
                source_rules.extend(source.rules);
            }
        }
        source_rules.sort_by_key(|rule| rule.effect_id);
        assert_eq!(source_rules, catalog.effects);
    }

    #[test]
    fn current_build_descriptions_disposition_environment_and_owner_only_effects() {
        let RdpsEffectLookup::Reviewed(anthem) = classify_rdps_effect(702_004).unwrap() else {
            panic!("expected reviewed Exaltation Anthem 702004");
        };
        assert_eq!(anthem.review_state, RdpsReviewState::NonContributing);
        assert_eq!(anthem.source_scope, RdpsSourceScope::Environment);
        assert_eq!(anthem.magnitude_basis_points, Some(1_000));
        assert!(!anthem.attribution_enabled);

        let RdpsEffectLookup::Reviewed(rhapsody) = classify_rdps_effect(3_057_200).unwrap() else {
            panic!("expected reviewed Heroic Rhapsody 3057200");
        };
        assert_eq!(rhapsody.review_state, RdpsReviewState::NonContributing);
        assert_eq!(rhapsody.contribution_kind, RdpsContributionKind::SelfOnly);
        assert_eq!(rhapsody.source_scope, RdpsSourceScope::Owner);
        assert_eq!(rhapsody.target_scope, RdpsTargetScope::SelfOnly);
        assert!(!rhapsody.attribution_enabled);
    }

    #[test]
    fn haste_chant_is_reviewed_as_owner_only_without_enabling_attribution() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(3_056_391).unwrap() else {
            panic!("expected reviewed Haste Chant 3056391");
        };
        assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
        assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
        assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::Unresolved);
        assert!(!rule.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn join_forces_is_owner_local_proximity_scaling_without_transfer_credit() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(997_516).unwrap() else {
            panic!("expected reviewed Join Forces 997516");
        };
        assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
        assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
        assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
        assert_eq!(rule.magnitude_basis_points, Some(800));
        assert_eq!(rule.stacking_rule, RdpsStackingRule::StackScaled);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn coordinated_conversion_is_owner_local_element_damage_without_transfer_credit() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(997_530).unwrap() else {
            panic!("expected reviewed Coordinated Conversion 997530");
        };
        assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
        assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
        assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
        assert_eq!(rule.magnitude_basis_points, Some(500));
        assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn healing_driven_damage_procs_remain_the_healers_ordinary_damage() {
        for effect_id in [997_539, 997_540, 997_541] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed healing-driven owner effect {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
            assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
            assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
            assert!(!rule.attribution_enabled);
        }
    }

    #[test]
    fn rogue_imagine_summon_roots_do_not_duplicate_downstream_effect_credit() {
        for effect_id in [997_542, 997_543, 997_544] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Rogue Imagine summon marker {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::InternalMarker);
            assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
            assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
            assert_eq!(rule.magnitude_basis_points, None);
            assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
            assert!(!rule.attribution_enabled);
        }
    }

    #[test]
    fn rogue_defensive_synergies_cannot_enter_damage_attribution() {
        for effect_id in [
            997_521, 997_522, 997_523, 997_524, 997_525, 997_526, 997_527, 997_528, 997_529,
            997_531, 997_532,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Rogue defensive Synergy {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert!(!rule.attribution_enabled);
            assert!(matches!(
                rule.contribution_kind,
                RdpsContributionKind::HealingSupport
                    | RdpsContributionKind::Mitigation
                    | RdpsContributionKind::InternalMarker
            ));
        }
    }

    #[test]
    fn energy_synergy_retains_exact_resource_formula_as_zero_credit_candidate() {
        let RdpsEffectLookup::Reviewed(root) = classify_rdps_effect(997_519).unwrap() else {
            panic!("expected reviewed Energy Synergy Domain root 997519");
        };
        assert_eq!(root.review_state, RdpsReviewState::NonContributing);
        assert_eq!(root.contribution_kind, RdpsContributionKind::InternalMarker);
        assert!(!root.attribution_enabled);

        let RdpsEffectLookup::Reviewed(resource) = classify_rdps_effect(997_520).unwrap() else {
            panic!("expected reviewed Energy Synergy Domain child 997520");
        };
        assert_eq!(resource.review_state, RdpsReviewState::Candidate);
        assert_eq!(
            resource.contribution_kind,
            RdpsContributionKind::ResourceSupport
        );
        assert_eq!(resource.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(resource.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(resource.magnitude_basis_points, Some(10_000));
        assert_eq!(resource.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!resource.attribution_enabled);
    }

    #[test]
    fn aoyi_target_mitigation_dispositions_match_specialized_runtime_authority() {
        for effect_id in [2_110_078, 2_110_092, 2_110_167] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Aoyi target-mitigation effect {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::Candidate);
            assert_eq!(
                rule.contribution_kind,
                RdpsContributionKind::TargetVulnerability
            );
            assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
            assert_eq!(rule.target_scope, RdpsTargetScope::Enemy);
            assert!(!rule.attribution_enabled);
        }

        let RdpsEffectLookup::Reviewed(poison) = classify_rdps_effect(2_110_099).unwrap() else {
            panic!("expected reviewed Arcane! Poison Explosion effect 2110099");
        };
        assert_eq!(poison.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            poison.contribution_kind,
            RdpsContributionKind::TargetVulnerability
        );
        assert_eq!(poison.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(poison.target_scope, RdpsTargetScope::Enemy);
        assert_eq!(poison.stacking_rule, RdpsStackingRule::StackScaled);
        assert!(
            !poison.attribution_enabled,
            "the specialized state projector owns tier, stacks, overlap, and conservation"
        );

        let RdpsEffectLookup::Reviewed(emitter) = classify_rdps_effect(2_110_166).unwrap() else {
            panic!("expected reviewed Celestial Spirit Mage emitter 2110166");
        };
        assert_eq!(emitter.review_state, RdpsReviewState::NonContributing);
        assert_eq!(
            emitter.contribution_kind,
            RdpsContributionKind::InternalMarker
        );
        assert_eq!(emitter.target_scope, RdpsTargetScope::Enemy);
        assert!(!emitter.attribution_enabled);
    }

    #[test]
    fn sanctuary_aura_shield_is_defensive_support_not_damage_credit() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(998_341).unwrap() else {
            panic!("expected reviewed Sanctuary Aura shield 998341");
        };
        assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::Mitigation);
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn synergy_luck_field_retains_root_and_exact_external_proc_child() {
        let RdpsEffectLookup::Reviewed(root) = classify_rdps_effect(997_533).unwrap() else {
            panic!("expected reviewed Synergy Luck Field root 997533");
        };
        assert_eq!(root.review_state, RdpsReviewState::NonContributing);
        assert_eq!(root.contribution_kind, RdpsContributionKind::InternalMarker);

        let RdpsEffectLookup::Reviewed(child) = classify_rdps_effect(997_534).unwrap() else {
            panic!("expected reviewed Synergy Luck Field child 997534");
        };
        assert_eq!(child.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            child.contribution_kind,
            RdpsContributionKind::DirectDamageAmplification
        );
        assert_eq!(child.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(child.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(child.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!child.attribution_enabled);
    }

    #[test]
    fn tactical_blessing_retains_root_and_exact_external_chance_child() {
        let RdpsEffectLookup::Reviewed(root) = classify_rdps_effect(997_557).unwrap() else {
            panic!("expected reviewed Tactical Blessing root 997557");
        };
        assert_eq!(root.review_state, RdpsReviewState::NonContributing);
        assert_eq!(root.contribution_kind, RdpsContributionKind::InternalMarker);
        assert!(!root.attribution_enabled);

        let RdpsEffectLookup::Reviewed(child) = classify_rdps_effect(997_570).unwrap() else {
            panic!("expected reviewed Tactical Blessing child 997570");
        };
        assert_eq!(child.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            child.contribution_kind,
            RdpsContributionKind::OffensiveStatBoost
        );
        assert_eq!(child.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(child.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(child.magnitude_basis_points, Some(1_000));
        assert_eq!(child.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!child.attribution_enabled);
    }

    #[test]
    fn all_class_aura_retains_exact_role_scaled_party_attack_formula() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(998_542).unwrap() else {
            panic!("expected reviewed All-Class Aura effect 998542");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            rule.contribution_kind,
            RdpsContributionKind::OffensiveStatBoost
        );
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn pulse_family_other_than_tactical_blessing_never_transfers_damage() {
        for effect_id in [
            997_550, 997_551, 997_552, 997_553, 997_554, 997_555, 997_556, 997_558, 997_559,
            997_560, 997_561, 997_562, 997_563, 997_564, 997_565, 997_566, 997_567, 997_568,
            997_569,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Pulse-family disposition {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert!(!rule.attribution_enabled);
            assert!(matches!(
                rule.contribution_kind,
                RdpsContributionKind::SelfOnly
                    | RdpsContributionKind::HealingSupport
                    | RdpsContributionKind::InternalMarker
            ));
        }
    }

    #[test]
    fn mastery_and_bloodwrath_families_are_owner_local_or_support_only() {
        for effect_id in (997_580..=997_599).chain(997_610..=997_642) {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Mastery/Bloodwrath disposition {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert!(!rule.attribution_enabled);
            assert!(matches!(
                rule.contribution_kind,
                RdpsContributionKind::SelfOnly
                    | RdpsContributionKind::HealingSupport
                    | RdpsContributionKind::Mitigation
                    | RdpsContributionKind::InternalMarker
            ));
        }
    }

    #[test]
    fn phantom_rally_is_owner_companion_attack_without_transfer_credit() {
        let RdpsEffectLookup::Reviewed(config) = classify_rdps_effect(2_110_082).unwrap() else {
            panic!("expected reviewed Phantom Rally config marker 2110082");
        };
        assert_eq!(config.review_state, RdpsReviewState::NonContributing);
        assert_eq!(
            config.contribution_kind,
            RdpsContributionKind::InternalMarker
        );
        assert_eq!(config.source_scope, RdpsSourceScope::Owner);
        assert_eq!(config.target_scope, RdpsTargetScope::SelfOnly);
        assert_eq!(config.magnitude_basis_points, None);
        assert_eq!(config.stacking_rule, RdpsStackingRule::Fixed);
        assert!(!config.attribution_enabled);

        let RdpsEffectLookup::Reviewed(status) = classify_rdps_effect(2_110_121).unwrap() else {
            panic!("expected reviewed Phantom Rally status 2110121");
        };
        assert_eq!(status.review_state, RdpsReviewState::NonContributing);
        assert_eq!(status.contribution_kind, RdpsContributionKind::SelfOnly);
        assert_eq!(status.source_scope, RdpsSourceScope::Owner);
        assert_eq!(status.target_scope, RdpsTargetScope::SelfOnly);
        assert_eq!(status.magnitude_basis_points, None);
        assert_eq!(status.stacking_rule, RdpsStackingRule::Fixed);
        assert!(!status.attribution_enabled);
    }

    #[test]
    fn party_offense_roots_do_not_duplicate_their_applied_child_credit() {
        for effect_id in [2_207_250, 2_302_120] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed party-offense parent marker {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::InternalMarker);
            assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
            assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
            assert_eq!(rule.magnitude_basis_points, None);
            assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
            assert!(!rule.attribution_enabled);
        }
    }

    #[test]
    fn critical_cold_child_retains_exact_external_crit_chance_role() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_204_471).unwrap() else {
            panic!("expected reviewed Critical Cold child 2204471");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            rule.contribution_kind,
            RdpsContributionKind::OffensiveStatBoost
        );
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, Some(300));
        assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn inspire_child_retains_exact_party_haste_opportunity_role() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(31_602).unwrap() else {
            panic!("expected reviewed Inspire child 31602");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::Haste);
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, Some(1_000));
        assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn swift_vortex_retains_nonstacking_party_haste_as_a_fail_closed_candidate() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_110_060).unwrap() else {
            panic!("expected reviewed Swift Vortex status 2110060");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Candidate);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::Haste);
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn unavailable_inspire_and_strengthen_is_owner_local() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_202_720).unwrap() else {
            panic!("expected reviewed Inspire and Strengthen status 2202720");
        };
        assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
        assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
        assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn battlelust_learned_owner_rows_do_not_stand_in_for_applied_aura_children() {
        for effect_id in [2_100_212, 2_100_300] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Battlelust owner marker {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::InternalMarker);
            assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
            assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
            assert_eq!(rule.magnitude_basis_points, None);
            assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
            assert!(!rule.attribution_enabled);
        }
    }

    #[test]
    fn unavailable_battlelust_applied_family_never_transfers_player_rdps() {
        for effect_id in [2_100_104, 2_100_105] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Battlelust healing row {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::HealingSupport);
            assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
            assert!(!rule.attribution_enabled);
        }

        let RdpsEffectLookup::Reviewed(enemy_debuff) = classify_rdps_effect(2_100_107).unwrap()
        else {
            panic!("expected reviewed Battlelust enemy debuff");
        };
        assert_eq!(enemy_debuff.review_state, RdpsReviewState::NonContributing);
        assert_eq!(
            enemy_debuff.contribution_kind,
            RdpsContributionKind::TargetVulnerability
        );
        assert_eq!(enemy_debuff.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(enemy_debuff.target_scope, RdpsTargetScope::Enemy);
        assert_eq!(enemy_debuff.magnitude_basis_points, None);
        assert_eq!(enemy_debuff.stacking_rule, RdpsStackingRule::Unresolved);
        assert!(!enemy_debuff.attribution_enabled);

        let RdpsEffectLookup::Reviewed(ally_buff) = classify_rdps_effect(2_100_108).unwrap() else {
            panic!("expected reviewed Battlelust ally buff");
        };
        assert_eq!(ally_buff.review_state, RdpsReviewState::NonContributing);
        assert_eq!(
            ally_buff.contribution_kind,
            RdpsContributionKind::Mitigation
        );
        assert_eq!(ally_buff.target_scope, RdpsTargetScope::PartyMembers);
        assert!(!ally_buff.attribution_enabled);

        for effect_id in [2_100_207, 2_100_208] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Battlelust aura marker {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::InternalMarker);
            assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
            assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
            assert!(!rule.attribution_enabled);
        }

        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn blessing_retains_exact_thirty_percent_party_damage_without_approximate_credit() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_100_154).unwrap() else {
            panic!("expected reviewed Blessing status 2100154");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            rule.contribution_kind,
            RdpsContributionKind::DirectDamageAmplification
        );
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, Some(3_000));
        assert_eq!(rule.stacking_rule, RdpsStackingRule::Unresolved);
        assert!(!rule.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn promoted_stat_resonance_and_team_luck_children_keep_composite_roles() {
        for effect_id in [2_207_252, 2_302_121] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed promoted composite child {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
            assert_eq!(
                rule.contribution_kind,
                RdpsContributionKind::OffensiveStatBoost
            );
            assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
            assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
            assert_eq!(rule.magnitude_basis_points, None);
            assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
            assert!(!rule.attribution_enabled);
        }
    }

    #[test]
    fn overhealing_cooldown_is_a_reviewed_internal_marker_without_damage_credit() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_202_113).unwrap() else {
            panic!("expected reviewed Overhealing cooldown marker 2202113");
        };
        assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::InternalMarker);
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
        assert!(!rule.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn exact_support_and_marker_batch_never_enters_damage_attribution() {
        for effect_id in [
            21_402, 21_404, 21_427, 21_428, 55_301, 55_302, 55_304, 55_314, 55_339, 55_342, 55_344,
            55_346, 55_361, 829_130, 2_100_412, 2_202_091, 2_202_112, 2_202_120, 2_202_262,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed healing-support effect {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::HealingSupport);
            assert!(!rule.attribution_enabled);
        }
        for effect_id in [
            21_408, 21_411, 21_413, 21_422, 55_226, 55_315, 55_407, 2_100_410, 2_100_411,
            2_110_117, 2_201_452, 2_202_705, 2_206_331, 3_003_070, 3_003_071,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed mitigation effect {effect_id}");
            };
            assert_eq!(rule.contribution_kind, RdpsContributionKind::Mitigation);
            assert!(!rule.attribution_enabled);
        }
        for effect_id in [
            2_110_050, 2_110_056, 2_110_057, 2_202_121, 2_202_261, 2_202_263, 3_057_111,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed internal marker {effect_id}");
            };
            assert_eq!(rule.contribution_kind, RdpsContributionKind::InternalMarker);
            assert!(!rule.attribution_enabled);
        }
        let RdpsEffectLookup::Reviewed(environmental) = classify_rdps_effect(884_173).unwrap()
        else {
            panic!("expected reviewed environmental visual marker 884173");
        };
        assert_eq!(
            environmental.contribution_kind,
            RdpsContributionKind::Environmental
        );
        assert!(!environmental.attribution_enabled);

        let RdpsEffectLookup::Reviewed(owner_only) = classify_rdps_effect(2_300_621).unwrap()
        else {
            panic!("expected reviewed owner-only module effect 2300621");
        };
        assert_eq!(owner_only.review_state, RdpsReviewState::NonContributing);
        assert_eq!(owner_only.contribution_kind, RdpsContributionKind::SelfOnly);
        assert_eq!(owner_only.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(owner_only.target_scope, RdpsTargetScope::Enemy);
        assert_eq!(owner_only.stacking_rule, RdpsStackingRule::StackScaled);
        assert!(!owner_only.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn exact_owner_self_batch_never_becomes_transferred_rdps() {
        for effect_id in [
            2_200_601, 2_200_602, 2_201_201, 2_201_220, 2_201_540, 2_201_570, 2_203_040, 2_203_220,
            2_203_530, 2_203_540, 2_206_551, 2_208_490, 2_406_150, 2_406_160, 2_407_290, 3_003_410,
            3_003_420, 3_003_440, 3_003_480,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed owner-self effect {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
            assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
            assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
            assert!(!rule.attribution_enabled);
        }
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn exact_owner_talent_tail_and_parent_ids_never_suppress_external_children() {
        for effect_id in [
            2_110_064, 2_200_320, 2_200_470, 2_200_600, 2_200_720, 2_201_200, 2_202_170, 2_202_540,
            2_202_570, 2_203_420, 2_203_450, 2_203_460, 2_204_320, 2_204_470, 2_204_520, 2_205_060,
            2_205_120, 2_205_140, 2_205_160, 2_205_200, 2_205_270, 2_205_380, 2_205_480, 2_205_510,
            2_206_110, 2_206_180, 2_206_240, 2_206_290, 2_206_400, 2_206_550, 2_206_680, 2_401_260,
            2_404_150, 3_002_410, 3_003_050,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed owner-local effect {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
            assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
            assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
            assert!(!rule.attribution_enabled);
        }
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn exact_description_dispositions_keep_owner_mechanics_out_of_rdps() {
        for effect_id in [
            2_201_330, 2_205_310, 2_208_260, 2_208_310, 2_405_150, 2_406_110, 2_407_280, 3_003_210,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed owner-only effect {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
            assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
            assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
            assert_eq!(rule.magnitude_basis_points, None);
            assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
            assert!(!rule.attribution_enabled);
        }
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn heroes_immortal_design_label_resolves_to_an_internal_counter() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_110_032).unwrap() else {
            panic!("expected reviewed Air Blade Thrust counter 2110032");
        };
        assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::InternalMarker);
        assert_eq!(rule.source_scope, RdpsSourceScope::Owner);
        assert_eq!(rule.target_scope, RdpsTargetScope::SelfOnly);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::StackScaled);
        assert!(!rule.attribution_enabled);
    }

    #[test]
    fn external_max_hp_is_a_current_build_candidate_without_runtime_authority() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_404_261).unwrap() else {
            panic!("expected reviewed external MaxHP candidate 2404261");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Candidate);
        assert_eq!(rule.contribution_kind, RdpsContributionKind::StateScaling);
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::StackScaled);
        assert!(!rule.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn observed_exaltation_anthems_are_environmental_and_never_credit_a_player() {
        for (effect_id, magnitude_basis_points) in
            [(702_003, 500), (702_004, 1_000), (702_005, 2_000)]
        {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Exaltation Anthem {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(
                rule.contribution_kind,
                RdpsContributionKind::DirectDamageAmplification
            );
            assert_eq!(rule.source_scope, RdpsSourceScope::Environment);
            assert_eq!(rule.target_scope, RdpsTargetScope::EffectTarget);
            assert_eq!(rule.magnitude_basis_points, Some(magnitude_basis_points));
            assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
            assert!(!rule.attribution_enabled);
        }
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn unclassified_effects_remain_explicit_timeline_evidence() {
        assert_eq!(
            classify_rdps_effect(2_203_291).unwrap(),
            RdpsEffectLookup::RetainedMappedUnclassified {
                effect_id: 2_203_291
            }
        );
        assert_eq!(
            classify_rdps_effect(9_999_999_999).unwrap(),
            RdpsEffectLookup::RetainedUnknownUnclassified {
                effect_id: 9_999_999_999
            }
        );
    }

    #[test]
    fn current_build_owner_only_wounding_curse_reviews_never_enter_the_reducer() {
        for effect_id in [2_203_031, 2_205_031] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed owner-only Wounding Curse {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::SelfOnly);
            assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
            assert_eq!(rule.target_scope, RdpsTargetScope::Enemy);
            assert_eq!(rule.magnitude_basis_points, Some(1_000));
            assert_eq!(rule.stacking_rule, RdpsStackingRule::Fixed);
            assert!(!rule.attribution_enabled);
        }

        let rules = confirmed_damage_contribution_rules().unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn current_build_damage_to_healing_and_ally_defense_never_enter_damage_attribution() {
        let RdpsEffectLookup::Reviewed(symbiotic_mark) = classify_rdps_effect(21_423).unwrap()
        else {
            panic!("expected reviewed Symbiotic Mark 21423");
        };
        assert_eq!(
            symbiotic_mark.review_state,
            RdpsReviewState::NonContributing
        );
        assert_eq!(
            symbiotic_mark.contribution_kind,
            RdpsContributionKind::HealingSupport
        );
        assert!(!symbiotic_mark.attribution_enabled);

        let RdpsEffectLookup::Reviewed(sandshroud) = classify_rdps_effect(2_201_452).unwrap()
        else {
            panic!("expected reviewed Sandshroud 2201452");
        };
        assert_eq!(sandshroud.review_state, RdpsReviewState::NonContributing);
        assert_eq!(
            sandshroud.contribution_kind,
            RdpsContributionKind::Mitigation
        );
        assert!(!sandshroud.attribution_enabled);

        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }

    #[test]
    fn target_vulnerability_identity_remains_candidate_while_exact_state_rule_is_promoted() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(55_228).unwrap() else {
            panic!("expected reviewed target vulnerability 55228");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Candidate);
        assert_eq!(
            rule.contribution_kind,
            RdpsContributionKind::TargetVulnerability
        );
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::EffectTarget);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!rule.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&55_228),
            "only the exact context-scoped state rule grants runtime credit"
        );
    }

    #[test]
    fn synergy_crit_field_child_is_confirmed_but_uses_the_critical_state_projector() {
        for effect_id in [997_536, 997_537] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed Synergy Crit Field marker {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::InternalMarker);
            assert!(!rule.attribution_enabled);
        }

        let RdpsEffectLookup::Reviewed(child) = classify_rdps_effect(997_538).unwrap() else {
            panic!("expected reviewed Synergy Crit Field recipient child");
        };
        assert_eq!(child.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            child.contribution_kind,
            RdpsContributionKind::DirectDamageAmplification
        );
        assert_eq!(child.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(child.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(child.magnitude_basis_points, Some(300));
        assert_eq!(child.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!child.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&997_538),
            "the exact critical-only state projector, not the generic unconditional reducer, grants runtime credit"
        );
    }

    #[test]
    fn element_sharing_child_is_confirmed_but_uses_the_element_state_projector() {
        let RdpsEffectLookup::Reviewed(root) = classify_rdps_effect(997_512).unwrap() else {
            panic!("expected reviewed Element Sharing root marker");
        };
        assert_eq!(root.review_state, RdpsReviewState::NonContributing);
        assert_eq!(root.contribution_kind, RdpsContributionKind::InternalMarker);
        assert!(!root.attribution_enabled);

        let RdpsEffectLookup::Reviewed(child) = classify_rdps_effect(997_513).unwrap() else {
            panic!("expected reviewed Element Sharing recipient child");
        };
        assert_eq!(child.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            child.contribution_kind,
            RdpsContributionKind::DirectDamageAmplification
        );
        assert_eq!(child.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(child.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(child.magnitude_basis_points, Some(2_000));
        assert_eq!(child.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!child.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&997_513),
            "the exact element-bucket state projector, not the generic unconditional reducer, grants runtime credit"
        );
    }

    #[test]
    fn coordinated_strike_child_is_confirmed_but_uses_the_attack_state_projector() {
        let RdpsEffectLookup::Reviewed(root) = classify_rdps_effect(997_510).unwrap() else {
            panic!("expected reviewed Coordinated Strike root marker");
        };
        assert_eq!(root.review_state, RdpsReviewState::NonContributing);
        assert_eq!(root.contribution_kind, RdpsContributionKind::InternalMarker);
        assert!(!root.attribution_enabled);

        let RdpsEffectLookup::Reviewed(child) = classify_rdps_effect(997_511).unwrap() else {
            panic!("expected reviewed Coordinated Strike recipient child");
        };
        assert_eq!(child.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            child.contribution_kind,
            RdpsContributionKind::DirectDamageAmplification
        );
        assert_eq!(child.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(child.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(child.magnitude_basis_points, Some(1_500));
        assert_eq!(child.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!child.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&997_511),
            "the exact Attack-percent state projector, not the generic unconditional reducer, grants runtime credit"
        );
    }

    #[test]
    fn enhanced_synergy_child_is_confirmed_but_uses_the_boost_state_projector() {
        let RdpsEffectLookup::Reviewed(root) = classify_rdps_effect(997_517).unwrap() else {
            panic!("expected reviewed Enhanced Synergy root marker");
        };
        assert_eq!(root.review_state, RdpsReviewState::NonContributing);
        assert_eq!(root.contribution_kind, RdpsContributionKind::InternalMarker);
        assert!(!root.attribution_enabled);

        let RdpsEffectLookup::Reviewed(child) = classify_rdps_effect(997_518).unwrap() else {
            panic!("expected reviewed Enhanced Synergy recipient child");
        };
        assert_eq!(child.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            child.contribution_kind,
            RdpsContributionKind::DirectDamageAmplification
        );
        assert_eq!(child.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(child.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(child.magnitude_basis_points, Some(1_000));
        assert_eq!(child.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!child.attribution_enabled);
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&997_518),
            "the exact boost-bucket state projector grants runtime credit"
        );
    }

    #[test]
    fn attribute_transfer_child_is_confirmed_but_uses_the_lane_state_projector() {
        let RdpsEffectLookup::Reviewed(root) = classify_rdps_effect(997_514).unwrap() else {
            panic!("expected reviewed Attribute Transfer root marker");
        };
        assert_eq!(root.review_state, RdpsReviewState::NonContributing);
        assert_eq!(root.contribution_kind, RdpsContributionKind::InternalMarker);
        assert!(!root.attribution_enabled);

        let RdpsEffectLookup::Reviewed(child) = classify_rdps_effect(997_515).unwrap() else {
            panic!("expected reviewed Attribute Transfer recipient child");
        };
        assert_eq!(child.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            child.contribution_kind,
            RdpsContributionKind::DirectDamageAmplification
        );
        assert_eq!(child.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(child.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(child.magnitude_basis_points, None);
        assert_eq!(child.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!child.attribution_enabled);
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&997_515),
            "only the exact lane-bound state projector grants runtime credit"
        );
    }

    #[test]
    fn mechanical_power_review_catalog_stays_separate_from_the_promoted_state_runtime() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_110_140).unwrap() else {
            panic!("expected reviewed Mechanical Power recipient effect");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            rule.contribution_kind,
            RdpsContributionKind::OffensiveStatBoost
        );
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!rule.attribution_enabled);
        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&2_110_140),
            "only the exact class-11 +750 packet-final proportional state route is promoted"
        );
    }

    #[test]
    fn inspiration_keeps_catalog_identity_separate_from_promoted_chance_components() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_202_041).unwrap() else {
            panic!("expected reviewed Inspiration recipient child");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            rule.contribution_kind,
            RdpsContributionKind::OffensiveStatBoost
        );
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!rule.attribution_enabled);
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&2_202_041),
            "the specialized projector preserves each proven Inspiration lane without a generic composite scalar"
        );
    }

    #[test]
    fn functional_amp_uses_the_dormant_current_state_runtime_not_the_older_review_catalog() {
        let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(2_110_143).unwrap() else {
            panic!("expected reviewed Functional Amp recipient child");
        };
        assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
        assert_eq!(
            rule.contribution_kind,
            RdpsContributionKind::OffensiveStatBoost
        );
        assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
        assert_eq!(rule.target_scope, RdpsTargetScope::PartyMembers);
        assert_eq!(rule.magnitude_basis_points, None);
        assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
        assert!(!rule.attribution_enabled);
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&2_110_143),
            "the migrated rule is armed, while the projector still requires a live exact lifecycle and reversible +360 packet transition before credit"
        );
    }

    #[test]
    fn production_effect_identities_never_flatten_specialized_formula_paths() {
        let expected = [
            (
                55_333,
                RdpsContributionKind::DirectDamageAmplification,
                RdpsTargetScope::PartyMembers,
                None,
            ),
            (
                2_110_065,
                RdpsContributionKind::OffensiveStatBoost,
                RdpsTargetScope::PartyMembers,
                Some(1_000),
            ),
            (
                2_110_125,
                RdpsContributionKind::DirectDamageAmplification,
                RdpsTargetScope::PartyMembers,
                None,
            ),
            (
                3_003_052,
                RdpsContributionKind::OffensiveStatBoost,
                RdpsTargetScope::EffectTarget,
                Some(200),
            ),
        ];

        for (effect_id, kind, target_scope, magnitude) in expected {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed production effect {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::Confirmed);
            assert_eq!(rule.contribution_kind, kind);
            assert_eq!(rule.source_scope, RdpsSourceScope::EffectSource);
            assert_eq!(rule.target_scope, target_scope);
            assert_eq!(rule.magnitude_basis_points, magnitude);
            assert_eq!(rule.stacking_rule, RdpsStackingRule::RefreshOnly);
            assert!(!rule.attribution_enabled);
            assert!(
                crate::proven_state_damage_contribution_effect_ids()
                    .unwrap()
                    .contains(&effect_id),
                "effect {effect_id} must remain owned by its specialized state projector"
            );
        }

        assert!(confirmed_damage_contribution_rules().unwrap().is_empty());
    }
}
