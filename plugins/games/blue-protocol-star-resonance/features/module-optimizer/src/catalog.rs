use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::scoring::ScoringRules;
use crate::types::{AttributeCatalogEntry, OptimizerCatalog, OptimizerError};

const GAME_ID: &str = "blue-protocol-star-resonance";

#[derive(Debug, Deserialize)]
struct CatalogManifest {
    catalog_revision: String,
    #[serde(default)]
    supported_builds: Vec<SupportedBuild>,
}

#[derive(Debug, Deserialize)]
struct SupportedBuild {
    client_build: String,
}

#[derive(Debug, Deserialize)]
struct LocalizationEntry {
    key: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct OptimizerAliases {
    locale: String,
    aliases: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct EffectRecord {
    id: i32,
    localization_key: Option<String>,
    icon: Option<String>,
    attributes: EffectAttributes,
}

#[derive(Debug, Deserialize)]
struct EffectAttributes {
    levels: Vec<EffectLevel>,
}

#[derive(Debug, Deserialize)]
struct EffectLevel {
    required_link_points: i32,
    fight_value: i32,
}

#[derive(Debug, Deserialize)]
struct LinkRecord {
    id: i32,
    attributes: LinkAttributes,
}

#[derive(Debug, Deserialize)]
struct LinkAttributes {
    link_value: i32,
    fight_value: i32,
}

pub fn load_catalog_from_install_root(
    install_root: &Path,
) -> Result<(ScoringRules, OptimizerCatalog), OptimizerError> {
    load_catalog_from_path(
        &install_root
            .join("plugins/games")
            .join(GAME_ID)
            .join("game-data/catalog"),
    )
}

pub fn load_catalog_from_path(
    catalog_root: &Path,
) -> Result<(ScoringRules, OptimizerCatalog), OptimizerError> {
    let manifest_path = catalog_root.join("manifest.json");
    let manifest: CatalogManifest = read_json(&manifest_path)?;
    let localization_path = catalog_root.join("localization/en-US/modules/profile-catalog.json");
    let localization_entries: Vec<LocalizationEntry> = read_json(&localization_path)?;
    let localization = localization_entries
        .into_iter()
        .map(|entry| (entry.key, entry.text))
        .collect::<BTreeMap<_, _>>();
    let game_plugin_root = catalog_root
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            OptimizerError::InvalidCatalog(format!(
                "catalog root {} is not under a game-data directory",
                catalog_root.display()
            ))
        })?;
    let aliases_path = game_plugin_root
        .join("features/module-optimizer/localization/en-US/attribute-aliases.json");
    let optimizer_aliases: OptimizerAliases = read_json(&aliases_path)?;
    if optimizer_aliases.locale != "en-US" {
        return Err(OptimizerError::InvalidCatalog(format!(
            "optimizer aliases use locale {}, expected en-US",
            optimizer_aliases.locale
        )));
    }

    let mut attributes = read_json_directory::<EffectRecord>(&catalog_root.join("module-effects"))?
        .into_iter()
        .map(|effect| {
            let mut levels = effect
                .attributes
                .levels
                .into_iter()
                .filter(|level| level.required_link_points > 0)
                .collect::<Vec<_>>();
            levels.sort_by_key(|level| level.required_link_points);
            let localization_key = effect.localization_key.as_deref();
            let official_name = localization_key
                .and_then(|key| localization.get(key))
                .cloned()
                .unwrap_or_else(|| format!("Module effect {}", effect.id));
            let name = localization_key
                .and_then(|key| optimizer_aliases.aliases.get(key))
                .cloned()
                .unwrap_or_else(|| official_name.clone());
            let official_name = (official_name != name).then_some(official_name);
            AttributeCatalogEntry {
                id: effect.id,
                name,
                official_name,
                icon: effect.icon,
                thresholds: levels
                    .iter()
                    .map(|level| level.required_link_points)
                    .collect(),
                fight_values: levels.iter().map(|level| level.fight_value).collect(),
            }
        })
        .collect::<Vec<_>>();
    attributes.sort_by_key(|entry| entry.id);

    let link_records =
        read_json_directory::<LinkRecord>(&catalog_root.join("module-link-effects"))?;
    let maximum_link_value = link_records
        .iter()
        .map(|record| record.attributes.link_value)
        .max()
        .ok_or_else(|| OptimizerError::InvalidCatalog("module-link-effects is empty".into()))?;
    let mut link_power = vec![None; usize::try_from(maximum_link_value).unwrap_or_default() + 1];
    for record in link_records {
        if record.id != record.attributes.link_value {
            return Err(OptimizerError::InvalidCatalog(format!(
                "module link row {} does not match link value {}",
                record.id, record.attributes.link_value
            )));
        }
        let index = usize::try_from(record.attributes.link_value)
            .map_err(|_| OptimizerError::InvalidCatalog("negative module link value".into()))?;
        link_power[index] = Some(record.attributes.fight_value);
    }
    let link_power = link_power
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            value.ok_or_else(|| {
                OptimizerError::InvalidCatalog(format!(
                    "module link fight value {index} is missing"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let rules = ScoringRules::from_catalog_entries(
        manifest.catalog_revision.clone(),
        &attributes,
        link_power,
    )?;
    let catalog = OptimizerCatalog {
        game_id: GAME_ID.into(),
        catalog_revision: manifest.catalog_revision,
        scoring_revision: rules.scoring_revision().into(),
        client_builds: manifest
            .supported_builds
            .into_iter()
            .map(|build| build.client_build)
            .collect(),
        attributes,
        combination_sizes: vec![4, 5],
        default_max_solutions: 10,
    };
    Ok((rules, catalog))
}

fn read_json<T>(path: &Path) -> Result<T, OptimizerError>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).map_err(|source| OptimizerError::ReadCatalog {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| OptimizerError::DecodeCatalog {
        path: path.to_path_buf(),
        source,
    })
}

fn read_json_directory<T>(root: &Path) -> Result<Vec<T>, OptimizerError>
where
    T: for<'de> Deserialize<'de>,
{
    let entries = fs::read_dir(root).map_err(|source| OptimizerError::ReadCatalog {
        path: root.to_path_buf(),
        source,
    })?;
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<PathBuf>>();
    paths.sort();
    paths
        .iter()
        .map(|path| read_json(path))
        .collect::<Result<Vec<_>, _>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_catalog_reproduces_the_cn_0_2_0_scoring_tables() {
        let catalog_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game-data/catalog");
        let (actual, catalog) = load_catalog_from_path(&catalog_root).unwrap();
        let expected = ScoringRules::cn_0_2_0_fixture();
        assert_eq!(actual.attributes, expected.attributes);
        assert_eq!(actual.link_power, expected.link_power);
        assert_eq!(catalog.attributes.len(), 21);
        assert_eq!(catalog.client_builds, ["24252055"]);
        assert_eq!(
            catalog
                .attributes
                .iter()
                .find(|entry| entry.id == 1112)
                .map(|entry| (entry.name.as_str(), entry.official_name.as_deref())),
            Some(("Intelligence Boost", Some("Intellect Boost")))
        );
        assert_eq!(
            catalog
                .attributes
                .iter()
                .find(|entry| entry.id == 1307)
                .map(|entry| entry.name.as_str()),
            Some("Resistance (Magic)")
        );
    }
}
