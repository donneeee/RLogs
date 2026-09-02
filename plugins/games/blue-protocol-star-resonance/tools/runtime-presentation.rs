#![allow(clippy::type_complexity)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Deserialize)]
struct MonsterRecord {
    id: i64,
    localization_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SceneRecord {
    id: i64,
    localization_key: Option<String>,
    attributes: SceneAttributes,
}

#[derive(Debug, Deserialize)]
struct SceneAttributes {
    scene_id: i64,
    scene_type: i32,
    scene_subtype: i32,
    parent_scene_id: i64,
    scene_resource_id: i64,
}

#[derive(Debug, Deserialize)]
struct BattleImagineRecord {
    id: i64,
    localization_key: String,
    icon: String,
    attributes: BattleImagineAttributes,
}

#[derive(Debug, Deserialize)]
struct BattleImagineAttributes {
    item_tier: u32,
    rarity_classification: Option<u32>,
    aoyi_skill_id: Option<i64>,
    aoyi_skill_name_localization_key: Option<String>,
    maximum_tier: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SkillRecord {
    id: i64,
    localization_key: Option<String>,
    attributes: SkillAttributes,
}

#[derive(Debug, Deserialize)]
struct SkillAttributes {
    unlock_conditions: Vec<i32>,
}

#[derive(Debug, Deserialize)]
struct LocalizationEntry {
    locale: String,
    key: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct MonsterLocalizationBundle {
    schema_version: u16,
    locale: String,
    monsters: Vec<(i64, String)>,
}

#[derive(Debug, Clone, Serialize)]
struct ScenePresentation {
    scene_id: i64,
    scene_type: i32,
    scene_subtype: i32,
    parent_scene_id: i64,
    scene_resource_id: i64,
}

#[derive(Debug, Serialize)]
struct ScenePresentationBundle {
    schema_version: u16,
    scenes: Vec<ScenePresentation>,
}

#[derive(Debug, Serialize)]
struct SceneLocalizationBundle {
    schema_version: u16,
    locale: String,
    scenes: Vec<(i64, String)>,
}

#[derive(Debug, Clone, Serialize)]
struct BattleImaginePresentation {
    skill_id: i64,
    item_id: i64,
    item_tier: u32,
    rarity: String,
    maximum_tier: u32,
    icon: String,
}

#[derive(Debug, Serialize)]
struct BattleImaginePresentationBundle {
    schema_version: u16,
    imagines: Vec<BattleImaginePresentation>,
}

#[derive(Debug, Serialize)]
struct BattleImagineLocalizationBundle {
    schema_version: u16,
    locale: String,
    imagines: Vec<(i64, String)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AuxiliaryActionPresentation {
    skill_id: i64,
    icon: String,
    action_kind: String,
    maximum_tier: Option<u32>,
    replacement_imagine_skill_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AuxiliaryActionSourceCatalog {
    schema_version: u16,
    slots: Vec<i32>,
    actions: Vec<AuxiliaryActionSource>,
}

#[derive(Debug, Deserialize)]
struct AuxiliaryActionSource {
    skill_id: i64,
    kind: String,
    icon: String,
    replacement_imagine_skill_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AuxiliaryActionPresentationBundle {
    schema_version: u16,
    skills: Vec<AuxiliaryActionPresentation>,
}

#[derive(Debug, Serialize)]
struct AuxiliaryActionLocalizationBundle {
    schema_version: u16,
    locale: String,
    skills: Vec<(i64, String)>,
}

#[derive(Debug, Deserialize)]
struct ReviewedCombatActionSourceCatalog {
    schema_version: u16,
    game_build: String,
    actions: Vec<ReviewedCombatActionSource>,
}

#[derive(Debug, Deserialize)]
struct ReviewedCombatActionSource {
    ability_id: i64,
    kind: String,
    localization_key: String,
    icon: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewedCombatActionPresentation {
    ability_id: i64,
    kind: String,
    resolution: &'static str,
    icon: Option<String>,
    recount_group_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ReviewedCombatActionPresentationBundle {
    schema_version: u16,
    actions: Vec<ReviewedCombatActionPresentation>,
}

#[derive(Debug, Serialize)]
struct ReviewedCombatActionLocalizationBundle {
    schema_version: u16,
    locale: String,
    actions: Vec<(i64, String)>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR runtime presentation build failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1).map(PathBuf::from);
    let catalog_root = arguments.next().ok_or_else(usage)?;
    let output_root = arguments.next().ok_or_else(usage)?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let monster_keys = load_monster_keys(&catalog_root.join("monsters"))?;
    if monster_keys.is_empty() {
        return Err("the reviewed monster catalog is empty".into());
    }
    let (scene_keys, scene_presentation) = load_scenes(&catalog_root.join("scenes"))?;
    if scene_presentation.is_empty() {
        return Err("the reviewed scene catalog is empty".into());
    }
    let localization_root = catalog_root.join("localization");
    let (imagine_keys, imagine_presentation) =
        load_battle_imagines(&catalog_root.join("imagines/battle"))?;
    if imagine_presentation.is_empty() {
        return Err("the reviewed battle-Imagine catalog has no direct skill mappings".into());
    }
    let (auxiliary_keys, auxiliary_presentation) = load_auxiliary_actions(
        &catalog_root.join("skills"),
        &catalog_root.join("loadouts/auxiliary-actions.v1.json"),
    )?;
    if auxiliary_presentation.is_empty() {
        return Err("the reviewed skill catalog has no auxiliary action skills".into());
    }
    let (reviewed_action_keys, reviewed_action_presentation) =
        load_reviewed_combat_actions(&catalog_root.join("combat-actions"))?;
    if reviewed_action_presentation.is_empty() {
        return Err("the reviewed observed combat-action catalog is empty".into());
    }
    let mut locale_directories = fs::read_dir(&localization_root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .collect::<Vec<_>>();
    locale_directories.sort_by_key(|entry| entry.file_name());
    if locale_directories.is_empty() {
        return Err("the reviewed localization catalog has no locales".into());
    }

    fs::create_dir_all(&output_root)?;
    let runtime_root = output_root
        .parent()
        .ok_or("runtime localization output has no parent folder")?;
    let scene_presentation_path = runtime_root.join("scene-presentation.v1.json");
    write_json_atomic(
        &scene_presentation_path,
        &ScenePresentationBundle {
            schema_version: SCHEMA_VERSION,
            scenes: scene_presentation.clone(),
        },
    )?;
    println!(
        "wrote {} reviewed scene mappings to {}",
        scene_presentation.len(),
        scene_presentation_path.display()
    );
    let imagine_presentation_path = runtime_root.join("battle-imagine-presentation.v1.json");
    write_json_atomic(
        &imagine_presentation_path,
        &BattleImaginePresentationBundle {
            schema_version: SCHEMA_VERSION,
            imagines: imagine_presentation.clone(),
        },
    )?;
    println!(
        "wrote {} reviewed battle-Imagine mappings to {}",
        imagine_presentation.len(),
        imagine_presentation_path.display()
    );
    let auxiliary_presentation_path = runtime_root.join("auxiliary-action-presentation.v1.json");
    write_json_atomic(
        &auxiliary_presentation_path,
        &AuxiliaryActionPresentationBundle {
            schema_version: SCHEMA_VERSION,
            skills: auxiliary_presentation.clone(),
        },
    )?;
    println!(
        "wrote {} reviewed auxiliary action mappings to {}",
        auxiliary_presentation.len(),
        auxiliary_presentation_path.display()
    );
    let reviewed_action_presentation_path =
        runtime_root.join("reviewed-combat-action-presentation.v1.json");
    write_json_atomic(
        &reviewed_action_presentation_path,
        &ReviewedCombatActionPresentationBundle {
            schema_version: SCHEMA_VERSION,
            actions: reviewed_action_presentation.clone(),
        },
    )?;
    println!(
        "wrote {} reviewed observed combat-action mappings to {}",
        reviewed_action_presentation.len(),
        reviewed_action_presentation_path.display()
    );
    for locale_directory in locale_directories {
        let locale = locale_directory.file_name().to_string_lossy().into_owned();
        let entries = load_localization_entries(&locale_directory.path().join("monsters"))?;
        let mut monsters = Vec::with_capacity(monster_keys.len());
        for (key, id) in &monster_keys {
            let entry = entries.get(key).ok_or_else(|| {
                format!("locale {locale} is missing reviewed monster localization {key}")
            })?;
            if entry.locale != locale {
                return Err(format!(
                    "localization {} claims locale {} but lives under {locale}",
                    entry.key, entry.locale
                )
                .into());
            }
            if entry.text.trim().is_empty() {
                return Err(format!("localization {} has an empty name", entry.key).into());
            }
            monsters.push((*id, entry.text.clone()));
        }
        monsters.sort_by_key(|(id, _)| *id);
        let bundle = MonsterLocalizationBundle {
            schema_version: SCHEMA_VERSION,
            locale: locale.clone(),
            monsters,
        };
        let directory = output_root.join(&locale);
        fs::create_dir_all(&directory)?;
        let path = directory.join("monster-names.v1.json");
        write_json_atomic(&path, &bundle)?;
        println!(
            "wrote {} reviewed monster names for {locale} to {}",
            bundle.monsters.len(),
            path.display()
        );

        let scene_entries = load_localization_entries(&locale_directory.path().join("scenes"))?;
        let mut scenes = Vec::with_capacity(scene_keys.len());
        for (key, id) in &scene_keys {
            let entry = scene_entries.get(key).ok_or_else(|| {
                format!("locale {locale} is missing reviewed scene localization {key}")
            })?;
            if entry.locale != locale || entry.text.trim().is_empty() {
                return Err(format!(
                    "scene localization {} has an invalid locale or empty name",
                    entry.key
                )
                .into());
            }
            scenes.push((*id, entry.text.clone()));
        }
        scenes.sort_by_key(|(id, _)| *id);
        let scene_path = directory.join("scene-names.v1.json");
        write_json_atomic(
            &scene_path,
            &SceneLocalizationBundle {
                schema_version: SCHEMA_VERSION,
                locale: locale.clone(),
                scenes,
            },
        )?;

        let imagine_entries = load_localization_entries(&locale_directory.path().join("imagines"))?;
        let mut imagines = Vec::with_capacity(imagine_keys.len());
        for (id, keys) in &imagine_keys {
            let entry = keys
                .iter()
                .find_map(|key| imagine_entries.get(key))
                .ok_or_else(|| {
                    format!(
                        "locale {locale} is missing reviewed battle-Imagine localization for item {id}"
                    )
                })?;
            if entry.locale != locale || entry.text.trim().is_empty() {
                return Err(format!(
                    "battle-Imagine localization {} has an invalid locale or empty name",
                    entry.key
                )
                .into());
            }
            imagines.push((*id, entry.text.clone()));
        }
        imagines.sort_by_key(|(id, _)| *id);
        let imagine_path = directory.join("battle-imagine-names.v1.json");
        write_json_atomic(
            &imagine_path,
            &BattleImagineLocalizationBundle {
                schema_version: SCHEMA_VERSION,
                locale: locale.clone(),
                imagines,
            },
        )?;

        let skill_entries = load_localization_entries(&locale_directory.path().join("skills"))?;
        let combat_action_entries =
            load_localization_entries(&locale_directory.path().join("combat-actions"))?;
        let mut auxiliary_skills = Vec::with_capacity(auxiliary_keys.len());
        for (skill_id, key) in &auxiliary_keys {
            let entry = skill_entries.get(key).ok_or_else(|| {
                format!("locale {locale} is missing reviewed auxiliary action localization {key}")
            })?;
            if entry.locale != locale || entry.text.trim().is_empty() {
                return Err(format!(
                    "auxiliary action localization {} has an invalid locale or empty name",
                    entry.key
                )
                .into());
            }
            auxiliary_skills.push((*skill_id, entry.text.clone()));
        }
        auxiliary_skills.sort_by_key(|(id, _)| *id);
        let auxiliary_path = directory.join("auxiliary-action-names.v1.json");
        write_json_atomic(
            &auxiliary_path,
            &AuxiliaryActionLocalizationBundle {
                schema_version: SCHEMA_VERSION,
                locale: locale.clone(),
                skills: auxiliary_skills,
            },
        )?;

        let recount_entries =
            load_localization_entries(&locale_directory.path().join("recount-groups"))?;
        let mut reviewed_actions = Vec::with_capacity(reviewed_action_keys.len());
        for (ability_id, key) in &reviewed_action_keys {
            let entry = if key.starts_with("skill.") {
                skill_entries.get(key)
            } else if key.starts_with("recount_group.") {
                recount_entries.get(key)
            } else if key.starts_with("combat_action.") {
                combat_action_entries.get(key)
            } else {
                None
            }
            .ok_or_else(|| {
                format!("locale {locale} is missing reviewed combat-action localization {key}")
            })?;
            if entry.locale != locale || entry.text.trim().is_empty() {
                return Err(format!(
                    "reviewed combat-action localization {} has an invalid locale or empty name",
                    entry.key
                )
                .into());
            }
            reviewed_actions.push((*ability_id, entry.text.clone()));
        }
        reviewed_actions.sort_by_key(|(id, _)| *id);
        let reviewed_action_path = directory.join("reviewed-combat-action-names.v1.json");
        write_json_atomic(
            &reviewed_action_path,
            &ReviewedCombatActionLocalizationBundle {
                schema_version: SCHEMA_VERSION,
                locale: locale.clone(),
                actions: reviewed_actions,
            },
        )?;
    }
    Ok(())
}

fn load_reviewed_combat_actions(
    source_root: &Path,
) -> Result<
    (BTreeMap<i64, String>, Vec<ReviewedCombatActionPresentation>),
    Box<dyn std::error::Error>,
> {
    let mut keys = BTreeMap::new();
    let mut presentation = BTreeMap::new();
    for source_path in json_files(source_root)? {
        let source: ReviewedCombatActionSourceCatalog =
            serde_json::from_slice(&fs::read(&source_path)?)?;
        if source.schema_version != SCHEMA_VERSION || source.game_build.trim().is_empty() {
            return Err(format!(
                "unsupported reviewed combat-action source catalog in {}",
                source_path.display()
            )
            .into());
        }
        for action in source.actions {
            let valid_key = action.localization_key.starts_with("skill.")
                || action.localization_key.starts_with("recount_group.")
                || action.localization_key.starts_with("combat_action.");
            let recount_group_id = action
                .localization_key
                .strip_prefix("recount_group.")
                .and_then(|remainder| remainder.split('.').next())
                .map(str::parse::<i64>)
                .transpose()
                .map_err(|error| {
                    format!(
                        "reviewed combat action {} has an invalid Recount group ID: {error}",
                        action.ability_id
                    )
                })?;
            if action.ability_id <= 0
                || action.kind.trim().is_empty()
                || !valid_key
                || recount_group_id.is_some_and(|id| id <= 0)
                || action.icon.as_deref().is_some_and(str::is_empty)
            {
                return Err(format!(
                    "invalid reviewed combat action {} in {}",
                    action.ability_id,
                    source_path.display()
                )
                .into());
            }
            if keys
                .insert(action.ability_id, action.localization_key)
                .is_some()
            {
                return Err(
                    format!("duplicate reviewed combat action ID {}", action.ability_id).into(),
                );
            }
            presentation.insert(
                action.ability_id,
                ReviewedCombatActionPresentation {
                    ability_id: action.ability_id,
                    kind: action.kind,
                    resolution: "localized",
                    icon: action.icon,
                    recount_group_id,
                },
            );
        }
    }
    Ok((keys, presentation.into_values().collect()))
}

fn load_auxiliary_actions(
    root: &Path,
    source_path: &Path,
) -> Result<(BTreeMap<i64, String>, Vec<AuxiliaryActionPresentation>), Box<dyn std::error::Error>> {
    let auxiliary_slots = [21, 22, 23, 24];
    let source: AuxiliaryActionSourceCatalog = serde_json::from_slice(&fs::read(source_path)?)?;
    if source.schema_version != SCHEMA_VERSION || source.slots != auxiliary_slots {
        return Err(format!(
            "unsupported auxiliary action source catalog in {}",
            source_path.display()
        )
        .into());
    }
    let mut presentation_by_skill = BTreeMap::new();
    for action in source.actions {
        let kind_is_valid = match action.kind.as_str() {
            "role_skill" => action.replacement_imagine_skill_id.is_none(),
            "role_imagine" => action.replacement_imagine_skill_id.is_some_and(|id| id > 0),
            _ => false,
        };
        if action.skill_id <= 0 || action.icon.trim().is_empty() || !kind_is_valid {
            return Err(format!(
                "invalid auxiliary action {} in {}",
                action.skill_id,
                source_path.display()
            )
            .into());
        }
        let presentation = AuxiliaryActionPresentation {
            skill_id: action.skill_id,
            icon: action.icon,
            maximum_tier: (action.kind == "role_imagine").then_some(4),
            action_kind: action.kind,
            replacement_imagine_skill_id: action.replacement_imagine_skill_id,
        };
        if presentation_by_skill
            .insert(action.skill_id, presentation)
            .is_some()
        {
            return Err(format!("duplicate auxiliary action skill ID {}", action.skill_id).into());
        }
    }
    let mut keys = BTreeMap::new();
    let mut presentation = Vec::new();
    for path in json_files(root)? {
        let record: SkillRecord = serde_json::from_slice(&fs::read(&path)?)?;
        if record.id <= 0
            || !auxiliary_slots
                .iter()
                .all(|slot| record.attributes.unlock_conditions.contains(slot))
        {
            continue;
        }
        let Some(localization_key) = record.localization_key.filter(|key| !key.trim().is_empty())
        else {
            continue;
        };
        if keys.insert(record.id, localization_key).is_some() {
            return Err(format!("duplicate auxiliary action skill ID {}", record.id).into());
        }
        presentation.push(presentation_by_skill.remove(&record.id).ok_or_else(|| {
            format!(
                "auxiliary action {} has no reviewed presentation",
                record.id
            )
        })?);
    }
    if let Some(skill_id) = presentation_by_skill.keys().next() {
        return Err(format!(
            "reviewed auxiliary action {skill_id} is absent from the current skill catalog"
        )
        .into());
    }
    presentation.sort_by_key(|skill| skill.skill_id);
    Ok((keys, presentation))
}

fn load_battle_imagines(
    root: &Path,
) -> Result<(BTreeMap<i64, Vec<String>>, Vec<BattleImaginePresentation>), Box<dyn std::error::Error>>
{
    let mut keys = BTreeMap::new();
    let mut by_skill = BTreeMap::new();
    for path in json_files(root)? {
        let record: BattleImagineRecord = serde_json::from_slice(&fs::read(&path)?)?;
        if record.id <= 0 || record.localization_key.trim().is_empty() {
            return Err(format!("invalid battle-Imagine identity in {}", path.display()).into());
        }
        let Some(skill_id) = record.attributes.aoyi_skill_id.filter(|id| *id > 0) else {
            continue;
        };
        let mut localization_keys = vec![record.localization_key];
        if let Some(key) = record.attributes.aoyi_skill_name_localization_key {
            localization_keys.push(key);
        }
        keys.insert(record.id, localization_keys);
        let presentation = BattleImaginePresentation {
            skill_id,
            item_id: record.id,
            item_tier: record.attributes.item_tier,
            rarity: battle_imagine_rarity(
                record.attributes.item_tier,
                record.attributes.rarity_classification,
            )?
            .to_owned(),
            maximum_tier: record.attributes.maximum_tier.ok_or_else(|| {
                format!("mapped battle-Imagine skill {skill_id} has no maximum tier")
            })?,
            icon: record.icon,
        };
        if by_skill.insert(skill_id, presentation).is_some() {
            return Err(format!("duplicate battle-Imagine skill ID {skill_id}").into());
        }
    }
    Ok((keys, by_skill.into_values().collect()))
}

fn battle_imagine_rarity(
    item_tier: u32,
    rarity_classification: Option<u32>,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    match (item_tier, rarity_classification) {
        (3, None | Some(1)) => Ok("Epic"),
        (4, None | Some(2)) => Ok("SR"),
        (4, Some(3)) => Ok("SSR"),
        (4, Some(4)) => Ok("Collab"),
        _ => Err(format!(
            "unsupported Battle Imagine rarity: item tier {item_tier}, classification {rarity_classification:?}"
        )
        .into()),
    }
}

fn load_monster_keys(root: &Path) -> Result<BTreeMap<String, i64>, Box<dyn std::error::Error>> {
    let mut keys = BTreeMap::new();
    for path in json_files(root)? {
        let record: MonsterRecord = serde_json::from_slice(&fs::read(&path)?)?;
        let Some(key) = record.localization_key else {
            continue;
        };
        if key != format!("monster.{}.name", record.id) {
            return Err(format!(
                "monster {} has unexpected localization key {key} in {}",
                record.id,
                path.display()
            )
            .into());
        }
        if let Some(previous) = keys.insert(key.clone(), record.id) {
            return Err(format!(
                "duplicate monster localization key {key} for IDs {previous} and {}",
                record.id
            )
            .into());
        }
    }
    Ok(keys)
}

fn load_scenes(
    root: &Path,
) -> Result<(BTreeMap<String, i64>, Vec<ScenePresentation>), Box<dyn std::error::Error>> {
    let mut keys = BTreeMap::new();
    let mut by_id = BTreeMap::new();
    for path in json_files(root)? {
        let record: SceneRecord = serde_json::from_slice(&fs::read(&path)?)?;
        if record.id <= 0 || record.attributes.scene_id != record.id {
            return Err(format!("invalid scene identity in {}", path.display()).into());
        }
        if let Some(key) = record.localization_key {
            let expected = format!("scene.{}.name", record.id);
            if key != expected {
                return Err(format!(
                    "scene {} has unexpected localization key {key} in {}",
                    record.id,
                    path.display()
                )
                .into());
            }
            if let Some(previous) = keys.insert(key.clone(), record.id) {
                return Err(format!(
                    "duplicate scene localization key {key} for IDs {previous} and {}",
                    record.id
                )
                .into());
            }
        }
        let presentation = ScenePresentation {
            scene_id: record.id,
            scene_type: record.attributes.scene_type,
            scene_subtype: record.attributes.scene_subtype,
            parent_scene_id: record.attributes.parent_scene_id,
            scene_resource_id: record.attributes.scene_resource_id,
        };
        if by_id.insert(record.id, presentation).is_some() {
            return Err(format!("duplicate scene ID {}", record.id).into());
        }
    }
    Ok((keys, by_id.into_values().collect()))
}

fn load_localization_entries(
    root: &Path,
) -> Result<BTreeMap<String, LocalizationEntry>, Box<dyn std::error::Error>> {
    let mut entries = BTreeMap::new();
    for path in json_files(root)? {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        let values = match value {
            serde_json::Value::Array(values) => values,
            value @ serde_json::Value::Object(_) => vec![value],
            _ => {
                return Err(format!(
                    "localization file must contain an object or array: {}",
                    path.display()
                )
                .into());
            }
        };
        for value in values {
            let mut entry: LocalizationEntry = serde_json::from_value(value)?;
            entry.text = entry
                .text
                .replace(
                    ['\u{0000}', '\u{200b}', '\u{200c}', '\u{200d}', '\u{feff}'],
                    "",
                )
                .trim()
                .to_owned();
            let key = entry.key.clone();
            if entries.insert(key.clone(), entry).is_some() {
                return Err(format!("duplicate localization key {key}").into());
            }
        }
    }
    Ok(entries)
}

fn json_files(root: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    collect_json_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_json_files(
    root: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_json_files(&path, files)?;
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn write_json_atomic(
    path: &Path,
    value: &impl Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    let partial = path.with_extension("partial");
    match fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::write(&partial, encoded)?;
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::rename(partial, path)?;
    Ok(())
}

fn usage() -> String {
    "usage: rlogs-bpsr-runtime-presentation <reviewed-catalog-folder> <runtime-localization-folder>"
        .into()
}
