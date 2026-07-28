use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use rlogs_game_data::{
    AssetRecord, CompiledGameDataArtifact, GameDataManifest, GameDataRecord, LocalizationEntry,
    SymbolKind,
};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

fn main() {
    if let Err(error) = run() {
        eprintln!("game-data build failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let source = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: rlogs-game-data-build <source-build-folder> <compiled.json>")?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: rlogs-game-data-build <source-build-folder> <compiled.json>")?;
    if arguments.next().is_some() {
        return Err("usage: rlogs-game-data-build <source-build-folder> <compiled.json>".into());
    }

    let artifact = compile_source(&source)?;
    let encoded = serde_json::to_vec_pretty(&artifact)?;
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let temporary = output.with_extension("tmp");
    fs::write(&temporary, encoded)?;
    fs::rename(&temporary, &output)?;
    println!(
        "compiled {} records, {} localization entries, and {} assets to {} ({})",
        artifact.payload.records.len(),
        artifact.payload.localization.len(),
        artifact.payload.assets.len(),
        output.display(),
        artifact.content_digest
    );
    Ok(())
}

fn compile_source(root: &Path) -> Result<CompiledGameDataArtifact, Box<dyn std::error::Error>> {
    let manifest_path = root.join("manifest.json");
    let manifest: GameDataManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    let mut records = Vec::new();
    let mut localization = Vec::new();
    let mut icon_files = BTreeSet::new();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(root)?;
        validate_relative_path(relative)?;
        let Some(top) = first_component(relative) else {
            continue;
        };

        if relative == Path::new("manifest.json")
            || relative == Path::new("README.md")
            || relative == Path::new("extraction-requirements.json")
        {
            continue;
        }
        if top == "icons" {
            if entry.path().extension() == Some(OsStr::new("json")) {
                return Err(format!(
                    "icon metadata belongs in domain records, not {}",
                    relative.display()
                )
                .into());
            }
            icon_files.insert(normalized_path(relative));
            continue;
        }
        if top == "localization" {
            let locale = relative
                .components()
                .nth(1)
                .and_then(component_text)
                .ok_or_else(|| format!("missing locale folder in {}", relative.display()))?;
            require_json(relative)?;
            let item: LocalizationEntry = serde_json::from_slice(&fs::read(entry.path())?)?;
            if item.locale != locale {
                return Err(format!(
                    "locale {} does not match folder {} in {}",
                    item.locale,
                    locale,
                    relative.display()
                )
                .into());
            }
            localization.push(item);
            continue;
        }

        let expected_kind = domain_kind(top)
            .ok_or_else(|| format!("unrecognized top-level path {}", relative.display()))?;
        require_json(relative)?;
        let record: GameDataRecord = serde_json::from_slice(&fs::read(entry.path())?)?;
        if record.kind != expected_kind {
            return Err(format!(
                "record kind {:?} does not match folder {} in {}",
                record.kind,
                top,
                relative.display()
            )
            .into());
        }
        if record.kind == SymbolKind::Skill {
            validate_skill_path(relative, &record)?;
        }
        records.push(record);
    }

    validate_icons(&records, &icon_files, root)?;
    let assets = build_assets(&records, root)?;
    Ok(CompiledGameDataArtifact::build(
        manifest,
        records,
        localization,
        assets,
    )?)
}

fn validate_relative_path(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let text = value.to_string_lossy();
                if text.contains('\\') || text == "." || text == ".." {
                    return Err(format!("invalid source path {}", path.display()).into());
                }
            }
            _ => return Err(format!("invalid source path {}", path.display()).into()),
        }
    }
    Ok(())
}

fn validate_skill_path(
    path: &Path,
    record: &GameDataRecord,
) -> Result<(), Box<dyn std::error::Error>> {
    let parts = path
        .components()
        .filter_map(component_text)
        .collect::<Vec<_>>();
    if parts.len() < 4 {
        return Err(format!(
            "skill records must use skills/<class>/<spec>/<id-name>.json: {}",
            path.display()
        )
        .into());
    }
    let class_folder = &parts[1];
    let spec_folder = &parts[2];
    for (attribute, expected) in [("class_key", *class_folder), ("spec_key", *spec_folder)] {
        let actual = record
            .attributes
            .get(attribute)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                format!(
                    "skill {} is missing string attribute {}",
                    path.display(),
                    attribute
                )
            })?;
        if actual != expected {
            return Err(format!(
                "skill {} {}={} does not match folder {}",
                path.display(),
                attribute,
                actual,
                expected
            )
            .into());
        }
    }
    if let Some(icon) = &record.icon {
        let required_prefix = format!("icons/skills/{class_folder}/{spec_folder}/");
        if !icon.starts_with(&required_prefix) {
            return Err(format!("skill icon {} must be under {}", icon, required_prefix).into());
        }
    }
    Ok(())
}

fn validate_icons(
    records: &[GameDataRecord],
    icon_files: &BTreeSet<String>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let referenced = records
        .iter()
        .filter_map(|record| record.icon.as_ref())
        .cloned()
        .collect::<BTreeSet<_>>();
    for icon in &referenced {
        if icon.contains("..") || !icon.starts_with("icons/") || !root.join(icon).is_file() {
            return Err(format!("missing or unsafe icon path {icon}").into());
        }
    }
    let unreferenced = icon_files.difference(&referenced).collect::<Vec<_>>();
    if !unreferenced.is_empty() {
        return Err(format!("unreferenced icons: {unreferenced:?}").into());
    }
    Ok(())
}

fn build_assets(
    records: &[GameDataRecord],
    root: &Path,
) -> Result<Vec<AssetRecord>, Box<dyn std::error::Error>> {
    let mut by_path = BTreeMap::<String, String>::new();
    for record in records {
        let Some(path) = &record.icon else {
            continue;
        };
        if let Some(existing) = by_path.insert(path.clone(), record.stable_key.clone()) {
            return Err(format!(
                "icon {} is assigned to both {} and {}",
                path, existing, record.stable_key
            )
            .into());
        }
    }

    by_path
        .into_iter()
        .map(|(path, key)| {
            let bytes = fs::read(root.join(&path))?;
            Ok(AssetRecord {
                key,
                relative_path: path.clone(),
                media_type: media_type(&path)?.to_owned(),
                sha256: format!("sha256:{:x}", Sha256::digest(bytes)),
            })
        })
        .collect()
}

fn domain_kind(folder: &str) -> Option<SymbolKind> {
    match folder {
        "classes" => Some(SymbolKind::Class),
        "specializations" => Some(SymbolKind::Specialization),
        "skills" => Some(SymbolKind::Skill),
        "status-effects" => Some(SymbolKind::StatusEffect),
        "monsters" => Some(SymbolKind::Monster),
        "npcs" => Some(SymbolKind::Npc),
        "summons" => Some(SymbolKind::Summon),
        "projectiles" => Some(SymbolKind::Projectile),
        "traps" => Some(SymbolKind::Trap),
        "mechanics" => Some(SymbolKind::Mechanic),
        "entity-types" => Some(SymbolKind::EntityType),
        "scenes" => Some(SymbolKind::Scene),
        "maps" => Some(SymbolKind::Map),
        "dungeons" => Some(SymbolKind::Dungeon),
        "dungeon-objectives" => Some(SymbolKind::DungeonObjective),
        "items" => Some(SymbolKind::Item),
        "equipment" => Some(SymbolKind::Equipment),
        "equipment-sets" => Some(SymbolKind::EquipmentSet),
        "imagines" => Some(SymbolKind::Imagine),
        "cosmetics" => Some(SymbolKind::Cosmetic),
        "professions" => Some(SymbolKind::Profession),
        "talents" => Some(SymbolKind::Talent),
        _ => None,
    }
}

fn media_type(path: &str) -> Result<&'static str, Box<dyn std::error::Error>> {
    match Path::new(path).extension().and_then(OsStr::to_str) {
        Some("png") => Ok("image/png"),
        Some("webp") => Ok("image/webp"),
        Some("svg") => Ok("image/svg+xml"),
        extension => Err(format!("unsupported icon extension {extension:?} for {path}").into()),
    }
}

fn require_json(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if path.extension() != Some(OsStr::new("json")) {
        return Err(format!("expected JSON end product at {}", path.display()).into());
    }
    Ok(())
}

fn first_component(path: &Path) -> Option<&str> {
    path.components().next().and_then(component_text)
}

fn component_text(component: Component<'_>) -> Option<&str> {
    match component {
        Component::Normal(value) => value.to_str(),
        _ => None,
    }
}

fn normalized_path(path: &Path) -> String {
    path.components()
        .filter_map(component_text)
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/game-data/reference-build")
    }

    #[test]
    fn every_supported_human_folder_has_one_symbol_kind() {
        assert_eq!(domain_kind("skills"), Some(SymbolKind::Skill));
        assert_eq!(domain_kind("monsters"), Some(SymbolKind::Monster));
        assert_eq!(domain_kind("maps"), Some(SymbolKind::Map));
        assert_eq!(domain_kind("dungeons"), Some(SymbolKind::Dungeon));
        assert_eq!(domain_kind("unknown-folder"), None);
    }

    #[test]
    fn icon_media_types_are_explicit() {
        assert_eq!(media_type("icons/skill.webp").unwrap(), "image/webp");
        assert_eq!(media_type("icons/skill.png").unwrap(), "image/png");
        assert!(media_type("icons/skill.exe").is_err());
    }

    #[test]
    fn human_readable_fixture_compiles_to_a_valid_runtime_artifact() {
        let artifact = compile_source(&fixture_root()).unwrap();

        assert_eq!(artifact.payload.records.len(), 1);
        assert_eq!(artifact.payload.localization.len(), 1);
        assert_eq!(artifact.payload.assets.len(), 1);
        assert_eq!(
            artifact.payload.records[0].stable_key,
            "skill.stormblade.iaido.1714"
        );
        artifact.validate().unwrap();
    }
}
