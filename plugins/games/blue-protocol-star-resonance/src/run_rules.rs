use std::collections::{BTreeMap, BTreeSet};

use rlogs_combat::{
    ActivityKind, DifficultyTierRange, RaidRouteKind, RunReducerConfig, RunRuleCatalog,
    RunRuleCatalogError, RunRuleConfidence, RunRuleEvidence, RunRuleTarget, SceneRunRule,
};
use serde::Deserialize;
use thiserror::Error;

include!(concat!(env!("OUT_DIR"), "/bundled_dungeon_seasons.rs"));

const TINA_MINDREALM_RULES: &[u8] =
    include_bytes!("../run-rules/global/steam-24252055/activities/tina-mindrealm.json");
const UNSTABLE_TINA_MINDREALM_RULES: &[u8] =
    include_bytes!("../run-rules/global/steam-24252055/activities/unstable-tina-mindrealm.json");
const GUILD_HUNT_RULES: &[u8] =
    include_bytes!("../run-rules/global/steam-24252055/activities/guild-hunt.json");
const MECH_FACILITY_RULES: &[u8] =
    include_bytes!("../run-rules/global/steam-24252055/activities/mech-facility.json");
const SEA_RINGED_REEF_RULES: &[u8] =
    include_bytes!("../run-rules/global/steam-24252055/activities/sea-ringed-reef.json");
const FIELD_OF_FORGOTTEN_ILLUSIONS_RULES: &[u8] = include_bytes!(
    "../run-rules/global/steam-24252055/activities/field-of-forgotten-illusions.json"
);
const STIMEN_VAULTS_RULES: &[u8] =
    include_bytes!("../run-rules/global/steam-24252055/activities/stimen-vaults.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BpsrSceneRunIdentity {
    pub activity_id: String,
    pub activity_family_id: Option<String>,
    pub difficulty_family: Option<String>,
}

pub fn bundled_run_rule_catalogs() -> Result<Vec<RunRuleCatalog>, BpsrRunRuleError> {
    let mut catalogs = [
        TINA_MINDREALM_RULES,
        UNSTABLE_TINA_MINDREALM_RULES,
        GUILD_HUNT_RULES,
        MECH_FACILITY_RULES,
        SEA_RINGED_REEF_RULES,
        FIELD_OF_FORGOTTEN_ILLUSIONS_RULES,
        STIMEN_VAULTS_RULES,
    ]
    .into_iter()
    .map(serde_json::from_slice::<RunRuleCatalog>)
    .collect::<Result<Vec<_>, _>>()?;
    for catalog in &catalogs {
        catalog.validate()?;
    }
    let reviewed_scenes = catalogs
        .iter()
        .flat_map(RunRuleCatalog::enabled_scene_rules)
        .map(|(scene_id, _)| scene_id)
        .collect::<BTreeSet<_>>();
    let ruleset_id = catalogs[0].ruleset_id.clone();
    let ruleset_version = catalogs[0].ruleset_version;
    let target = catalogs[0].target.clone();
    let generated_master_catalog =
        bundled_master_dungeon_catalog(&reviewed_scenes, ruleset_id, ruleset_version, target)?;
    generated_master_catalog.validate()?;
    catalogs.push(generated_master_catalog);
    Ok(catalogs)
}

fn bundled_master_dungeon_catalog(
    reviewed_scenes: &BTreeSet<i32>,
    ruleset_id: String,
    ruleset_version: u32,
    target: RunRuleTarget,
) -> Result<RunRuleCatalog, BpsrRunRuleError> {
    let mut scenes = BTreeMap::<i32, SceneRunRule>::new();
    for (file_name, bytes) in BUNDLED_DUNGEON_SEASONS {
        let season = serde_json::from_slice::<DungeonSeasonCatalog>(bytes)?;
        if season.schema_version != 2 || season.kind != "dungeon_season" {
            return Err(BpsrRunRuleError::InvalidDungeonSeason {
                file: (*file_name).to_owned(),
                reason: "expected dungeon-season schema version 2".to_owned(),
            });
        }
        if season.attributes.season_id != season.id {
            return Err(BpsrRunRuleError::InvalidDungeonSeason {
                file: (*file_name).to_owned(),
                reason: "top-level and attribute season IDs differ".to_owned(),
            });
        }
        for family in season.attributes.activity_families {
            if family.family != "master"
                || family.difficulty_family != "master"
                || family.tier_identity.minimum != 1
                || family.tier_identity.maximum != 20
                || family.tier_identity.count != 20
            {
                return Err(BpsrRunRuleError::InvalidDungeonSeason {
                    file: (*file_name).to_owned(),
                    reason: "master families must contain the complete M1-M20 identity".to_owned(),
                });
            }
            for activity in family.activities {
                if activity.scene_id <= 0
                    || activity.dungeon_id <= 0
                    || activity.scene_id != activity.dungeon_id
                    || activity.first_tier_row_id != activity.dungeon_id * 100 + 1
                    || activity.last_tier_row_id != activity.dungeon_id * 100 + 20
                {
                    return Err(BpsrRunRuleError::InvalidDungeonSeason {
                        file: (*file_name).to_owned(),
                        reason: format!(
                            "dungeon {} does not preserve its reviewed scene and tier-row identity",
                            activity.dungeon_id
                        ),
                    });
                }
                if reviewed_scenes.contains(&activity.scene_id) {
                    continue;
                }
                let scene_id = activity.scene_id;
                let rule = SceneRunRule {
                    scene_id,
                    runtime_enabled: true,
                    activity_kind: ActivityKind::Dungeon,
                    activity_id: format!("scene.{scene_id}"),
                    activity_family_id: Some(activity.dungeon_key),
                    activity_localization_key: Some(format!("scene.{scene_id}.name")),
                    difficulty_family: Some(family.difficulty_family.clone()),
                    difficulty_localization_key: Some(family.label_format_localization_key.clone()),
                    difficulty_tier_range: Some(DifficultyTierRange {
                        minimum: family.tier_identity.minimum,
                        maximum: family.tier_identity.maximum,
                    }),
                    route_id: None,
                    raid_route_kind: None,
                    partition: None,
                    candidate_dungeon_ids: BTreeSet::from([i64::from(activity.dungeon_id)]),
                    mobbing_encounter_id: None,
                    boss_encounter_id: None,
                    boss_monster_ids: BTreeSet::new(),
                    objective_rules: BTreeMap::new(),
                    evidence: vec![RunRuleEvidence {
                        source: "reviewed-dungeon-season-catalog".to_owned(),
                        reference: format!("{file_name}:scene-{scene_id}"),
                        confidence: RunRuleConfidence::Verified,
                    }],
                };
                if let Some(existing) = scenes.get_mut(&scene_id) {
                    if !same_generated_master_identity(existing, &rule) {
                        return Err(BpsrRunRuleError::ConflictingMasterScene(scene_id));
                    }
                    existing.evidence.extend(rule.evidence);
                } else {
                    scenes.insert(scene_id, rule);
                }
            }
        }
    }

    Ok(RunRuleCatalog {
        schema_version: 1,
        ruleset_id,
        ruleset_version,
        target,
        scenes: scenes.into_values().collect(),
    })
}

fn same_generated_master_identity(left: &SceneRunRule, right: &SceneRunRule) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.evidence.clear();
    right.evidence.clear();
    left == right
}

#[derive(Debug, Deserialize)]
struct DungeonSeasonCatalog {
    schema_version: u16,
    kind: String,
    id: u32,
    attributes: DungeonSeasonAttributes,
}

#[derive(Debug, Deserialize)]
struct DungeonSeasonAttributes {
    season_id: u32,
    activity_families: Vec<DungeonSeasonFamily>,
}

#[derive(Debug, Deserialize)]
struct DungeonSeasonFamily {
    family: String,
    difficulty_family: String,
    label_format_localization_key: String,
    tier_identity: DungeonTierIdentity,
    activities: Vec<DungeonSeasonActivity>,
}

#[derive(Debug, Deserialize)]
struct DungeonTierIdentity {
    minimum: u32,
    maximum: u32,
    count: u32,
}

#[derive(Debug, Deserialize)]
struct DungeonSeasonActivity {
    dungeon_id: i32,
    dungeon_key: String,
    scene_id: i32,
    first_tier_row_id: i32,
    last_tier_row_id: i32,
}

pub fn bundled_run_reducer_config() -> Result<RunReducerConfig, BpsrRunRuleError> {
    let catalogs = bundled_run_rule_catalogs()?;
    let mut config = RunReducerConfig::default();
    for catalog in catalogs {
        if config.encounter_ruleset_id.is_none() {
            config.encounter_ruleset_id = Some(catalog.ruleset_id.clone());
            config.encounter_ruleset_version = Some(catalog.ruleset_version);
        } else if config.encounter_ruleset_id.as_deref() != Some(&catalog.ruleset_id)
            || config.encounter_ruleset_version != Some(catalog.ruleset_version)
        {
            return Err(BpsrRunRuleError::MixedRulesets);
        }
        for (scene_id, rule) in catalog.enabled_scene_rules() {
            if config.scene_rules.insert(scene_id, rule).is_some() {
                return Err(BpsrRunRuleError::DuplicateScene(scene_id));
            }
        }
    }
    Ok(config)
}

pub fn bundled_gauntlet_scene_ids() -> Result<BTreeSet<i32>, BpsrRunRuleError> {
    Ok(bundled_run_reducer_config()?
        .scene_rules
        .into_iter()
        .filter_map(|(scene_id, rule)| {
            (rule.raid_route_kind == Some(RaidRouteKind::Gauntlet)).then_some(scene_id)
        })
        .collect())
}

pub fn bundled_scene_run_identities()
-> Result<BTreeMap<i32, BpsrSceneRunIdentity>, BpsrRunRuleError> {
    Ok(bundled_run_reducer_config()?
        .scene_rules
        .into_iter()
        .map(|(scene_id, rule)| {
            (
                scene_id,
                BpsrSceneRunIdentity {
                    activity_id: rule.activity_id,
                    activity_family_id: rule.activity_family_id,
                    difficulty_family: rule.difficulty_family,
                },
            )
        })
        .collect())
}

#[derive(Debug, Error)]
pub enum BpsrRunRuleError {
    #[error("could not decode bundled BPSR run rules: {0}")]
    Json(#[from] serde_json::Error),
    #[error("bundled BPSR run rules are invalid: {0}")]
    Validation(#[from] RunRuleCatalogError),
    #[error("bundled BPSR run-rule files use different ruleset identities")]
    MixedRulesets,
    #[error("bundled BPSR run rules repeat scene {0}")]
    DuplicateScene(i32),
    #[error("bundled BPSR dungeon-season catalog {file} is invalid: {reason}")]
    InvalidDungeonSeason { file: String, reason: String },
    #[error("bundled BPSR dungeon-season catalogs contradict master scene {0}")]
    ConflictingMasterScene(i32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tina_rules_keep_master_family_and_twenty_tiers_separate() {
        let catalogs = bundled_run_rule_catalogs().unwrap();
        let tina = &catalogs[0];
        let normal = tina
            .scenes
            .iter()
            .find(|rule| rule.scene_id == 1631)
            .unwrap();
        let master = tina
            .scenes
            .iter()
            .find(|rule| rule.scene_id == 1633)
            .unwrap();
        assert_eq!(normal.difficulty_family.as_deref(), Some("normal"));
        assert!(normal.runtime_enabled);
        assert_eq!(master.difficulty_family.as_deref(), Some("master"));
        assert_eq!(
            master.difficulty_tier_range,
            Some(rlogs_combat::DifficultyTierRange {
                minimum: 1,
                maximum: 20,
            })
        );
        assert!(!master.runtime_enabled);
    }

    #[test]
    fn reducer_config_enables_reviewed_current_build_scenes() {
        let config = bundled_run_reducer_config().unwrap();
        assert!(config.scene_rules.contains_key(&1631));
        assert!(config.scene_rules.contains_key(&1632));
        assert!(config.scene_rules.contains_key(&1621));
        assert!(config.scene_rules.contains_key(&12_022));
        assert!(config.scene_rules.contains_key(&12_023));
        assert!(config.scene_rules.contains_key(&6525));
        assert!(config.scene_rules.contains_key(&6565));
        assert!(config.scene_rules.contains_key(&13_021));
        assert!(config.scene_rules.contains_key(&13_022));
        assert!(config.scene_rules.contains_key(&13_023));
        assert!(config.scene_rules.contains_key(&32_101));
        assert!(config.scene_rules.contains_key(&32_160));
        assert!(config.scene_rules.contains_key(&1633));
        assert!(config.scene_rules.contains_key(&6515));
    }

    #[test]
    fn non_tiered_activity_modes_have_explicit_difficulty_families() {
        let identities = bundled_scene_run_identities().unwrap();
        assert_eq!(
            identities
                .get(&1621)
                .and_then(|identity| identity.difficulty_family.as_deref()),
            Some("unstable")
        );
        assert_eq!(
            identities
                .get(&12_022)
                .and_then(|identity| identity.difficulty_family.as_deref()),
            Some("normal")
        );
        assert_eq!(
            identities
                .get(&12_023)
                .and_then(|identity| identity.difficulty_family.as_deref()),
            Some("hard")
        );
        assert_eq!(
            identities
                .get(&6525)
                .and_then(|identity| identity.difficulty_family.as_deref()),
            Some("master")
        );
        assert_eq!(
            identities
                .get(&6565)
                .and_then(|identity| identity.difficulty_family.as_deref()),
            Some("master")
        );
        for (scene_id, difficulty) in [(13_021, "normal"), (13_022, "hard"), (13_023, "nightmare")]
        {
            assert_eq!(
                identities
                    .get(&scene_id)
                    .and_then(|identity| identity.difficulty_family.as_deref()),
                Some(difficulty)
            );
        }
    }

    #[test]
    fn all_current_stimen_vault_floors_are_explicitly_difficulty_less() {
        let config = bundled_run_reducer_config().unwrap();
        let floors = (32_101..=32_160)
            .map(|scene_id| config.scene_rules.get(&scene_id).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(floors.len(), 60);
        for (index, rule) in floors.into_iter().enumerate() {
            let scene_id = 32_101 + index as i32;
            assert_eq!(rule.activity_id, format!("scene.{scene_id}"));
            assert_eq!(rule.activity_family_id.as_deref(), Some("stimen-vaults"));
            assert_eq!(rule.difficulty_family, None);
            assert_eq!(rule.difficulty_tier_range, None);
        }
    }

    #[test]
    fn exact_scene_rules_preserve_identity_without_encoding_boss_counts() {
        let config = bundled_run_reducer_config().unwrap();
        for (scene_id, difficulty) in [(12_022, "normal"), (12_023, "hard")] {
            let rule = config.scene_rules.get(&scene_id).unwrap();
            assert_eq!(rule.activity_family_id.as_deref(), Some("guild-hunt"));
            assert_eq!(rule.difficulty_family.as_deref(), Some(difficulty));
            let expected_boss_encounter_id = format!("scene.{scene_id}.boss");
            assert_eq!(
                rule.boss_encounter_id.as_deref(),
                Some(expected_boss_encounter_id.as_str())
            );
            assert_eq!(
                rule.boss_monster_ids.iter().copied().collect::<Vec<_>>(),
                vec![80_015, 80_016, 80_017, 80_018, 80_019]
            );
        }
        assert!(!config.scene_rules.contains_key(&12_021));
        assert!(!config.scene_rules.contains_key(&12_024));
    }

    #[test]
    fn mech_facility_rule_preserves_master_tiers_and_reviewed_boundaries() {
        let config = bundled_run_reducer_config().unwrap();
        let rule = config.scene_rules.get(&6525).unwrap();
        assert_eq!(rule.activity_family_id.as_deref(), Some("mech-facility"));
        assert_eq!(rule.difficulty_family.as_deref(), Some("master"));
        assert_eq!(
            rule.difficulty_tier_range,
            Some(rlogs_combat::DifficultyTierRange {
                minimum: 1,
                maximum: 20,
            })
        );
        assert_eq!(
            rule.boss_monster_ids.iter().copied().collect::<Vec<_>>(),
            vec![33_500]
        );
        assert_eq!(
            rule.objective_rules
                .get(&6_521_006)
                .map(|rule| rule.on_complete),
            Some(rlogs_combat::CompletedObjectiveAction::ClearMobbing)
        );
        assert_eq!(
            rule.objective_rules
                .get(&6_521_003)
                .map(|rule| rule.on_complete),
            Some(rlogs_combat::CompletedObjectiveAction::FinalObjective)
        );
    }

    #[test]
    fn sea_ringed_reef_rule_preserves_master_tiers_and_reviewed_boundaries() {
        let config = bundled_run_reducer_config().unwrap();
        let rule = config.scene_rules.get(&6565).unwrap();
        assert_eq!(rule.activity_family_id.as_deref(), Some("sea-ringed-reef"));
        assert_eq!(rule.difficulty_family.as_deref(), Some("master"));
        assert_eq!(
            rule.difficulty_tier_range,
            Some(rlogs_combat::DifficultyTierRange {
                minimum: 1,
                maximum: 20,
            })
        );
        assert_eq!(
            rule.boss_monster_ids.iter().copied().collect::<Vec<_>>(),
            vec![4601]
        );
        assert_eq!(
            rule.objective_rules
                .get(&6_561_003)
                .map(|rule| rule.on_complete),
            Some(rlogs_combat::CompletedObjectiveAction::ClearMobbing)
        );
        assert_eq!(
            rule.objective_rules
                .get(&6_561_011)
                .map(|rule| rule.on_complete),
            Some(rlogs_combat::CompletedObjectiveAction::EnterBossSegment)
        );
        assert_eq!(
            rule.objective_rules
                .get(&6_561_012)
                .map(|rule| rule.on_complete),
            Some(rlogs_combat::CompletedObjectiveAction::FinalObjective)
        );
    }

    #[test]
    fn every_reviewed_master_dungeon_accepts_packet_tiers_one_through_twenty() {
        let config = bundled_run_reducer_config().unwrap();
        let expected_scenes = BTreeSet::from([
            1033, 1123, 1150, 1223, 1235, 1333, 1423, 1533, 1633, 6009, 6023, 6123, 6223, 6333,
            6423, 6515, 6525, 6545, 6565,
        ]);
        let actual_scenes = config
            .scene_rules
            .iter()
            .filter_map(|(scene_id, rule)| {
                (rule.difficulty_family.as_deref() == Some("master")).then_some(*scene_id)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(actual_scenes, expected_scenes);
        for scene_id in expected_scenes {
            let rule = config.scene_rules.get(&scene_id).unwrap();
            assert_eq!(
                rule.difficulty_tier_range,
                Some(DifficultyTierRange {
                    minimum: 1,
                    maximum: 20,
                }),
                "scene {scene_id} must accept every exact packet tier"
            );
        }
    }

    #[test]
    fn an_identical_master_dungeon_may_return_in_a_later_season() {
        let rule = SceneRunRule {
            scene_id: 6515,
            runtime_enabled: true,
            activity_kind: ActivityKind::Dungeon,
            activity_id: "scene.6515".to_owned(),
            activity_family_id: Some("dungeon.6515".to_owned()),
            activity_localization_key: Some("scene.6515.name".to_owned()),
            difficulty_family: Some("master".to_owned()),
            difficulty_localization_key: Some("difficulty.master.label_format".to_owned()),
            difficulty_tier_range: Some(DifficultyTierRange {
                minimum: 1,
                maximum: 20,
            }),
            route_id: None,
            raid_route_kind: None,
            partition: None,
            candidate_dungeon_ids: BTreeSet::from([6515]),
            mobbing_encounter_id: None,
            boss_encounter_id: None,
            boss_monster_ids: BTreeSet::new(),
            objective_rules: BTreeMap::new(),
            evidence: vec![RunRuleEvidence {
                source: "reviewed-dungeon-season-catalog".to_owned(),
                reference: "season-3.json:scene-6515".to_owned(),
                confidence: RunRuleConfidence::Verified,
            }],
        };
        let mut later_season = rule.clone();
        later_season.evidence[0].reference = "season-5.json:scene-6515".to_owned();

        assert!(same_generated_master_identity(&rule, &later_season));
    }

    #[test]
    fn cursed_radiant_tomb_uses_generated_master_identity_without_guessed_boundaries() {
        let config = bundled_run_reducer_config().unwrap();
        let rule = config.scene_rules.get(&6515).unwrap();
        assert_eq!(rule.activity_id, "scene.6515");
        assert_eq!(rule.activity_family_id.as_deref(), Some("dungeon.6515"));
        assert_eq!(rule.difficulty_family.as_deref(), Some("master"));
        assert_eq!(
            rule.difficulty_tier_range,
            Some(DifficultyTierRange {
                minimum: 1,
                maximum: 20,
            })
        );
        assert!(rule.boss_monster_ids.is_empty());
        assert!(rule.objective_rules.is_empty());
    }
}
