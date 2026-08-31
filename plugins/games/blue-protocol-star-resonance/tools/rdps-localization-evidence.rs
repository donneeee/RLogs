use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

use serde::Serialize;
use serde_json::{Map, Value};

const SCHEMA_VERSION: u16 = 1;
const GAME_LOCALES: [&str; 11] = [
    "en", "zh-CN", "zh-TW", "ja", "ko-KR", "fr", "de", "es", "pt-BR", "th", "id",
];

#[derive(Debug, Serialize)]
struct EvidenceCatalog {
    schema_version: u16,
    generated_by: &'static str,
    policy: Policy,
    inputs: Inputs,
    summary: Summary,
    candidates: Vec<CandidateEvidence>,
}

#[derive(Debug, Serialize)]
struct Policy {
    scope: &'static str,
    localization_is_mechanics_proof: bool,
    missing_localization_is_hidden: bool,
    runtime_use: &'static str,
}

#[derive(Debug, Serialize)]
struct Inputs {
    candidate_inventory: String,
    buff_descriptions: String,
}

#[derive(Debug, Serialize)]
struct Summary {
    candidate_effects: usize,
    candidate_related_id_rows: usize,
    unique_related_buff_ids: usize,
    ids_with_description_entries: usize,
    ids_without_description_entries: usize,
    ids_with_all_game_locale_names: usize,
    ids_with_all_game_locale_descriptions: usize,
    game_locales: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct CandidateEvidence {
    effect_id: i64,
    observed_scope: ObservedScope,
    current_buff_row: Value,
    packet_origins: Value,
    related_ids: Vec<LocalizedBuffEvidence>,
}

#[derive(Debug, Serialize)]
struct ObservedScope {
    player_provider_to_player_recipient_windows: u64,
    player_provider_to_monster_recipient_windows: u64,
}

#[derive(Debug, Serialize)]
struct LocalizedBuffEvidence {
    buff_id: i64,
    relationships: Vec<String>,
    entry_found: bool,
    design_name: Option<String>,
    names: BTreeMap<&'static str, Option<String>>,
    clean_descriptions: BTreeMap<&'static str, Option<String>>,
    source_blocks: Vec<SourceBlock>,
    owner_party_split: Value,
    stacking: Value,
    relationships_from_description_source: Value,
    provenance: Value,
}

#[derive(Debug, Serialize)]
struct SourceBlock {
    source_file: Option<String>,
    source_path: Option<String>,
    kind: Option<String>,
    clean_descriptions: BTreeMap<&'static str, Option<String>>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let candidate_path = PathBuf::from(args.next().ok_or_else(usage)?);
    let descriptions_path = PathBuf::from(args.next().ok_or_else(usage)?);
    let output_path = PathBuf::from(args.next().ok_or_else(usage)?);
    if args.next().is_some() {
        return Err(usage().into());
    }

    let candidate_inventory: Value =
        serde_json::from_reader(BufReader::new(File::open(&candidate_path)?))?;
    let descriptions: Value =
        serde_json::from_reader(BufReader::new(File::open(&descriptions_path)?))?;
    let entries = descriptions
        .get("entriesByUid")
        .and_then(Value::as_object)
        .ok_or("BuffDescriptions.json is missing object entriesByUid")?;

    let candidate_rows = candidate_inventory
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or("candidate inventory is missing candidates")?;

    let mut unique_ids = BTreeSet::new();
    let mut candidates = Vec::with_capacity(candidate_rows.len());

    for row in candidate_rows {
        let effect_id = required_i64(row, "effect_id")?;
        let mut relation_map: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
        relation_map
            .entry(effect_id)
            .or_default()
            .insert("packet-observed-effect".to_owned());

        if let Some(related) = row.get("exact_related_buff_ids").and_then(Value::as_array) {
            for relation in related {
                let buff_id = required_i64(relation, "buff_id")?;
                let relationship = relation
                    .get("relationship")
                    .and_then(Value::as_str)
                    .unwrap_or("unspecified")
                    .to_owned();
                relation_map
                    .entry(buff_id)
                    .or_default()
                    .insert(relationship);
            }
        }

        let mut related_ids = Vec::with_capacity(relation_map.len());
        for (buff_id, relationships) in relation_map {
            unique_ids.insert(buff_id);
            related_ids.push(localized_evidence(
                buff_id,
                relationships.into_iter().collect(),
                entries.get(&buff_id.to_string()),
            ));
        }

        candidates.push(CandidateEvidence {
            effect_id,
            observed_scope: ObservedScope {
                player_provider_to_player_recipient_windows: optional_u64(
                    row,
                    "exact_player_provider_to_player_recipient_windows",
                ),
                player_provider_to_monster_recipient_windows: optional_u64(
                    row,
                    "exact_player_provider_to_monster_recipient_windows",
                ),
            },
            current_buff_row: row.get("current_buff_row").cloned().unwrap_or(Value::Null),
            packet_origins: row
                .get("packet_origins")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
            related_ids,
        });
    }

    let localized_rows = candidates
        .iter()
        .flat_map(|candidate| candidate.related_ids.iter())
        .collect::<Vec<_>>();
    let unique_localized_rows = localized_rows.iter().fold(
        BTreeMap::<i64, &LocalizedBuffEvidence>::new(),
        |mut by_id, evidence| {
            by_id.entry(evidence.buff_id).or_insert(evidence);
            by_id
        },
    );
    let ids_with_description_entries = unique_localized_rows
        .values()
        .filter(|evidence| evidence.entry_found)
        .count();
    let ids_with_all_game_locale_names = unique_localized_rows
        .values()
        .filter(|evidence| evidence.names.values().all(Option::is_some))
        .count();
    let ids_with_all_game_locale_descriptions = unique_localized_rows
        .values()
        .filter(|evidence| evidence.clean_descriptions.values().all(Option::is_some))
        .count();

    let output = EvidenceCatalog {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-localization-evidence",
        policy: Policy {
            scope: "research-only packet-observed cross-actor candidate evidence",
            localization_is_mechanics_proof: false,
            missing_localization_is_hidden: false,
            runtime_use: "not loaded by the live parser or combat reducer",
        },
        inputs: Inputs {
            candidate_inventory: candidate_path.display().to_string(),
            buff_descriptions: descriptions_path.display().to_string(),
        },
        summary: Summary {
            candidate_effects: candidates.len(),
            candidate_related_id_rows: localized_rows.len(),
            unique_related_buff_ids: unique_ids.len(),
            ids_with_description_entries,
            ids_without_description_entries: unique_ids.len() - ids_with_description_entries,
            ids_with_all_game_locale_names,
            ids_with_all_game_locale_descriptions,
            game_locales: GAME_LOCALES.to_vec(),
        },
        candidates,
    };

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    serde_json::to_writer_pretty(BufWriter::new(File::create(output_path)?), &output)?;
    Ok(())
}

fn localized_evidence(
    buff_id: i64,
    relationships: Vec<String>,
    entry: Option<&Value>,
) -> LocalizedBuffEvidence {
    let empty = Map::new();
    let entry_object = entry.and_then(Value::as_object).unwrap_or(&empty);
    let names = localized_values(entry_object.get("names"), None);
    let clean_descriptions = localized_values(
        entry_object.get("cleanDescriptions"),
        entry_object.get("descriptions"),
    );
    let source_blocks = entry_object
        .get("descriptionBlocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|block| SourceBlock {
            source_file: string_field(block, "sourceFile"),
            source_path: string_field(block, "sourcePath"),
            kind: string_field(block, "kind"),
            clean_descriptions: localized_values(
                block.get("cleanDescriptions"),
                block.get("descriptions"),
            ),
        })
        .collect();

    LocalizedBuffEvidence {
        buff_id,
        relationships,
        entry_found: entry.is_some(),
        design_name: entry_object
            .get("names")
            .and_then(Value::as_object)
            .and_then(|names| names.get("design"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        names,
        clean_descriptions,
        source_blocks,
        owner_party_split: cloned_or_null(entry_object, "ownerPartySplit"),
        stacking: cloned_or_null(entry_object, "stacking"),
        relationships_from_description_source: cloned_or_null(entry_object, "relationships"),
        provenance: cloned_or_null(entry_object, "provenance"),
    }
}

fn localized_values(
    preferred: Option<&Value>,
    fallback: Option<&Value>,
) -> BTreeMap<&'static str, Option<String>> {
    let preferred = preferred.and_then(Value::as_object);
    let fallback = fallback.and_then(Value::as_object);
    GAME_LOCALES
        .into_iter()
        .map(|locale| {
            let value = preferred
                .and_then(|values| values.get(locale))
                .or_else(|| fallback.and_then(|values| values.get(locale)))
                .and_then(Value::as_str)
                .map(str::to_owned);
            (locale, value)
        })
        .collect()
}

fn required_i64(value: &Value, field: &str) -> Result<i64, Box<dyn std::error::Error>> {
    value
        .get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer {field}").into())
}

fn optional_u64(value: &Value, field: &str) -> u64 {
    value.get(field).and_then(Value::as_u64).unwrap_or(0)
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn cloned_or_null(values: &Map<String, Value>, field: &str) -> Value {
    values.get(field).cloned().unwrap_or(Value::Null)
}

fn usage() -> String {
    "usage: rlogs-bpsr-rdps-localization-evidence <rdps-candidate-inventory.json> <BuffDescriptions.json> <output.json>".to_owned()
}
