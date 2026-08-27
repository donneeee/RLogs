use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ModuleProfile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleEffectCatalog {
    pub catalog_revision: String,
    effects: BTreeMap<i32, ModuleEffectDefinition>,
    module_effect_ids: BTreeMap<i32, BTreeSet<i32>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModuleEffectDefinition {
    id: i32,
    levels: Vec<ModuleEffectLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleEffectLevel {
    pub level: i32,
    pub required_link_points: i32,
    pub effect_config_records: Vec<Vec<i64>>,
    pub effect_keys: Vec<Vec<String>>,
    pub effect_values: Vec<Vec<i64>>,
    pub fight_value: i32,
}

impl ModuleEffectLevel {
    /// Runtime status-effect configuration IDs referenced by exact-build
    /// `ModEffectTable` effect-config records of kind 3.
    pub fn runtime_status_effect_ids(&self) -> Vec<i64> {
        self.effect_config_records
            .iter()
            .filter_map(|record| {
                (record.first() == Some(&3))
                    .then(|| record.get(1).copied())
                    .flatten()
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveModuleEffectSnapshot {
    pub catalog_revision: String,
    pub effects: Vec<ActiveModuleEffect>,
    pub issues: Vec<ModuleEffectResolutionIssue>,
}

impl ActiveModuleEffectSnapshot {
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn effect(&self, effect_id: i32) -> Option<&ActiveModuleEffect> {
        self.effects
            .binary_search_by_key(&effect_id, |effect| effect.effect_id)
            .ok()
            .map(|index| &self.effects[index])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveModuleEffect {
    pub effect_id: i32,
    pub total_link_points: i32,
    pub active_level: ModuleEffectLevel,
    pub sources: Vec<ModuleEffectSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleEffectSource {
    pub equipped_slot: i32,
    pub module_config_id: i32,
    pub link_points: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModuleEffectResolutionIssue {
    DuplicateInventoryInstance {
        instance_id: String,
    },
    InstanceEquippedMoreThanOnce {
        instance_id: String,
        first_slot: i32,
        duplicate_slot: i32,
    },
    MissingEquippedModule {
        equipped_slot: i32,
        instance_id: String,
    },
    MissingModuleDefinition {
        equipped_slot: i32,
        module_config_id: i32,
    },
    EffectNotAllowedForModule {
        equipped_slot: i32,
        module_config_id: i32,
        effect_id: i32,
    },
    MissingLinkPoints {
        equipped_slot: i32,
        module_config_id: i32,
        effect_id: i32,
    },
    NegativeLinkPoints {
        equipped_slot: i32,
        module_config_id: i32,
        effect_id: i32,
        link_points: i32,
    },
    MissingEffectDefinition {
        effect_id: i32,
        total_link_points: i32,
    },
    NoUnlockedLevel {
        effect_id: i32,
        total_link_points: i32,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ModuleEffectCatalogError {
    #[error("failed to read module-effect catalog file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to decode module-effect catalog file {path}: {source}")]
    Decode {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid module-effect catalog: {0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize)]
struct CatalogManifest {
    catalog_revision: String,
}

#[derive(Debug, Deserialize)]
struct ModuleEffectRecord {
    id: i32,
    attributes: ModuleEffectAttributes,
}

#[derive(Debug, Deserialize)]
struct ModuleEffectAttributes {
    levels: Vec<ModuleEffectLevel>,
}

#[derive(Debug, Deserialize)]
struct ModuleRecord {
    id: i32,
    attributes: ModuleAttributes,
}

#[derive(Debug, Deserialize)]
struct ModuleAttributes {
    effect_ids: Vec<i32>,
}

impl ModuleEffectCatalog {
    pub fn load_from_path(catalog_root: &Path) -> Result<Self, ModuleEffectCatalogError> {
        let manifest: CatalogManifest = read_json(&catalog_root.join("manifest.json"))?;
        let mut effects = BTreeMap::new();
        for path in json_files_recursive(&catalog_root.join("module-effects"))? {
            let mut record: ModuleEffectRecord = read_json(&path)?;
            record
                .attributes
                .levels
                .sort_by_key(|level| (level.required_link_points, level.level));
            validate_levels(record.id, &record.attributes.levels)?;
            if effects
                .insert(
                    record.id,
                    ModuleEffectDefinition {
                        id: record.id,
                        levels: record.attributes.levels,
                    },
                )
                .is_some()
            {
                return Err(ModuleEffectCatalogError::Invalid(format!(
                    "duplicate module-effect ID {}",
                    record.id
                )));
            }
        }

        let mut module_effect_ids = BTreeMap::new();
        for path in json_files_recursive(&catalog_root.join("modules"))? {
            let record: ModuleRecord = read_json(&path)?;
            if module_effect_ids
                .insert(
                    record.id,
                    record.attributes.effect_ids.into_iter().collect(),
                )
                .is_some()
            {
                return Err(ModuleEffectCatalogError::Invalid(format!(
                    "duplicate module ID {}",
                    record.id
                )));
            }
        }

        if effects.is_empty() || module_effect_ids.is_empty() {
            return Err(ModuleEffectCatalogError::Invalid(
                "module-effect or module definitions are empty".into(),
            ));
        }

        Ok(Self {
            catalog_revision: manifest.catalog_revision,
            effects,
            module_effect_ids,
        })
    }

    /// Resolve one immutable packet-captured module profile.
    ///
    /// `initial_link_points` is the current per-part value carried by the
    /// snapshot. `upgrade_records` is deliberately not added: in the verified
    /// current-build capture, all 1,937 part values already equalled their
    /// successful upgrade-record counts. Adding both would double-count every
    /// observed part.
    pub fn resolve(&self, profile: &ModuleProfile) -> ActiveModuleEffectSnapshot {
        let mut inventory = BTreeMap::new();
        let mut issues = Vec::new();
        for module in &profile.inventory {
            if inventory
                .insert(module.instance_id.as_str(), module)
                .is_some()
            {
                issues.push(ModuleEffectResolutionIssue::DuplicateInventoryInstance {
                    instance_id: module.instance_id.clone(),
                });
            }
        }

        let mut equipped_instances = BTreeMap::<&str, i32>::new();
        let mut sources = BTreeMap::<i32, Vec<ModuleEffectSource>>::new();
        let mut blocked_effect_ids = BTreeSet::new();

        for (&slot, instance_id) in &profile.equipped_slots {
            if let Some(first_slot) = equipped_instances.insert(instance_id, slot) {
                issues.push(ModuleEffectResolutionIssue::InstanceEquippedMoreThanOnce {
                    instance_id: instance_id.clone(),
                    first_slot,
                    duplicate_slot: slot,
                });
                continue;
            }
            let Some(module) = inventory.get(instance_id.as_str()).copied() else {
                issues.push(ModuleEffectResolutionIssue::MissingEquippedModule {
                    equipped_slot: slot,
                    instance_id: instance_id.clone(),
                });
                continue;
            };
            let Some(allowed_effect_ids) = self.module_effect_ids.get(&module.config_id) else {
                issues.push(ModuleEffectResolutionIssue::MissingModuleDefinition {
                    equipped_slot: slot,
                    module_config_id: module.config_id,
                });
                for part in &module.parts {
                    blocked_effect_ids.insert(part.part_id);
                }
                continue;
            };

            for part in &module.parts {
                if !allowed_effect_ids.contains(&part.part_id) {
                    blocked_effect_ids.insert(part.part_id);
                    issues.push(ModuleEffectResolutionIssue::EffectNotAllowedForModule {
                        equipped_slot: slot,
                        module_config_id: module.config_id,
                        effect_id: part.part_id,
                    });
                    continue;
                }
                let Some(link_points) = part.initial_link_points else {
                    blocked_effect_ids.insert(part.part_id);
                    issues.push(ModuleEffectResolutionIssue::MissingLinkPoints {
                        equipped_slot: slot,
                        module_config_id: module.config_id,
                        effect_id: part.part_id,
                    });
                    continue;
                };
                if link_points < 0 {
                    blocked_effect_ids.insert(part.part_id);
                    issues.push(ModuleEffectResolutionIssue::NegativeLinkPoints {
                        equipped_slot: slot,
                        module_config_id: module.config_id,
                        effect_id: part.part_id,
                        link_points,
                    });
                    continue;
                }
                sources
                    .entry(part.part_id)
                    .or_default()
                    .push(ModuleEffectSource {
                        equipped_slot: slot,
                        module_config_id: module.config_id,
                        link_points,
                    });
            }
        }

        let mut effects = Vec::new();
        for (effect_id, mut effect_sources) in sources {
            if blocked_effect_ids.contains(&effect_id) {
                continue;
            }
            effect_sources.sort_by_key(|source| source.equipped_slot);
            let total_link_points = effect_sources.iter().map(|source| source.link_points).sum();
            let Some(definition) = self.effects.get(&effect_id) else {
                issues.push(ModuleEffectResolutionIssue::MissingEffectDefinition {
                    effect_id,
                    total_link_points,
                });
                continue;
            };
            debug_assert_eq!(definition.id, effect_id);
            let Some(active_level) = definition
                .levels
                .iter()
                .rfind(|level| total_link_points >= level.required_link_points)
                .cloned()
            else {
                issues.push(ModuleEffectResolutionIssue::NoUnlockedLevel {
                    effect_id,
                    total_link_points,
                });
                continue;
            };
            effects.push(ActiveModuleEffect {
                effect_id,
                total_link_points,
                active_level,
                sources: effect_sources,
            });
        }
        effects.sort_by_key(|effect| effect.effect_id);

        ActiveModuleEffectSnapshot {
            catalog_revision: self.catalog_revision.clone(),
            effects,
            issues,
        }
    }
}

fn validate_levels(
    effect_id: i32,
    levels: &[ModuleEffectLevel],
) -> Result<(), ModuleEffectCatalogError> {
    if levels.is_empty() {
        return Err(ModuleEffectCatalogError::Invalid(format!(
            "module effect {effect_id} has no levels"
        )));
    }
    if levels.windows(2).any(|pair| {
        pair[0].required_link_points >= pair[1].required_link_points
            || pair[0].level >= pair[1].level
    }) {
        return Err(ModuleEffectCatalogError::Invalid(format!(
            "module effect {effect_id} levels are not strictly increasing"
        )));
    }
    Ok(())
}

fn read_json<T>(path: &Path) -> Result<T, ModuleEffectCatalogError>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).map_err(|source| ModuleEffectCatalogError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ModuleEffectCatalogError::Decode {
        path: path.to_path_buf(),
        source,
    })
}

fn json_files_recursive(root: &Path) -> Result<Vec<PathBuf>, ModuleEffectCatalogError> {
    fn visit(root: &Path, paths: &mut Vec<PathBuf>) -> Result<(), ModuleEffectCatalogError> {
        let entries = fs::read_dir(root).map_err(|source| ModuleEffectCatalogError::Read {
            path: root.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| ModuleEffectCatalogError::Read {
                path: root.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            if path.is_dir() {
                visit(&path, paths)?;
            } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
                paths.push(path);
            }
        }
        Ok(())
    }

    let mut paths = Vec::new();
    visit(root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModuleItemProfile, ModulePartProfile, ModuleUpgradeRecord};

    fn catalog() -> ModuleEffectCatalog {
        ModuleEffectCatalog::load_from_path(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("game-data/catalog"),
        )
        .unwrap()
    }

    fn module(instance_id: &str, config_id: i32, parts: &[(i32, i32)]) -> ModuleItemProfile {
        ModuleItemProfile {
            instance_id: instance_id.into(),
            config_id,
            count: Some(1),
            quality: Some(3),
            load_flag: Some(1),
            module_type: None,
            level: Some(60),
            parts: parts
                .iter()
                .map(|(part_id, value)| ModulePartProfile {
                    part_id: *part_id,
                    initial_link_points: Some(*value),
                })
                .collect(),
            // These records mirror the current value and must not be added.
            upgrade_records: parts
                .iter()
                .flat_map(|(part_id, value)| {
                    (0..*value).map(|_| ModuleUpgradeRecord {
                        part_id: *part_id,
                        succeeded: Some(true),
                    })
                })
                .collect(),
            success_rate: Some(10000),
        }
    }

    #[test]
    fn exact_captured_loadout_sums_effect_families_and_selects_thresholds() {
        let profile = ModuleProfile {
            equipped_slots: BTreeMap::from([
                (1, "a".into()),
                (2, "b".into()),
                (3, "c".into()),
                (4, "d".into()),
                (5, "e".into()),
            ]),
            inventory: vec![
                module("a", 5500103, &[(2104, 10), (1110, 2), (1111, 3)]),
                module("b", 5500103, &[(2104, 10), (1408, 3), (1410, 4)]),
                module("c", 5500103, &[(2404, 9), (1409, 5), (1407, 1)]),
                module("d", 5500303, &[(2404, 7), (1308, 6), (1408, 5)]),
                module("e", 5500103, &[(2404, 4), (1408, 9), (1111, 3)]),
            ],
        };

        let snapshot = catalog().resolve(&profile);
        assert!(snapshot.is_complete(), "{:?}", snapshot.issues);
        let actual = snapshot
            .effects
            .iter()
            .map(|effect| {
                (
                    effect.effect_id,
                    (effect.total_link_points, effect.active_level.level),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual[&2104], (20, 6));
        assert_eq!(actual[&2404], (20, 6));
        assert_eq!(actual[&1408], (17, 5));
        assert_eq!(actual[&1111], (6, 2));
        assert_eq!(actual[&1110], (2, 1));
        assert_eq!(actual[&1410], (4, 2));
        assert_eq!(actual[&1409], (5, 2));
        assert_eq!(actual[&1407], (1, 1));
        assert_eq!(actual[&1308], (6, 2));
    }

    #[test]
    fn team_luck_and_crit_resolves_exact_runtime_status_and_values() {
        let profile = ModuleProfile {
            equipped_slots: BTreeMap::from([(1, "a".into()), (2, "b".into())]),
            inventory: vec![
                module("a", 5500202, &[(2406, 10)]),
                module("b", 5500202, &[(2406, 10)]),
            ],
        };

        let snapshot = catalog().resolve(&profile);
        let effect = snapshot.effect(2406).unwrap();
        assert_eq!(effect.total_link_points, 20);
        assert_eq!(effect.active_level.level, 6);
        assert_eq!(effect.active_level.required_link_points, 20);
        assert_eq!(effect.active_level.effect_values, [vec![520, 340]]);
        assert_eq!(effect.active_level.runtime_status_effect_ids(), [2302120]);
    }

    #[test]
    fn resolving_a_new_snapshot_does_not_mutate_an_older_result() {
        let catalog = catalog();
        let old = catalog.resolve(&ModuleProfile {
            equipped_slots: BTreeMap::from([(1, "old".into()), (2, "old-2".into())]),
            inventory: vec![
                module("old", 5500202, &[(2406, 8)]),
                module("old-2", 5500202, &[(2406, 8)]),
            ],
        });
        let new = catalog.resolve(&ModuleProfile {
            equipped_slots: BTreeMap::from([(1, "new".into()), (2, "new-2".into())]),
            inventory: vec![
                module("new", 5500202, &[(2406, 10)]),
                module("new-2", 5500202, &[(2406, 10)]),
            ],
        });

        assert_eq!(old.effect(2406).unwrap().active_level.level, 5);
        assert_eq!(new.effect(2406).unwrap().active_level.level, 6);
        assert_eq!(old.effect(2406).unwrap().active_level.level, 5);
    }
}
