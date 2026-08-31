use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Serialize)]
struct SemanticSurface {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    promotion_state: &'static str,
    input: InputArtifact,
    policy: Policy,
    summary: Summary,
    linked_hit_event_candidate_lookup: BTreeMap<String, Vec<i64>>,
    rows: BTreeMap<String, SurfaceRow>,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    role: &'static str,
    file: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Policy {
    runtime_formula_authority: bool,
    semantic_decoded_bridge: bool,
    raw_ctb_offset_authority: bool,
    exact_build_table_required: bool,
    unresolved_rows_hidden: bool,
    lookup_rule: &'static str,
    damage_script_field_rule: &'static str,
    coefficient_field_rule: &'static str,
}

#[derive(Debug, Serialize)]
struct Summary {
    decoded_rows: usize,
    emitted_rows: usize,
    lookup_keys: usize,
    ambiguous_lookup_keys: usize,
    maximum_candidates_per_lookup: usize,
}

#[derive(Debug, Serialize)]
struct SurfaceRow {
    damage_id: i64,
    linked_id: i64,
    hit_event_suffix_candidate: i64,
    damage_script: String,
    int_array_pool_1_candidates_by_offset: BTreeMap<String, ArrayValues>,
}

#[derive(Debug, Serialize)]
struct ArrayValues {
    values: Vec<i64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("damage-attribute semantic surface failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 6 {
        return Err(usage().into());
    }
    let decoded_table_path = PathBuf::from(option(&arguments, "--decoded-table")?);
    let game_build = option(&arguments, "--build")?.to_owned();
    let output_path = PathBuf::from(option(&arguments, "--output")?);
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".into());
    }

    let bytes = fs::read(&decoded_table_path)?;
    let decoded: Value = serde_json::from_slice(&bytes)?;
    let (lookup, rows) = build_surface(&decoded)?;
    let ambiguous_lookup_keys = lookup.values().filter(|values| values.len() > 1).count();
    let maximum_candidates_per_lookup = lookup.values().map(Vec::len).max().unwrap_or(0);
    let decoded_rows = decoded.as_object().map_or(0, serde_json::Map::len);
    let surface = SemanticSurface {
        schema_version: SCHEMA_VERSION,
        game_build,
        generated_by: "rlogs-bpsr-damage-attr-semantic-surface",
        promotion_state: "offline_exact_build_semantic_bridge",
        input: InputArtifact {
            role: "exact_build_decoded_damage_attr_table",
            file: display_path(&decoded_table_path),
            bytes: bytes.len(),
            sha256: hex_digest(&bytes),
        },
        policy: Policy {
            runtime_formula_authority: false,
            semantic_decoded_bridge: true,
            raw_ctb_offset_authority: false,
            exact_build_table_required: true,
            unresolved_rows_hidden: false,
            lookup_rule: "TypeEnum plus decimal Id modulo 100; every decoded row is retained",
            damage_script_field_rule: "damage_script mirrors decoded DamageScript exactly and is grouping evidence, not server formula authority",
            coefficient_field_rule: "offset 28 mirrors decoded PVEDamageRadio and offset 32 mirrors decoded PVEFixedParameter",
        },
        summary: Summary {
            decoded_rows,
            emitted_rows: rows.len(),
            lookup_keys: lookup.len(),
            ambiguous_lookup_keys,
            maximum_candidates_per_lookup,
        },
        linked_hit_event_candidate_lookup: lookup,
        rows,
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(&output_path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &surface)?;
    writer.write_all(b"\n")?;
    eprintln!(
        "wrote {} rows and {} lookup keys to {}",
        surface.summary.emitted_rows,
        surface.summary.lookup_keys,
        output_path.display()
    );
    Ok(())
}

fn build_surface(
    decoded: &Value,
) -> Result<(BTreeMap<String, Vec<i64>>, BTreeMap<String, SurfaceRow>), String> {
    let decoded_rows = decoded
        .as_object()
        .ok_or_else(|| "decoded DamageAttrTable root must be an object".to_owned())?;
    let mut lookup = BTreeMap::<String, BTreeSet<i64>>::new();
    let mut rows = BTreeMap::new();
    for (key, row) in decoded_rows {
        let id = integer(row, "Id", key)?;
        if key != &id.to_string() {
            return Err(format!("row key {key} does not equal Id {id}"));
        }
        let linked_id = integer(row, "TypeEnum", key)?;
        let hit_event_suffix_candidate = id.rem_euclid(100);
        let damage_script = string(row, "DamageScript", key)?;
        let mut arrays = BTreeMap::new();
        arrays.insert(
            "28".to_owned(),
            ArrayValues {
                values: integer_array(row, "PVEDamageRadio", key)?,
            },
        );
        arrays.insert(
            "32".to_owned(),
            ArrayValues {
                values: integer_array(row, "PVEFixedParameter", key)?,
            },
        );
        lookup
            .entry(format!("{linked_id}:{hit_event_suffix_candidate}"))
            .or_default()
            .insert(id);
        if rows
            .insert(
                key.clone(),
                SurfaceRow {
                    damage_id: id,
                    linked_id,
                    hit_event_suffix_candidate,
                    damage_script,
                    int_array_pool_1_candidates_by_offset: arrays,
                },
            )
            .is_some()
        {
            return Err(format!("duplicate DamageAttr row {key}"));
        }
    }
    Ok((
        lookup
            .into_iter()
            .map(|(key, values)| (key, values.into_iter().collect()))
            .collect(),
        rows,
    ))
}

fn integer(row: &Value, field: &str, key: &str) -> Result<i64, String> {
    row.get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("DamageAttr row {key} has no integer {field}"))
}

fn integer_array(row: &Value, field: &str, key: &str) -> Result<Vec<i64>, String> {
    row.get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("DamageAttr row {key} has no array {field}"))?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.as_i64().ok_or_else(|| {
                format!("DamageAttr row {key} field {field}[{index}] is not an integer")
            })
        })
        .collect()
}

fn string(row: &Value, field: &str, key: &str) -> Result<String, String> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("DamageAttr row {key} has no string {field}"))
}

fn option<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| format!("missing {name}; {}", usage()))
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-damage-attr-semantic-surface --decoded-table <DamageAttrTable.json> --build <numeric-build> --output <surface.json>"
}

fn display_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn retains_every_row_and_builds_sorted_ambiguous_lookup() {
        let decoded = json!({
            "15501": {"Id": 15501, "TypeEnum": 155, "DamageScript": "Attack", "PVEDamageRadio": [5200], "PVEFixedParameter": []},
            "1550101": {"Id": 1550101, "TypeEnum": 155, "DamageScript": "MAttack", "PVEDamageRadio": [3400], "PVEFixedParameter": [7]},
            "15502": {"Id": 15502, "TypeEnum": 155, "DamageScript": "SpDamage", "PVEDamageRadio": [], "PVEFixedParameter": [9]}
        });
        let (lookup, rows) = build_surface(&decoded).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(lookup["155:1"], vec![15501, 1550101]);
        assert_eq!(rows["1550101"].hit_event_suffix_candidate, 1);
        assert_eq!(rows["1550101"].damage_script, "MAttack");
        assert_eq!(
            rows["1550101"].int_array_pool_1_candidates_by_offset["32"].values,
            vec![7]
        );
    }

    #[test]
    fn rejects_key_and_id_disagreement() {
        let decoded = json!({
            "100": {"Id": 101, "TypeEnum": 1, "DamageScript": "Attack", "PVEDamageRadio": [], "PVEFixedParameter": []}
        });
        assert!(
            build_surface(&decoded)
                .unwrap_err()
                .contains("does not equal")
        );
    }
}
