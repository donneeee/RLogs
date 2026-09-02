use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ActivityKind, EncounterKind, LeaderboardPartitionKey, RaidRouteKind, RunSegmentKind};

pub const RUN_RULE_CATALOG_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_SCENE_RULES: usize = 4_096;
const MAXIMUM_BOSS_MONSTERS_PER_SCENE: usize = 256;
const MAXIMUM_OBJECTIVES_PER_SCENE: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRuleCatalog {
    pub schema_version: u16,
    pub ruleset_id: String,
    pub ruleset_version: u32,
    pub target: RunRuleTarget,
    pub scenes: Vec<SceneRunRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRuleTarget {
    pub deployment_id: String,
    pub channel: String,
    pub client_build: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneRunRule {
    pub scene_id: i32,
    #[serde(default = "enabled_by_default")]
    pub runtime_enabled: bool,
    pub activity_kind: ActivityKind,
    pub activity_id: String,
    pub activity_family_id: Option<String>,
    pub activity_localization_key: Option<String>,
    pub difficulty_family: Option<String>,
    pub difficulty_localization_key: Option<String>,
    pub difficulty_tier_range: Option<DifficultyTierRange>,
    pub route_id: Option<String>,
    pub raid_route_kind: Option<RaidRouteKind>,
    pub partition: Option<LeaderboardPartitionKey>,
    #[serde(default)]
    pub candidate_dungeon_ids: BTreeSet<i64>,
    pub mobbing_encounter_id: Option<String>,
    pub boss_encounter_id: Option<String>,
    #[serde(default)]
    pub boss_monster_ids: BTreeSet<i64>,
    #[serde(default)]
    pub objective_rules: BTreeMap<i64, DungeonObjectiveRule>,
    #[serde(default)]
    pub evidence: Vec<RunRuleEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifficultyTierRange {
    pub minimum: u32,
    pub maximum: u32,
}

impl SceneRunRule {
    /// A rule with only a boss encounter is a single-encounter boss floor.
    /// Player-versus-monster combat can enter the boss segment even when the
    /// activity uses dynamically generated monster identities.
    pub fn is_boss_only(&self) -> bool {
        self.mobbing_encounter_id.is_none() && self.boss_encounter_id.is_some()
    }

    pub fn encounter_kind(&self, encounter_id: &str) -> Option<EncounterKind> {
        if self.mobbing_encounter_id.as_deref() == Some(encounter_id) {
            Some(EncounterKind::Mobbing)
        } else if self.boss_encounter_id.as_deref() == Some(encounter_id) {
            Some(match self.activity_kind {
                ActivityKind::Dungeon => EncounterKind::Boss,
                ActivityKind::Raid => match self.raid_route_kind {
                    Some(RaidRouteKind::Gauntlet) => EncounterKind::GauntletBoss,
                    Some(RaidRouteKind::SingleBoss | RaidRouteKind::Unknown) | None => {
                        EncounterKind::RaidBoss
                    }
                },
                ActivityKind::Unknown => EncounterKind::Unknown,
            })
        } else {
            None
        }
    }

    pub fn encounter_segment(&self, encounter_id: &str) -> Option<RunSegmentKind> {
        if self.mobbing_encounter_id.as_deref() == Some(encounter_id) {
            Some(RunSegmentKind::Mobbing)
        } else if self.boss_encounter_id.as_deref() == Some(encounter_id) {
            Some(match self.activity_kind {
                ActivityKind::Dungeon => RunSegmentKind::Boss,
                ActivityKind::Raid => match self.raid_route_kind {
                    Some(RaidRouteKind::Gauntlet) => RunSegmentKind::Gauntlet,
                    Some(RaidRouteKind::SingleBoss | RaidRouteKind::Unknown) | None => {
                        RunSegmentKind::RaidBoss
                    }
                },
                ActivityKind::Unknown => RunSegmentKind::Unknown,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DungeonObjectiveRole {
    MobbingCompletion,
    BossPhaseGate,
    RunCompletion,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletedObjectiveAction {
    ClearMobbing,
    EnterBossSegment,
    FinalObjective,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DungeonObjectiveRule {
    pub role: DungeonObjectiveRole,
    pub on_complete: CompletedObjectiveAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunRuleConfidence {
    Verified,
    Corroborated,
    Candidate,
    UserConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRuleEvidence {
    pub source: String,
    pub reference: String,
    pub confidence: RunRuleConfidence,
}

impl RunRuleCatalog {
    pub fn validate(&self) -> Result<(), RunRuleCatalogError> {
        if self.schema_version != RUN_RULE_CATALOG_SCHEMA_VERSION {
            return Err(RunRuleCatalogError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        for (field, value) in [
            ("ruleset_id", self.ruleset_id.as_str()),
            ("target.deployment_id", self.target.deployment_id.as_str()),
            ("target.channel", self.target.channel.as_str()),
            ("target.client_build", self.target.client_build.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(RunRuleCatalogError::EmptyField(field));
            }
        }
        if self.ruleset_version == 0 {
            return Err(RunRuleCatalogError::ZeroRulesetVersion);
        }
        if self.scenes.len() > MAXIMUM_SCENE_RULES {
            return Err(RunRuleCatalogError::TooManyScenes(self.scenes.len()));
        }
        let mut scene_ids = BTreeSet::new();
        for scene in &self.scenes {
            if scene.scene_id <= 0 {
                return Err(RunRuleCatalogError::InvalidSceneId(scene.scene_id));
            }
            if !scene_ids.insert(scene.scene_id) {
                return Err(RunRuleCatalogError::DuplicateSceneId(scene.scene_id));
            }
            if scene.activity_id.trim().is_empty() {
                return Err(RunRuleCatalogError::EmptyActivityId(scene.scene_id));
            }
            if let Some(range) = scene.difficulty_tier_range
                && (range.minimum == 0 || range.maximum < range.minimum)
            {
                return Err(RunRuleCatalogError::InvalidDifficultyTierRange(
                    scene.scene_id,
                ));
            }
            if scene.boss_monster_ids.len() > MAXIMUM_BOSS_MONSTERS_PER_SCENE {
                return Err(RunRuleCatalogError::TooManyBossMonsters(scene.scene_id));
            }
            if scene.candidate_dungeon_ids.iter().any(|id| *id <= 0) {
                return Err(RunRuleCatalogError::InvalidCandidateDungeonId(
                    scene.scene_id,
                ));
            }
            if scene.objective_rules.len() > MAXIMUM_OBJECTIVES_PER_SCENE {
                return Err(RunRuleCatalogError::TooManyObjectives(scene.scene_id));
            }
            if scene.boss_monster_ids.iter().any(|id| *id <= 0) {
                return Err(RunRuleCatalogError::InvalidBossMonsterId(scene.scene_id));
            }
            if scene.objective_rules.keys().any(|id| *id <= 0) {
                return Err(RunRuleCatalogError::InvalidObjectiveId(scene.scene_id));
            }
            if scene
                .objective_rules
                .values()
                .any(|rule| rule.on_complete == CompletedObjectiveAction::ClearMobbing)
                && scene.mobbing_encounter_id.is_none()
            {
                return Err(RunRuleCatalogError::MissingMobbingEncounter(scene.scene_id));
            }
            if scene.runtime_enabled
                && !scene.boss_monster_ids.is_empty()
                && scene.boss_encounter_id.is_none()
            {
                return Err(RunRuleCatalogError::MissingBossEncounter(scene.scene_id));
            }
        }
        Ok(())
    }

    pub fn enabled_scene_rules(&self) -> BTreeMap<i32, SceneRunRule> {
        self.scenes
            .iter()
            .filter(|rule| rule.runtime_enabled)
            .cloned()
            .map(|rule| (rule.scene_id, rule))
            .collect()
    }
}

const fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RunRuleCatalogError {
    #[error("unsupported run-rule catalog schema {0}")]
    UnsupportedSchemaVersion(u16),
    #[error("run-rule catalog field {0} is empty")]
    EmptyField(&'static str),
    #[error("run-rule catalog ruleset_version must be greater than zero")]
    ZeroRulesetVersion,
    #[error("run-rule catalog contains {0} scenes, above the safety limit")]
    TooManyScenes(usize),
    #[error("run-rule catalog contains invalid scene ID {0}")]
    InvalidSceneId(i32),
    #[error("run-rule catalog repeats scene ID {0}")]
    DuplicateSceneId(i32),
    #[error("scene {0} has an empty activity ID")]
    EmptyActivityId(i32),
    #[error("scene {0} has an invalid difficulty tier range")]
    InvalidDifficultyTierRange(i32),
    #[error("scene {0} exceeds the boss-monster safety limit")]
    TooManyBossMonsters(i32),
    #[error("scene {0} exceeds the objective-rule safety limit")]
    TooManyObjectives(i32),
    #[error("scene {0} contains a non-positive boss monster ID")]
    InvalidBossMonsterId(i32),
    #[error("scene {0} contains a non-positive candidate dungeon ID")]
    InvalidCandidateDungeonId(i32),
    #[error("scene {0} contains a non-positive objective ID")]
    InvalidObjectiveId(i32),
    #[error("scene {0} clears mobbing but has no mobbing encounter ID")]
    MissingMobbingEncounter(i32),
    #[error("enabled scene {0} has boss monsters but no boss encounter ID")]
    MissingBossEncounter(i32),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> RunRuleCatalog {
        RunRuleCatalog {
            schema_version: RUN_RULE_CATALOG_SCHEMA_VERSION,
            ruleset_id: "test-rules".into(),
            ruleset_version: 1,
            target: RunRuleTarget {
                deployment_id: "global".into(),
                channel: "steam".into(),
                client_build: "fixture".into(),
            },
            scenes: vec![SceneRunRule {
                scene_id: 1631,
                runtime_enabled: true,
                activity_kind: ActivityKind::Dungeon,
                activity_id: "scene.1631".into(),
                activity_family_id: Some("tina-mindrealm".into()),
                activity_localization_key: Some("scene.1631.name".into()),
                difficulty_family: Some("normal".into()),
                difficulty_localization_key: Some("difficulty.normal".into()),
                difficulty_tier_range: None,
                route_id: None,
                raid_route_kind: None,
                partition: None,
                candidate_dungeon_ids: BTreeSet::from([1_031, 1_631]),
                mobbing_encounter_id: Some("scene.1631.mobbing".into()),
                boss_encounter_id: Some("monster.33701".into()),
                boss_monster_ids: BTreeSet::from([33_701]),
                objective_rules: BTreeMap::from([(
                    100_178,
                    DungeonObjectiveRule {
                        role: DungeonObjectiveRole::MobbingCompletion,
                        on_complete: CompletedObjectiveAction::ClearMobbing,
                    },
                )]),
                evidence: Vec::new(),
            }],
        }
    }

    #[test]
    fn validates_and_indexes_enabled_rules() {
        let rules = catalog();
        rules.validate().unwrap();
        assert_eq!(
            rules
                .enabled_scene_rules()
                .get(&1631)
                .and_then(|rule| rule.difficulty_family.as_deref()),
            Some("normal")
        );
    }

    #[test]
    fn rejects_duplicate_scene_ids() {
        let mut rules = catalog();
        rules.scenes.push(rules.scenes[0].clone());
        assert_eq!(
            rules.validate(),
            Err(RunRuleCatalogError::DuplicateSceneId(1631))
        );
    }
}
