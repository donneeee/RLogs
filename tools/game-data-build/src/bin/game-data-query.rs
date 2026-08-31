use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rlogs_game_data::{CachePolicy, GameDataRecord, GameDataStore, LocalizationEntry, SymbolKind};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct LocalizationResult {
    locale: String,
    key: String,
    text: Option<String>,
}

#[derive(Debug, Serialize)]
struct LocalizationSearchResult {
    locale: String,
    key: String,
    text: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("game-data query failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    const USAGE: &str = "usage:\n  game-data-query <compiled-catalog-folder> localization <locale> <localization-key>...\n  game-data-query <compiled-catalog-folder> localization-search <locale> <required-text>...\n  game-data-query <compiled-catalog-folder> record <skill|skill-effect|status-effect|talent> <id>...";
    let mut arguments = std::env::args_os().skip(1);
    let root = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| USAGE.to_owned())?;
    let operation = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| USAGE.to_owned())?;
    let store = GameDataStore::open(&root, CachePolicy::default())?;
    match operation.as_str() {
        "localization" => {
            let locale = next_string(&mut arguments, USAGE)?;
            let keys = remaining_strings(arguments, USAGE)?;
            let results = keys
                .into_iter()
                .map(|key| {
                    let text = store
                        .localized(&locale, &key)?
                        .map(|value| value.to_string());
                    Ok(LocalizationResult {
                        locale: locale.clone(),
                        key,
                        text,
                    })
                })
                .collect::<Result<Vec<_>, rlogs_game_data::GameDataError>>()?;
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        "localization-search" => {
            let locale = next_string(&mut arguments, USAGE)?;
            let needles = remaining_strings(arguments, USAGE)?
                .into_iter()
                .map(|value| value.to_lowercase())
                .collect::<Vec<_>>();
            let results = search_localization(&root, &locale, &needles)?;
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        "record" => {
            let kind = match next_string(&mut arguments, USAGE)?.as_str() {
                "skill" => SymbolKind::Skill,
                "skill-effect" => SymbolKind::SkillEffect,
                "status-effect" => SymbolKind::StatusEffect,
                "talent" => SymbolKind::Talent,
                _ => return Err(USAGE.into()),
            };
            let ids = remaining_strings(arguments, USAGE)?
                .into_iter()
                .map(|value| value.parse::<i64>())
                .collect::<Result<Vec<_>, _>>()?;
            let results = ids
                .into_iter()
                .map(|id| {
                    store
                        .record(kind, id)
                        .map(|record| record.map(owned_record))
                })
                .collect::<Result<Vec<_>, _>>()?;
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn search_localization(
    root: &Path,
    locale: &str,
    needles: &[String],
) -> Result<Vec<LocalizationSearchResult>, Box<dyn std::error::Error>> {
    let folder = root.join("localization").join(locale);
    let mut paths = fs::read_dir(&folder)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".json.zst"))
    });
    paths.sort();

    let mut results = Vec::new();
    for path in paths {
        let compressed = fs::read(path)?;
        let decoded = zstd::stream::decode_all(Cursor::new(compressed))?;
        let entries = serde_json::from_slice::<Vec<LocalizationEntry>>(&decoded)?;
        for entry in entries {
            let searchable = format!("{}\n{}", entry.key, entry.text).to_lowercase();
            if needles.iter().all(|needle| searchable.contains(needle)) {
                results.push(LocalizationSearchResult {
                    locale: entry.locale,
                    key: entry.key,
                    text: entry.text,
                });
            }
        }
    }
    results.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(results)
}

fn next_string(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    usage: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| usage.to_owned().into())
}

fn remaining_strings(
    arguments: impl Iterator<Item = std::ffi::OsString>,
    usage: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let values = arguments
        .map(|value| value.into_string().map_err(|_| usage.to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(usage.to_owned().into());
    }
    Ok(values)
}

fn owned_record(record: std::sync::Arc<GameDataRecord>) -> GameDataRecord {
    (*record).clone()
}
