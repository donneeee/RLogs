use std::collections::{BTreeMap, BTreeSet};

use rlogs_bpsr_module_optimizer::ModuleCandidate;
use serde::{Deserialize, Serialize};

use crate::profile_packages::LocalProfilePackageStore;

const MODULE_OPTIMIZER_INVENTORY_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_MODULES_PER_CHARACTER: usize = 4_096;

#[derive(Clone, Debug, Serialize)]
pub struct LocalModuleInventoryView {
    pub schema_version: u16,
    pub characters: Vec<LocalModuleCharacterView>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalModuleCharacterView {
    pub package_id: String,
    pub character_id: String,
    pub display_name: Option<String>,
    pub deployment: String,
    pub region: String,
    pub source_client_build: String,
    pub observed_unix_millis: u64,
    pub modules: Vec<ModuleCandidate>,
    pub current_instance_ids: Vec<String>,
    pub module_snapshot_available: bool,
    pub module_snapshot_detail: String,
}

#[derive(Debug, Deserialize)]
struct ProfileModules {
    #[serde(default)]
    inventory: Vec<ModuleCandidate>,
    #[serde(default)]
    equipped_slots: BTreeMap<String, String>,
}

pub fn load_local_module_inventories(
    store: &mut LocalProfilePackageStore,
) -> Result<LocalModuleInventoryView, String> {
    store.reload()?;
    let snapshot = store.snapshot();
    let mut issues = snapshot.issues;
    let mut characters = Vec::new();
    for entry in snapshot.entries {
        if entry.game_plugin_id != rlogs_game_bpsr::BPSR_GAME_PLUGIN_ID {
            continue;
        }
        let inspection = match store.inspect(&entry.package_id) {
            Ok(inspection) => inspection,
            Err(error) => {
                issues.push(format!(
                    "Could not inspect the local profile for {}: {error}",
                    entry.character_id
                ));
                continue;
            }
        };
        let modules_value = inspection
            .package
            .request
            .payload
            .body
            .get("modules")
            .cloned();
        let (modules, current_instance_ids, module_snapshot_available, detail) =
            match modules_value {
                Some(value) if !value.is_null() => match decode_modules(value) {
                    Ok((modules, current_instance_ids)) if !modules.is_empty() => {
                        let detail = format!(
                            "{} owned module{} · {} equipped",
                            modules.len(),
                            if modules.len() == 1 { "" } else { "s" },
                            current_instance_ids.len()
                        );
                        (modules, current_instance_ids, true, detail)
                    }
                    Ok((modules, current_instance_ids)) => (
                        modules,
                        current_instance_ids,
                        false,
                        "The latest live profile snapshot contains an empty module inventory."
                            .into(),
                    ),
                    Err(error) => {
                        issues.push(format!(
                            "Module snapshot for {} is invalid: {error}",
                            entry.character_id
                        ));
                        (
                            Vec::new(),
                            Vec::new(),
                            false,
                            format!("The latest module snapshot could not be used: {error}"),
                        )
                    }
                },
                _ => (
                    Vec::new(),
                    Vec::new(),
                    false,
                    "No module inventory has been observed for this character yet. Keep the game open and let rLogs capture a refreshed character profile."
                        .into(),
                ),
            };
        characters.push(LocalModuleCharacterView {
            package_id: entry.package_id,
            character_id: entry.character_id,
            display_name: entry.display_name,
            deployment: entry.deployment,
            region: entry.region,
            source_client_build: entry.source_client_build,
            observed_unix_millis: entry.created_unix_millis,
            modules,
            current_instance_ids,
            module_snapshot_available,
            module_snapshot_detail: detail,
        });
    }
    Ok(LocalModuleInventoryView {
        schema_version: MODULE_OPTIMIZER_INVENTORY_SCHEMA_VERSION,
        characters,
        issues,
    })
}

fn decode_modules(value: serde_json::Value) -> Result<(Vec<ModuleCandidate>, Vec<String>), String> {
    let modules: ProfileModules = serde_json::from_value(value)
        .map_err(|error| format!("profile modules do not match the supported schema: {error}"))?;
    if modules.inventory.len() > MAXIMUM_MODULES_PER_CHARACTER {
        return Err(format!(
            "inventory exceeds the {MAXIMUM_MODULES_PER_CHARACTER}-module safety limit"
        ));
    }
    let mut inventory_ids = BTreeSet::new();
    for (index, module) in modules.inventory.iter().enumerate() {
        if module.instance_id.trim().is_empty() {
            return Err(format!("module {} has an empty instance ID", index + 1));
        }
        if !inventory_ids.insert(module.instance_id.clone()) {
            return Err(format!(
                "module instance ID {} appears more than once",
                module.instance_id
            ));
        }
    }
    let mut equipped = modules
        .equipped_slots
        .into_iter()
        .map(|(slot, instance_id)| {
            let slot = slot
                .parse::<u32>()
                .map_err(|_| format!("equipped module slot {slot} is not numeric"))?;
            if instance_id.trim().is_empty() {
                return Err(format!(
                    "equipped module slot {slot} has an empty instance ID"
                ));
            }
            if !inventory_ids.contains(&instance_id) {
                return Err(format!(
                    "equipped module {instance_id} in slot {slot} is missing from inventory"
                ));
            }
            Ok((slot, instance_id))
        })
        .collect::<Result<Vec<_>, String>>()?;
    equipped.sort_by_key(|(slot, _)| *slot);
    Ok((
        modules.inventory,
        equipped
            .into_iter()
            .map(|(_, instance_id)| instance_id)
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_snapshot_preserves_large_string_ids_and_slot_order() {
        let (modules, current) = decode_modules(serde_json::json!({
            "inventory": [
                {"instance_id":"9007199254740993","config_id":5500104,"quality":4,"parts":[{"part_id":1110,"initial_link_points":20}]},
                {"instance_id":"9007199254740995","config_id":5500204,"quality":4,"parts":[{"part_id":2406,"initial_link_points":16}]}
            ],
            "equipped_slots": {"2":"9007199254740995","1":"9007199254740993"}
        }))
        .expect("valid module snapshot");
        assert_eq!(modules.len(), 2);
        assert_eq!(
            current,
            ["9007199254740993".to_owned(), "9007199254740995".to_owned()]
        );
    }

    #[test]
    fn module_snapshot_rejects_an_equipped_id_missing_from_inventory() {
        let error = decode_modules(serde_json::json!({
            "inventory": [{"instance_id":"1","config_id":5500101,"parts":[]}],
            "equipped_slots": {"1":"2"}
        }))
        .expect_err("missing equipped module must fail");
        assert!(error.contains("missing from inventory"));
    }
}
