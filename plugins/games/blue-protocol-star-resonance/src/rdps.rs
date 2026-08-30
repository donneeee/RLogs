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

    const REVIEW_SOURCES: [&str; 13] = [
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
            "../game-data/catalog/rdps-effects/self-only/confirmed/2300621-dmg-stack.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/unclassified/unclassified/2404261-external-maxhp-current-build.json"
        ),
    ];

    const REVIEW_BATCH_SOURCES: [&str; 2] = [
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/current-build-support-and-marker-batch.v1.json"
        ),
        include_str!(
            "../game-data/catalog/rdps-effects/non-contributing/confirmed/current-build-owner-self-and-support-batch.v1.json"
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
    fn older_build_candidates_are_retained_but_not_claimed_as_current_review() {
        for effect_id in [702_004, 3_057_200] {
            assert_eq!(
                classify_rdps_effect(effect_id).unwrap(),
                RdpsEffectLookup::RetainedMappedUnclassified { effect_id }
            );
        }
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
            21_402, 21_427, 21_428, 55_301, 55_302, 55_304, 55_314, 55_339, 55_342, 55_344, 55_346,
            55_361, 829_130, 2_100_412, 2_202_091, 2_202_262,
        ] {
            let RdpsEffectLookup::Reviewed(rule) = classify_rdps_effect(effect_id).unwrap() else {
                panic!("expected reviewed healing-support effect {effect_id}");
            };
            assert_eq!(rule.review_state, RdpsReviewState::NonContributing);
            assert_eq!(rule.contribution_kind, RdpsContributionKind::HealingSupport);
            assert!(!rule.attribution_enabled);
        }
        for effect_id in [
            21_408, 21_411, 21_413, 55_226, 55_407, 2_110_117, 2_201_452, 2_206_331, 3_003_071,
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
            2_200_601, 2_200_602, 2_201_201, 2_206_551, 2_406_150, 2_406_160, 3_003_410, 3_003_420,
            3_003_480,
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
    fn unresolved_external_survival_status_remains_unclassified_and_fail_closed() {
        let effect_id = 2_110_032;
        assert_eq!(
            classify_rdps_effect(effect_id).unwrap(),
            RdpsEffectLookup::RetainedMappedUnclassified { effect_id }
        );
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
        for (effect_id, magnitude_basis_points) in [(702_003, 500), (702_005, 2_000)] {
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
    fn mechanical_power_review_catalog_stays_separate_from_the_promoted_state_runtime() {
        assert_eq!(
            classify_rdps_effect(2_110_140).unwrap(),
            RdpsEffectLookup::RetainedMappedUnclassified {
                effect_id: 2_110_140
            }
        );
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
        assert_eq!(
            classify_rdps_effect(2_202_041).unwrap(),
            RdpsEffectLookup::RetainedMappedUnclassified {
                effect_id: 2_202_041
            }
        );
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&2_202_041),
            "only the Crit-only and Lucky-only chance components are production enabled; the catalog remains unclassified and the combined, dependency, Attack, Mastery, and haste lanes remain uncredited"
        );
    }

    #[test]
    fn functional_amp_uses_the_dormant_current_state_runtime_not_the_older_review_catalog() {
        assert_eq!(
            classify_rdps_effect(2_110_143).unwrap(),
            RdpsEffectLookup::RetainedMappedUnclassified {
                effect_id: 2_110_143
            }
        );
        assert!(
            crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&2_110_143),
            "the migrated rule is armed, while the projector still requires a live exact lifecycle and reversible +360 packet transition before credit"
        );
    }
}
