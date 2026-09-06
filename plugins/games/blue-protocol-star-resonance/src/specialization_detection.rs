use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use serde::Deserialize;

const BUNDLED_SPECIALIZATION_DETECTION: &str =
    include_str!("../game-data/runtime/specialization-detection.v2.json");
pub const SPECIALIZATION_DETECTION_GAME_BUILD: &str = "24687926";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecializationDetectionDefinition {
    schema_version: u16,
    client_build: String,
    specializations: Vec<SpecializationEvidenceDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecializationEvidenceDefinition {
    specialization_id: i32,
    class_id: i32,
    primary_ability_ids: Vec<i64>,
    supporting_ability_ids: Vec<i64>,
    passive_selector_ids: Vec<i64>,
    talent_node_ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SpecializationAbilityEvidenceStrength {
    Supporting,
    Primary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpecializationAbilityEvidence {
    pub(crate) class_id: i32,
    pub(crate) specialization_id: i32,
    pub(crate) strength: SpecializationAbilityEvidenceStrength,
}

#[derive(Debug)]
struct SpecializationDetectionCatalog {
    by_class_ability: HashMap<(i32, i64), SpecializationAbilityEvidence>,
    by_class_passive_selector: HashMap<(i32, i64), i32>,
    by_class_talent: HashMap<(i32, i64), i32>,
    by_unique_ability: HashMap<i64, SpecializationAbilityEvidence>,
    by_unique_passive_selector: HashMap<i64, (i32, i32)>,
    talent_nodes_by_specialization: HashMap<i32, Vec<i64>>,
}

fn catalog() -> Result<&'static SpecializationDetectionCatalog, String> {
    static CATALOG: OnceLock<Result<SpecializationDetectionCatalog, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let definition: SpecializationDetectionDefinition =
                serde_json::from_str(BUNDLED_SPECIALIZATION_DETECTION).map_err(|error| {
                    format!("bundled BPSR specialization detection is invalid: {error}")
                })?;
            if definition.schema_version != 2
                || definition.client_build != SPECIALIZATION_DETECTION_GAME_BUILD
                || definition.specializations.is_empty()
                || definition.specializations.len() > 64
            {
                return Err(
                    "bundled BPSR specialization detection has an unsupported shape".into(),
                );
            }

            let mut specialization_ids = BTreeSet::new();
            let mut by_class_ability = HashMap::new();
            let mut by_class_passive_selector = HashMap::new();
            let mut by_class_talent = HashMap::new();
            let mut by_unique_ability = HashMap::new();
            let mut by_unique_passive_selector = HashMap::new();
            let mut talent_nodes_by_specialization = HashMap::new();
            for evidence in definition.specializations {
                if evidence.specialization_id <= 0
                    || evidence.class_id <= 0
                    || (evidence.primary_ability_ids.is_empty()
                        && evidence.supporting_ability_ids.is_empty()
                        && evidence.passive_selector_ids.is_empty()
                        && evidence.talent_node_ids.is_empty())
                    || evidence.primary_ability_ids.len() > 64
                    || evidence.supporting_ability_ids.len() > 64
                    || evidence.passive_selector_ids.len() > 64
                    || evidence.talent_node_ids.len() > 64
                    || !specialization_ids.insert(evidence.specialization_id)
                {
                    return Err(format!(
                        "bundled BPSR specialization {} has invalid detection metadata",
                        evidence.specialization_id
                    ));
                }

                let mut talent_node_ids = evidence.talent_node_ids.clone();
                talent_node_ids.sort_unstable();
                talent_node_ids.dedup();
                talent_nodes_by_specialization.insert(evidence.specialization_id, talent_node_ids);

                for (ability_ids, strength) in [
                    (
                        evidence.primary_ability_ids,
                        SpecializationAbilityEvidenceStrength::Primary,
                    ),
                    (
                        evidence.supporting_ability_ids,
                        SpecializationAbilityEvidenceStrength::Supporting,
                    ),
                ] {
                    for ability_id in ability_ids {
                        let ability_evidence = SpecializationAbilityEvidence {
                            class_id: evidence.class_id,
                            specialization_id: evidence.specialization_id,
                            strength,
                        };
                        if ability_id <= 0
                            || by_class_ability
                                .insert((evidence.class_id, ability_id), ability_evidence)
                                .is_some()
                            || by_unique_ability
                                .insert(ability_id, ability_evidence)
                                .is_some()
                        {
                            return Err(format!(
                                "bundled BPSR specialization ability {ability_id} is ambiguous"
                            ));
                        }
                    }
                }
                for selector_id in evidence.passive_selector_ids {
                    if selector_id <= 0
                        || by_class_passive_selector
                            .insert(
                                (evidence.class_id, selector_id),
                                evidence.specialization_id,
                            )
                            .is_some()
                        || by_unique_passive_selector
                            .insert(
                                selector_id,
                                (evidence.class_id, evidence.specialization_id),
                            )
                            .is_some()
                    {
                        return Err(format!(
                            "bundled BPSR specialization passive selector {selector_id} is ambiguous"
                        ));
                    }
                }
                for talent_node_id in evidence.talent_node_ids {
                    if talent_node_id <= 0
                        || by_class_talent
                            .insert(
                                (evidence.class_id, talent_node_id),
                                evidence.specialization_id,
                            )
                            .is_some()
                    {
                        return Err(format!(
                            "bundled BPSR specialization talent {talent_node_id} is ambiguous"
                        ));
                    }
                }
            }

            Ok(SpecializationDetectionCatalog {
                by_class_ability,
                by_class_passive_selector,
                by_class_talent,
                by_unique_ability,
                by_unique_passive_selector,
                talent_nodes_by_specialization,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Returns the exact talent-selector IDs associated with a runtime specialization.
///
/// This is an identity crosswalk only. It does not imply that a remote player has
/// selected any child talent, passive, or Imagine dependency.
pub fn specialization_talent_node_ids(
    specialization_id: i32,
) -> Result<Option<&'static [i64]>, String> {
    Ok(catalog()?
        .talent_nodes_by_specialization
        .get(&specialization_id)
        .map(Vec::as_slice))
}

#[cfg(test)]
pub(crate) fn specialization_from_ability_id(
    class_id: i32,
    ability_id: i64,
) -> Result<Option<i32>, String> {
    Ok(catalog()?
        .by_class_ability
        .get(&(class_id, ability_id))
        .map(|evidence| evidence.specialization_id))
}

pub(crate) fn specialization_ability_evidence(
    class_id: Option<i32>,
    ability_id: i64,
) -> Result<Option<SpecializationAbilityEvidence>, String> {
    let catalog = catalog()?;
    Ok(match class_id {
        Some(class_id) => catalog
            .by_class_ability
            .get(&(class_id, ability_id))
            .copied(),
        None => catalog.by_unique_ability.get(&ability_id).copied(),
    })
}

pub fn specialization_from_observed_abilities(
    class_id: i32,
    ability_ids: impl IntoIterator<Item = i64>,
) -> Result<Option<i32>, String> {
    let catalog = catalog()?;
    let mut primary_matches = BTreeSet::new();
    let mut supporting_matches = BTreeSet::new();
    for ability_id in ability_ids {
        let Some(evidence) = catalog.by_class_ability.get(&(class_id, ability_id)) else {
            continue;
        };
        match evidence.strength {
            SpecializationAbilityEvidenceStrength::Primary => {
                primary_matches.insert(evidence.specialization_id);
            }
            SpecializationAbilityEvidenceStrength::Supporting => {
                supporting_matches.insert(evidence.specialization_id);
            }
        }
    }
    let matches = if primary_matches.is_empty() {
        supporting_matches
    } else {
        primary_matches
    };
    Ok((matches.len() == 1).then(|| *matches.first().expect("one specialization match")))
}

pub fn specialization_identity_from_observed_abilities(
    ability_ids: impl IntoIterator<Item = i64>,
) -> Result<Option<(i32, i32)>, String> {
    let catalog = catalog()?;
    let mut primary_matches = BTreeSet::new();
    let mut supporting_matches = BTreeSet::new();
    for ability_id in ability_ids {
        let Some(evidence) = catalog.by_unique_ability.get(&ability_id) else {
            continue;
        };
        let identity = (evidence.class_id, evidence.specialization_id);
        match evidence.strength {
            SpecializationAbilityEvidenceStrength::Primary => {
                primary_matches.insert(identity);
            }
            SpecializationAbilityEvidenceStrength::Supporting => {
                supporting_matches.insert(identity);
            }
        }
    }
    let matches = if primary_matches.is_empty() {
        supporting_matches
    } else {
        primary_matches
    };
    Ok((matches.len() == 1).then(|| *matches.first().expect("one specialization identity match")))
}

pub(crate) fn specialization_from_passive_selectors(
    class_id: i32,
    selector_ids: impl IntoIterator<Item = i64>,
) -> Result<Option<i32>, String> {
    let catalog = catalog()?;
    let matches = selector_ids
        .into_iter()
        .filter_map(|selector_id| {
            catalog
                .by_class_passive_selector
                .get(&(class_id, selector_id))
                .copied()
        })
        .collect::<BTreeSet<_>>();
    Ok((matches.len() == 1).then(|| *matches.first().expect("one specialization match")))
}

pub(crate) fn specialization_identity_from_passive_selectors(
    selector_ids: impl IntoIterator<Item = i64>,
) -> Result<Option<(i32, i32)>, String> {
    let catalog = catalog()?;
    let matches = selector_ids
        .into_iter()
        .filter_map(|selector_id| {
            catalog
                .by_unique_passive_selector
                .get(&selector_id)
                .copied()
        })
        .collect::<BTreeSet<_>>();
    Ok((matches.len() == 1).then(|| *matches.first().expect("one specialization match")))
}

pub(crate) fn specialization_from_evidence(
    class_id: i32,
    ability_ids: impl IntoIterator<Item = i64>,
    talent_node_ids: impl IntoIterator<Item = i64>,
) -> Result<Option<i32>, String> {
    let catalog = catalog()?;
    let ability_match = specialization_from_observed_abilities(class_id, ability_ids)?;
    let mut talent_matches = BTreeSet::new();
    for talent_node_id in talent_node_ids {
        if let Some(specialization_id) = catalog.by_class_talent.get(&(class_id, talent_node_id)) {
            talent_matches.insert(*specialization_id);
        }
    }
    if !talent_matches.is_empty() {
        return Ok((talent_matches.len() == 1)
            .then(|| *talent_matches.first().expect("one specialization match")));
    }
    Ok(ability_match)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_build_skill_pairs_resolve_inside_the_observed_class() {
        assert_eq!(specialization_from_ability_id(5, 1_518).unwrap(), Some(110));
        assert_eq!(
            specialization_from_ability_id(12, 2_406).unwrap(),
            Some(123)
        );
        assert_eq!(
            specialization_from_ability_id(11, 2_233).unwrap(),
            Some(117)
        );
        assert_eq!(specialization_from_ability_id(5, 2_406).unwrap(), None);
    }

    #[test]
    fn one_runtime_skill_can_restore_a_missing_class_and_specialization() {
        let evidence = specialization_ability_evidence(None, 2_301)
            .unwrap()
            .expect("unique ability evidence");
        assert_eq!((evidence.class_id, evidence.specialization_id), (13, 120));
        assert_eq!(
            specialization_identity_from_observed_abilities([2_233]).unwrap(),
            Some((11, 117))
        );
    }

    #[test]
    fn conflicting_evidence_never_guesses_a_specialization() {
        assert_eq!(
            specialization_from_evidence(12, [2_405, 2_406], []).unwrap(),
            None
        );
    }

    #[test]
    fn selected_talent_root_resolves_the_local_specialization() {
        assert_eq!(
            specialization_from_evidence(3, [], [312]).unwrap(),
            Some(128)
        );
    }

    #[test]
    fn current_falconry_runtime_identity_crosswalks_to_its_talent_selector() {
        assert_eq!(
            specialization_talent_node_ids(117).unwrap(),
            Some([1_129].as_slice())
        );
        assert_eq!(specialization_talent_node_ids(999).unwrap(), None);
    }

    #[test]
    fn selected_talent_root_outranks_retained_cross_spec_skills() {
        assert_eq!(
            specialization_from_evidence(3, [1_606, 1_613], [312]).unwrap(),
            Some(128)
        );
    }

    #[test]
    fn current_passive_selector_resolves_even_when_class_is_not_yet_known() {
        assert_eq!(
            specialization_identity_from_passive_selectors([2_208_130]).unwrap(),
            Some((3, 128))
        );
        assert_eq!(
            specialization_from_passive_selectors(3, [2_208_130]).unwrap(),
            Some(128)
        );
    }

    #[test]
    fn shared_twin_striker_skill_is_deliberately_not_a_signature() {
        assert_eq!(specialization_from_ability_id(3, 1_605).unwrap(), None);
    }

    #[test]
    fn twin_striker_observed_action_families_resolve_both_specializations() {
        assert_eq!(
            specialization_from_observed_abilities(3, [1_607, 1_608, 1_612]).unwrap(),
            Some(128)
        );
        assert_eq!(
            specialization_from_observed_abilities(3, [1_606, 1_613]).unwrap(),
            Some(129)
        );
        assert_eq!(
            specialization_from_ability_id(3, 35_107).unwrap(),
            Some(128)
        );
        assert_eq!(
            specialization_from_ability_id(3, 35_104).unwrap(),
            Some(129)
        );
    }

    #[test]
    fn twin_striker_primary_actions_outrank_cross_spec_supporting_procs() {
        assert_eq!(
            specialization_from_observed_abilities(3, [1_606, 35_107, 35_108]).unwrap(),
            Some(129)
        );
        assert_eq!(
            specialization_from_observed_abilities(3, [1_607, 35_104, 35_105]).unwrap(),
            Some(128)
        );
    }
}
