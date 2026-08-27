//! Build-scoped standard Attack/MAttack coefficient lookup.
//!
//! The generated catalog contains only unique `(ability, hit_event)` keys.
//! Ambiguous keys and nonstandard damage scripts remain canonical events but
//! are deliberately ineligible for live rDPS projection.

use std::{collections::HashMap, sync::OnceLock};

use serde::Deserialize;

use crate::rdps_runtime::rdps_runtime_config;

const DAMAGE_STAGE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OffensiveStatKind {
    PhysicalAttack,
    MagicalAttack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectedDamageStage {
    pub damage_attr_id: i64,
    pub offensive_stat: OffensiveStatKind,
    pub coefficient_basis_points: i64,
    pub fixed_parameter: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeSource {
    table: String,
    table_hash: i64,
    row_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimePolicy {
    lookup_key: String,
    ambiguous_keys: String,
    nonstandard_scripts: String,
    coefficient_selection: String,
    fixed_parameter_selection: String,
    unresolved_events: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeSummary {
    source_rows: usize,
    unique_lookup_keys: usize,
    ambiguous_lookup_keys: usize,
    standard_attack_rules: usize,
    standard_magic_attack_rules: usize,
    standard_rules: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeDamageStageRule {
    ability_id: i64,
    hit_event_id: i32,
    #[serde(default)]
    damage_source: Option<i32>,
    damage_attr_id: i64,
    damage_script: String,
    coefficient_basis_points_by_stage: Vec<i64>,
    fixed_parameter_by_level: Vec<i64>,
}

impl RuntimeDamageStageRule {
    fn offensive_stat(&self) -> Option<OffensiveStatKind> {
        match self.damage_script.as_str() {
            "Attack" => Some(OffensiveStatKind::PhysicalAttack),
            "MAttack" => Some(OffensiveStatKind::MagicalAttack),
            _ => None,
        }
    }

    fn select(
        &self,
        owner_stage: Option<i32>,
        owner_level: Option<i32>,
    ) -> Option<SelectedDamageStage> {
        let stage = owner_stage.unwrap_or_default();
        let coefficient_basis_points = if self.coefficient_basis_points_by_stage.len() == 1 {
            (stage >= 0).then_some(self.coefficient_basis_points_by_stage[0])?
        } else {
            let index = usize::try_from(stage).ok()?;
            *self.coefficient_basis_points_by_stage.get(index)?
        };
        if coefficient_basis_points <= 0 {
            return None;
        }

        let fixed_parameter = if self.fixed_parameter_by_level.is_empty() {
            0
        } else {
            let level = usize::try_from(owner_level?).ok()?;
            *self.fixed_parameter_by_level.get(level.checked_sub(1)?)?
        };
        Some(SelectedDamageStage {
            damage_attr_id: self.damage_attr_id,
            offensive_stat: self.offensive_stat()?,
            coefficient_basis_points,
            fixed_parameter,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeCatalogFile {
    schema_version: u16,
    game_build: String,
    generated_by: String,
    source: RuntimeSource,
    policy: RuntimePolicy,
    summary: RuntimeSummary,
    rules: Vec<RuntimeDamageStageRule>,
}

#[derive(Debug)]
struct RuntimeCatalog {
    rules: HashMap<(i64, i32, Option<i32>), RuntimeDamageStageRule>,
}

static DAMAGE_STAGE_CATALOG: OnceLock<Result<RuntimeCatalog, String>> = OnceLock::new();

fn damage_stage_catalog() -> Result<&'static RuntimeCatalog, String> {
    DAMAGE_STAGE_CATALOG
        .get_or_init(|| {
            let file: RuntimeCatalogFile = serde_json::from_str(include_str!(
                "../game-data/runtime/damage-stage-rdps.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR damage-stage catalog is invalid: {error}"))?;
            let runtime = rdps_runtime_config()?;
            let expected = &runtime.damage_stage_catalog;
            if file.schema_version != DAMAGE_STAGE_SCHEMA_VERSION
                || file.game_build != expected.authored_game_build
                || file.generated_by != "rlogs-bpsr-damage-stage-runtime-catalog"
                || file.source.table != expected.source_table
                || file.source.table_hash != expected.source_table_hash
                || file.source.row_count != expected.source_row_count
                || file.summary.source_rows != expected.source_row_count
                || file.summary.unique_lookup_keys != expected.unique_lookup_keys
                || file.summary.ambiguous_lookup_keys != expected.ambiguous_lookup_keys
                || file.summary.standard_attack_rules != expected.standard_attack_rules
                || file.summary.standard_magic_attack_rules != expected.standard_magic_attack_rules
                || file.summary.standard_rules != expected.standard_rules
                || file.rules.len() != expected.standard_rules
                || file.policy.lookup_key.is_empty()
                || file.policy.ambiguous_keys.is_empty()
                || file.policy.nonstandard_scripts.is_empty()
                || file.policy.coefficient_selection.is_empty()
                || file.policy.fixed_parameter_selection.is_empty()
                || file.policy.unresolved_events.is_empty()
            {
                return Err("bundled BPSR damage-stage catalog has an unsupported shape".into());
            }

            let mut rules = HashMap::with_capacity(file.rules.len());
            for rule in file.rules {
                if rule.ability_id <= 0
                    || rule.hit_event_id < 0
                    || rule.damage_attr_id <= 0
                    || rule.offensive_stat().is_none()
                    || rule
                        .coefficient_basis_points_by_stage
                        .iter()
                        .any(|value| *value < 0)
                {
                    return Err("bundled BPSR damage-stage rule contains an invalid value".into());
                }
                let key = (rule.ability_id, rule.hit_event_id, rule.damage_source);
                if rules.insert(key, rule).is_some() {
                    return Err("bundled BPSR damage-stage catalog contains a duplicate key".into());
                }
            }
            Ok(RuntimeCatalog { rules })
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn select_damage_stage(
    ability_id: i64,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    owner_stage: Option<i32>,
    owner_level: Option<i32>,
) -> Option<SelectedDamageStage> {
    let catalog = damage_stage_catalog().ok()?;
    let hit_event_id = hit_event_id.unwrap_or_default();
    catalog
        .rules
        .get(&(ability_id, hit_event_id, damage_source))
        .or_else(|| {
            damage_source
                .is_some()
                .then(|| catalog.rules.get(&(ability_id, hit_event_id, None)))
                .flatten()
        })?
        .select(owner_stage, owner_level)
}

pub(crate) fn validate_damage_stage_catalog() -> Result<(), String> {
    damage_stage_catalog().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_catalog_is_exactly_current_build_and_unique() {
        let catalog = damage_stage_catalog().unwrap();
        assert_eq!(
            catalog.rules.len(),
            rdps_runtime_config()
                .unwrap()
                .damage_stage_catalog
                .standard_rules
        );
    }

    #[test]
    fn packet_stage_and_level_select_current_build_values() {
        assert_eq!(
            select_damage_stage(2_203_291, Some(7), None, Some(1), Some(2)),
            Some(SelectedDamageStage {
                damage_attr_id: 2_220_329_107,
                offensive_stat: OffensiveStatKind::PhysicalAttack,
                coefficient_basis_points: 15_600,
                fixed_parameter: 35,
            })
        );
        assert_eq!(
            select_damage_stage(2_203_291, Some(7), None, None, Some(1))
                .map(|stage| stage.coefficient_basis_points),
            Some(15_000)
        );
    }

    #[test]
    fn missing_packet_level_or_out_of_range_stage_never_guesses() {
        assert_eq!(
            select_damage_stage(2_203_291, Some(7), None, Some(1), None),
            None
        );
        assert_eq!(
            select_damage_stage(2_203_291, Some(7), None, Some(99), Some(2)),
            None
        );
        assert_eq!(
            select_damage_stage(6_701, Some(999), None, Some(0), Some(1)),
            None
        );
    }

    #[test]
    fn source_disambiguated_rules_require_the_exact_numeric_damage_source() {
        assert_eq!(
            select_damage_stage(920_201, Some(1), Some(0), None, None)
                .map(|stage| (stage.damage_attr_id, stage.coefficient_basis_points)),
            Some((19_202_010_101, 5_000))
        );
        assert_eq!(
            select_damage_stage(920_201, Some(1), Some(1), None, None)
                .map(|stage| (stage.damage_attr_id, stage.coefficient_basis_points)),
            Some((392_020_101, 10_000))
        );
        assert_eq!(
            select_damage_stage(920_201, Some(1), None, None, None),
            None
        );
        assert!(select_damage_stage(2_203_291, Some(7), Some(99), Some(1), Some(2)).is_some());
    }
}
