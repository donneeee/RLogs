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
    Haste,
    OffensiveStatBoost,
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

    const REVIEW_SOURCES: [&str; 11] = [
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
    ];

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
        source_rules.sort_by_key(|rule| rule.effect_id);
        assert_eq!(source_rules, catalog.effects);
    }

    #[test]
    fn older_build_candidates_are_retained_but_not_claimed_as_current_review() {
        for effect_id in [702_003, 702_004, 702_005, 3_056_391, 3_057_200] {
            assert_eq!(
                classify_rdps_effect(effect_id).unwrap(),
                RdpsEffectLookup::RetainedMappedUnclassified { effect_id }
            );
        }
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
    fn older_build_owner_only_reviews_do_not_leak_into_the_current_reducer() {
        for effect_id in [2_203_031, 2_205_031, 2_300_621] {
            assert_eq!(
                classify_rdps_effect(effect_id).unwrap(),
                RdpsEffectLookup::RetainedMappedUnclassified { effect_id }
            );
        }

        let rules = confirmed_damage_contribution_rules().unwrap();
        assert!(rules.is_empty());
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
    fn mechanical_power_uses_the_scoped_state_runtime_not_the_review_catalog() {
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
            "only the observed class-11 +750 proportional component is production enabled; other tiers, classes, haste, and overlaps remain uncredited"
        );
    }

    #[test]
    fn inspiration_stays_retained_until_the_current_state_runtime_promotes_it() {
        assert_eq!(
            classify_rdps_effect(2_202_041).unwrap(),
            RdpsEffectLookup::RetainedMappedUnclassified {
                effect_id: 2_202_041
            }
        );
        assert!(
            !crate::proven_state_damage_contribution_effect_ids()
                .unwrap()
                .contains(&2_202_041),
            "packet-observed Inspiration state may be retained and explained without claiming that every required damage lane is attribution-ready"
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
