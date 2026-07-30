use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use rlogs_game_data::{
    AssetRecord, CachePolicy, CompiledShardDescriptor, DEFAULT_SHARD_BITS, GameDataManifest,
    GameDataRecord, GameDataStore, LocalizationEntry, RecordKey, ShardKind, SymbolKind,
    build_bundle_manifest, encode_json_shard, localization_bucket, numeric_id_bucket,
    stable_key_bucket, validate_source_data,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

const MAXIMUM_UNCOMPRESSED_SHARD_BYTES: u64 = 8 * 1024 * 1024;

fn main() {
    if let Err(error) = run() {
        eprintln!("game-data build failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    const USAGE: &str = "usage: rlogs-game-data-build [--asset-root <folder>] <source-catalog-folder> <compiled-folder>";
    let mut arguments = std::env::args_os().skip(1);
    let mut asset_root = None;
    let mut positional = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--asset-root" {
            if asset_root.is_some() {
                return Err(format!("--asset-root may only be supplied once\n{USAGE}").into());
            }
            asset_root = Some(
                arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("--asset-root requires a folder\n{USAGE}"))?,
            );
        } else {
            positional.push(PathBuf::from(argument));
        }
    }
    if positional.len() != 2 {
        return Err(USAGE.into());
    }
    let source = positional.remove(0);
    let output = positional.remove(0);
    let asset_root = asset_root.unwrap_or_else(|| source.clone());

    let source_data = compile_source(&source, &asset_root)?;
    let manifest = write_bundle(&source_data, &output, DEFAULT_SHARD_BITS)?;
    println!(
        "compiled {} records, {} localization entries, and {} assets into {} shards at {} ({})",
        source_data.records.len(),
        source_data.localization.len(),
        source_data.assets.len(),
        manifest.shards.len(),
        output.display(),
        manifest.content_digest
    );
    Ok(())
}

#[derive(Debug)]
struct SourceData {
    manifest: GameDataManifest,
    records: Vec<GameDataRecord>,
    localization: Vec<LocalizationEntry>,
    assets: Vec<AssetRecord>,
}

fn compile_source(
    root: &Path,
    asset_root: &Path,
) -> Result<SourceData, Box<dyn std::error::Error>> {
    let manifest_path = root.join("manifest.json");
    let manifest: GameDataManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    let mut records = Vec::new();
    let mut localization = Vec::new();
    let icon_files = collect_icon_files(asset_root)?;

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
            || relative == Path::new("promotion-summary.json")
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
            continue;
        }
        if top == "localization" {
            let locale = relative
                .components()
                .nth(1)
                .and_then(component_text)
                .ok_or_else(|| format!("missing locale folder in {}", relative.display()))?;
            require_json(relative)?;
            let mut items = parse_localization_file(entry.path())?;
            for item in &items {
                if item.locale != locale {
                    return Err(format!(
                        "locale {} does not match folder {} in {}",
                        item.locale,
                        locale,
                        relative.display()
                    )
                    .into());
                }
            }
            localization.append(&mut items);
            continue;
        }

        let expected_kind = domain_kind(top)
            .ok_or_else(|| format!("unrecognized top-level path {}", relative.display()))?;
        if entry.file_name() == OsStr::new("README.md") {
            continue;
        }
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

    validate_icons(&records, &icon_files, asset_root)?;
    let assets = build_assets(&records, asset_root)?;
    validate_source_data(&manifest, &records, &localization, &assets)?;
    Ok(SourceData {
        manifest,
        records,
        localization,
        assets,
    })
}

fn collect_icon_files(asset_root: &Path) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let icons_root = asset_root.join("icons");
    let mut icon_files = BTreeSet::new();
    for entry in WalkDir::new(&icons_root).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(asset_root)?;
        validate_relative_path(relative)?;
        if entry.path().extension() == Some(OsStr::new("json")) {
            return Err(format!(
                "icon metadata belongs in domain records, not {}",
                relative.display()
            )
            .into());
        }
        icon_files.insert(normalized_path(relative));
    }
    Ok(icon_files)
}

fn write_bundle(
    source: &SourceData,
    output: &Path,
    shard_bits: u8,
) -> Result<rlogs_game_data::CompiledBundleManifest, Box<dyn std::error::Error>> {
    if output.exists() {
        return Err(format!(
            "compiled output already exists; choose a new build folder: {}",
            output.display()
        )
        .into());
    }
    let temporary = output.with_extension(format!("tmp-{}", std::process::id()));
    if temporary.exists() {
        return Err(format!("temporary output already exists: {}", temporary.display()).into());
    }
    fs::create_dir_all(&temporary)?;

    let result = (|| {
        let mut descriptors = Vec::new();

        let mut records = BTreeMap::<(SymbolKind, u16), Vec<GameDataRecord>>::new();
        let mut record_keys = BTreeMap::<u16, Vec<RecordKey>>::new();
        for record in &source.records {
            records
                .entry((record.kind, numeric_id_bucket(record.id, shard_bits)))
                .or_default()
                .push(record.clone());
            record_keys
                .entry(stable_key_bucket(&record.stable_key, shard_bits))
                .or_default()
                .push(RecordKey {
                    stable_key: record.stable_key.clone(),
                    kind: record.kind,
                    id: record.id,
                });
        }
        for ((kind, bucket), mut entries) in records {
            entries.sort_by_key(|record| record.id);
            let relative = format!("records/{}/{bucket:02x}.json.zst", kind.folder());
            descriptors.push(write_shard(
                &temporary,
                &relative,
                ShardKind::Records,
                Some(kind),
                None,
                bucket,
                &entries,
            )?);
        }
        for (bucket, mut entries) in record_keys {
            entries.sort_by(|left, right| left.stable_key.cmp(&right.stable_key));
            let relative = format!("record-keys/{bucket:02x}.json.zst");
            descriptors.push(write_shard(
                &temporary,
                &relative,
                ShardKind::RecordKeys,
                None,
                None,
                bucket,
                &entries,
            )?);
        }

        let mut localization = BTreeMap::<(String, u16), Vec<LocalizationEntry>>::new();
        for entry in &source.localization {
            localization
                .entry((
                    entry.locale.clone(),
                    localization_bucket(&entry.key, shard_bits),
                ))
                .or_default()
                .push(entry.clone());
        }
        for ((locale, bucket), mut entries) in localization {
            entries.sort_by(|left, right| left.key.cmp(&right.key));
            let relative = format!("localization/{locale}/{bucket:02x}.json.zst");
            descriptors.push(write_shard(
                &temporary,
                &relative,
                ShardKind::Localization,
                None,
                Some(locale),
                bucket,
                &entries,
            )?);
        }

        let mut assets = BTreeMap::<u16, Vec<AssetRecord>>::new();
        for asset in &source.assets {
            assets
                .entry(stable_key_bucket(&asset.key, shard_bits))
                .or_default()
                .push(asset.clone());
        }
        for (bucket, mut entries) in assets {
            entries.sort_by(|left, right| left.key.cmp(&right.key));
            let relative = format!("assets/{bucket:02x}.json.zst");
            descriptors.push(write_shard(
                &temporary,
                &relative,
                ShardKind::Assets,
                None,
                None,
                bucket,
                &entries,
            )?);
        }

        let manifest = build_bundle_manifest(source.manifest.clone(), shard_bits, descriptors)?;
        fs::write(
            temporary.join("manifest.json"),
            serde_json::to_vec(&manifest)?,
        )?;
        verify_bundle(source, &temporary)?;
        Ok::<_, Box<dyn std::error::Error>>(manifest)
    })();

    match result {
        Ok(manifest) => {
            if let Some(parent) = output.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&temporary, output)?;
            Ok(manifest)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            Err(error)
        }
    }
}

fn verify_bundle(source: &SourceData, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let store = GameDataStore::open(root, CachePolicy::default())?;
    for expected in &source.records {
        let actual = store
            .record(expected.kind, expected.id)?
            .ok_or_else(|| format!("compiled bundle is missing {}", expected.stable_key))?;
        if actual.as_ref() != expected {
            return Err(format!(
                "compiled record differs for {}: expected={}, actual={}",
                expected.stable_key,
                serde_json::to_string(expected)?,
                serde_json::to_string(actual.as_ref())?
            )
            .into());
        }
        let by_key = store
            .record_by_key(&expected.stable_key)?
            .ok_or_else(|| format!("compiled key is missing {}", expected.stable_key))?;
        if by_key.kind != expected.kind || by_key.id != expected.id {
            return Err(
                format!("compiled key points elsewhere for {}", expected.stable_key).into(),
            );
        }
    }
    for expected in &source.localization {
        let actual = store
            .localization_entry(&expected.locale, &expected.key)?
            .ok_or_else(|| {
                format!(
                    "compiled localization is missing {}/{}",
                    expected.locale, expected.key
                )
            })?;
        if actual.as_ref() != expected {
            return Err(format!(
                "compiled localization differs for {}/{}: expected={}, actual={}",
                expected.locale,
                expected.key,
                serde_json::to_string(expected)?,
                serde_json::to_string(actual.as_ref())?
            )
            .into());
        }
    }
    for expected in &source.assets {
        let actual = store
            .asset(&expected.key)?
            .ok_or_else(|| format!("compiled asset is missing {}", expected.key))?;
        if actual.as_ref() != expected {
            return Err(format!("compiled asset differs for {}", expected.key).into());
        }
    }
    Ok(())
}

fn write_shard<T: Serialize>(
    root: &Path,
    relative: &str,
    kind: ShardKind,
    symbol_kind: Option<SymbolKind>,
    locale: Option<String>,
    bucket: u16,
    entries: &[T],
) -> Result<CompiledShardDescriptor, Box<dyn std::error::Error>> {
    let (compressed, uncompressed_bytes, content_sha256) = encode_json_shard(entries)?;
    if uncompressed_bytes > MAXIMUM_UNCOMPRESSED_SHARD_BYTES {
        return Err(format!(
            "{relative} is {uncompressed_bytes} bytes uncompressed; increase shard_bits or split the source domain"
        )
        .into());
    }
    let compressed_sha256 = format!("sha256:{:x}", Sha256::digest(&compressed));
    let output = root.join(relative);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, &compressed)?;
    Ok(CompiledShardDescriptor {
        kind,
        symbol_kind,
        locale,
        bucket,
        relative_path: relative.to_owned(),
        entries: entries.len().try_into()?,
        compressed_bytes: compressed.len().try_into()?,
        uncompressed_bytes,
        compressed_sha256,
        content_sha256,
    })
}

fn parse_localization_file(
    path: &Path,
) -> Result<Vec<LocalizationEntry>, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    if value.is_array() {
        Ok(serde_json::from_value(value)?)
    } else {
        Ok(vec![serde_json::from_value(value)?])
    }
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
            _ => {
                return Err(format!("invalid source path {}", path.display()).into());
            }
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
    let mut by_key = BTreeMap::<String, String>::new();
    for record in records {
        let Some(path) = &record.icon else {
            continue;
        };
        if let Some(existing) = by_key.insert(record.stable_key.clone(), path.clone()) {
            return Err(format!(
                "asset key {} is assigned to both {} and {}",
                record.stable_key, existing, path
            )
            .into());
        }
    }

    by_key
        .into_iter()
        .map(|(key, path)| {
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
    SymbolKind::ALL
        .into_iter()
        .find(|kind| kind.folder() == folder)
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
    use rlogs_game_data::{CachePolicy, GameDataBuild, GameDataStore};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/game-data/reference-build")
    }

    fn temporary_output() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rlogs-game-data-{}-{suffix}", std::process::id()))
    }

    #[test]
    fn every_supported_human_folder_has_one_symbol_kind() {
        assert_eq!(domain_kind("skills"), Some(SymbolKind::Skill));
        assert_eq!(domain_kind("monsters"), Some(SymbolKind::Monster));
        assert_eq!(domain_kind("maps"), Some(SymbolKind::Map));
        assert_eq!(domain_kind("dungeons"), Some(SymbolKind::Dungeon));
        assert_eq!(domain_kind("modules"), Some(SymbolKind::Module));
        assert_eq!(
            domain_kind("module-effects"),
            Some(SymbolKind::ModuleEffect)
        );
        assert_eq!(domain_kind("unknown-folder"), None);
    }

    #[test]
    fn shared_icon_files_are_exposed_through_each_record_key() {
        let source = compile_source(&fixture_root(), &fixture_root()).unwrap();
        let mut first = source
            .records
            .iter()
            .find(|record| record.icon.is_some())
            .unwrap()
            .clone();
        let mut second = first.clone();
        first.stable_key = "shared-icon.first".into();
        second.stable_key = "shared-icon.second".into();

        let assets = build_assets(&[first, second], &fixture_root()).unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].relative_path, assets[1].relative_path);
        assert_ne!(assets[0].key, assets[1].key);
        assert_eq!(assets[0].sha256, assets[1].sha256);
    }

    #[test]
    fn localization_files_may_be_one_entry_or_a_reviewed_array() {
        let path = fixture_root()
            .join("localization/en-US/skills/stormblade/iaido/1714-example-skill.json");
        assert_eq!(parse_localization_file(&path).unwrap().len(), 1);
    }

    #[test]
    fn fixture_compiles_and_loads_through_bounded_shards() {
        let source = compile_source(&fixture_root(), &fixture_root()).unwrap();
        let output = temporary_output();
        let manifest = write_bundle(&source, &output, DEFAULT_SHARD_BITS).unwrap();
        assert!(!manifest.shards.is_empty());

        let store = GameDataStore::open(&output, CachePolicy::default()).unwrap();
        assert_eq!(
            store
                .record(SymbolKind::Skill, 1714)
                .unwrap()
                .unwrap()
                .stable_key,
            "skill.stormblade.iaido.1714"
        );
        assert_eq!(
            store
                .record_by_key("skill.stormblade.iaido.1714")
                .unwrap()
                .unwrap()
                .id,
            1714
        );
        assert_eq!(
            store
                .localized("en-US", "game.skill.1714")
                .unwrap()
                .as_deref(),
            Some("Example Skill")
        );
        let supported_build = GameDataBuild {
            deployment_id: "global".into(),
            channel: "steam".into(),
            client_build: "fixture-not-for-live-use".into(),
        };
        assert!(
            store
                .record_for_build(SymbolKind::Skill, 1714, &supported_build)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store
                .localized_for_build("en-US", "game.skill.1714", &supported_build)
                .unwrap()
                .as_deref(),
            Some("Example Skill")
        );
        let unsupported_build = GameDataBuild {
            client_build: "different-build".into(),
            ..supported_build
        };
        assert!(
            store
                .record_for_build(SymbolKind::Skill, 1714, &unsupported_build)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .asset("skill.stormblade.iaido.1714")
                .unwrap()
                .unwrap()
                .media_type,
            "image/svg+xml"
        );
        let stats = store.cache_stats().unwrap();
        assert!(stats.resident_shards <= 4);
        assert!(stats.resident_bytes <= CachePolicy::default().maximum_resident_bytes);
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn tampered_shard_is_rejected_before_json_is_used() {
        let source = compile_source(&fixture_root(), &fixture_root()).unwrap();
        let output = temporary_output();
        let manifest = write_bundle(&source, &output, DEFAULT_SHARD_BITS).unwrap();
        let descriptor = manifest
            .shards
            .iter()
            .find(|shard| {
                shard.kind == ShardKind::Records && shard.symbol_kind == Some(SymbolKind::Skill)
            })
            .unwrap();
        let path = output.join(&descriptor.relative_path);
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.last_mut().unwrap();
        *last ^= 0xff;
        fs::write(&path, bytes).unwrap();

        let store = GameDataStore::open(&output, CachePolicy::default()).unwrap();
        assert!(store.record(SymbolKind::Skill, 1714).is_err());
        fs::remove_dir_all(output).unwrap();
    }
}
