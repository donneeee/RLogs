use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const EXPECTED_ROW_COUNT: usize = 3;
const ARRAY_FIELDS: &[&str] = &[
    "DefPara",
    "HitPara",
    "CriToCrit",
    "HasteToHastePct",
    "LuckToLuckyStrikeProb",
    "VersatilityToVersatilityPct",
    "MasteryToMasteryPct",
    "BlockToBlockRate",
    "HateRate",
    "MstToPlayerDmgUp",
    "PlayerToMstDmgDown",
    "MaxDmg",
    "ElementPowerToDam",
    "ElementDefToDamRes",
    "PhyPowerToDam",
    "MagPowerToDam",
    "RefDefPara",
    "SeasonMstToPlayerDmg",
    "SeasonPlayerToMstDmg",
    "SeasonPlayerToMstBk",
    "SeasonHeal",
];
const NUMBER_FIELDS: &[&str] = &["AoeAttenuation", "SingleAttenuation"];

#[derive(Debug, Serialize)]
struct TransformSurface {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    promotion_state: &'static str,
    source: SourceArtifact,
    table: TableIdentity,
    categories: FieldCategories,
    policy: SurfacePolicy,
    rows: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
struct SourceArtifact {
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct TableIdentity {
    name: &'static str,
    row_count: usize,
    expected_primary_keys: Vec<i64>,
    all_primary_keys_match_object_keys: bool,
    all_required_fields_present: bool,
    all_required_field_types_valid: bool,
}

#[derive(Debug, Serialize)]
struct FieldCategories {
    probability_and_stat_conversion: Vec<&'static str>,
    mitigation_and_power_conversion: Vec<&'static str>,
    encounter_and_season_scaling: Vec<&'static str>,
    attenuation_caps_and_threat: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct SurfacePolicy {
    runtime_formula_authority: bool,
    packet_replay_required: bool,
    exact_row_selection_requires_packet_proof: bool,
    curve_evaluation_requires_packet_proof: bool,
    rounding_requires_packet_proof: bool,
    cross_stage_ordering_requires_packet_proof: bool,
    unresolved_fields_hidden: bool,
    retained_table_values: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fight attribute transform surface failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 6 {
        return Err(usage().into());
    }
    let table_path = PathBuf::from(option(&arguments, "--table")?);
    let output_path = PathBuf::from(option(&arguments, "--output")?);
    let game_build = option(&arguments, "--build")?.to_owned();
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".into());
    }

    let table: Value = serde_json::from_reader(BufReader::new(File::open(&table_path)?))?;
    let rows = validate_table(&table)?;
    let surface = TransformSurface {
        schema_version: SCHEMA_VERSION,
        game_build,
        generated_by: "rlogs-bpsr-fight-attribute-transform-surface",
        promotion_state: "research-only-current-build-packet-replay-required",
        source: source_artifact(&table_path)?,
        table: TableIdentity {
            name: "FightAttrTranTable",
            row_count: rows.len(),
            expected_primary_keys: vec![1, 2, 3],
            all_primary_keys_match_object_keys: true,
            all_required_fields_present: true,
            all_required_field_types_valid: true,
        },
        categories: FieldCategories {
            probability_and_stat_conversion: vec![
                "CriToCrit",
                "HasteToHastePct",
                "LuckToLuckyStrikeProb",
                "VersatilityToVersatilityPct",
                "MasteryToMasteryPct",
                "BlockToBlockRate",
            ],
            mitigation_and_power_conversion: vec![
                "DefPara",
                "HitPara",
                "ElementPowerToDam",
                "ElementDefToDamRes",
                "PhyPowerToDam",
                "MagPowerToDam",
                "RefDefPara",
            ],
            encounter_and_season_scaling: vec![
                "MstToPlayerDmgUp",
                "PlayerToMstDmgDown",
                "SeasonMstToPlayerDmg",
                "SeasonPlayerToMstDmg",
                "SeasonPlayerToMstBk",
                "SeasonHeal",
            ],
            attenuation_caps_and_threat: vec![
                "AoeAttenuation",
                "SingleAttenuation",
                "HateRate",
                "MaxDmg",
            ],
        },
        policy: SurfacePolicy {
            runtime_formula_authority: false,
            packet_replay_required: true,
            exact_row_selection_requires_packet_proof: true,
            curve_evaluation_requires_packet_proof: true,
            rounding_requires_packet_proof: true,
            cross_stage_ordering_requires_packet_proof: true,
            unresolved_fields_hidden: false,
            retained_table_values: "every decoded row and field is retained verbatim",
        },
        rows,
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&output_path)?);
    serde_json::to_writer_pretty(&mut writer, &surface)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "wrote {} FightAttrTranTable rows for build {} to {}",
        surface.rows.len(),
        surface.game_build,
        output_path.display()
    );
    Ok(())
}

fn validate_table(table: &Value) -> Result<BTreeMap<String, Value>, String> {
    let object = table
        .as_object()
        .ok_or_else(|| "FightAttrTranTable root must be an object".to_owned())?;
    if object.len() != EXPECTED_ROW_COUNT {
        return Err(format!(
            "FightAttrTranTable must contain exactly {EXPECTED_ROW_COUNT} rows, observed {}",
            object.len()
        ));
    }

    let mut rows = BTreeMap::new();
    for expected_id in 1_i64..=EXPECTED_ROW_COUNT as i64 {
        let key = expected_id.to_string();
        let row = object
            .get(&key)
            .ok_or_else(|| format!("FightAttrTranTable row {key} is missing"))?;
        validate_row(&key, row)?;
        rows.insert(key, row.clone());
    }
    Ok(rows)
}

fn validate_row(key: &str, row: &Value) -> Result<(), String> {
    let object = row
        .as_object()
        .ok_or_else(|| format!("FightAttrTranTable row {key} must be an object"))?;
    let id = object
        .get("Id")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("FightAttrTranTable row {key} has no integer Id"))?;
    if id.to_string() != key {
        return Err(format!(
            "FightAttrTranTable row key {key} does not match Id {id}"
        ));
    }
    for field in ARRAY_FIELDS {
        let value = required(object, key, field)?;
        if !value.is_array() || !numeric_tree(value) {
            return Err(format!(
                "FightAttrTranTable row {key} field {field} must be a numeric array tree"
            ));
        }
    }
    for field in NUMBER_FIELDS {
        if !required(object, key, field)?.is_number() {
            return Err(format!(
                "FightAttrTranTable row {key} field {field} must be numeric"
            ));
        }
    }
    Ok(())
}

fn required<'a>(
    object: &'a Map<String, Value>,
    row: &str,
    field: &str,
) -> Result<&'a Value, String> {
    object
        .get(field)
        .ok_or_else(|| format!("FightAttrTranTable row {row} field {field} is missing"))
}

fn numeric_tree(value: &Value) -> bool {
    match value {
        Value::Number(_) => true,
        Value::Array(values) => values.iter().all(numeric_tree),
        _ => false,
    }
}

fn source_artifact(path: &Path) -> Result<SourceArtifact, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    Ok(SourceArtifact {
        file: path.to_string_lossy().replace('\\', "/"),
        bytes: bytes.len() as u64,
        sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
    })
}

fn option<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    let position = arguments
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(usage)?;
    arguments
        .get(position + 1)
        .map(String::as_str)
        .ok_or_else(usage)
}

fn usage() -> String {
    "usage: rlogs-bpsr-fight-attribute-transform-surface --table <FightAttrTranTable.json> --build <numeric> --output <report.json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(id: i64) -> Value {
        let mut object = Map::new();
        object.insert("Id".to_owned(), json!(id));
        for field in ARRAY_FIELDS {
            object.insert((*field).to_owned(), json!([1, 2.0, [3]]));
        }
        for field in NUMBER_FIELDS {
            object.insert((*field).to_owned(), json!(1.0));
        }
        Value::Object(object)
    }

    fn table() -> Value {
        json!({"1": row(1), "2": row(2), "3": row(3)})
    }

    #[test]
    fn accepts_the_complete_three_row_shape() {
        let rows = validate_table(&table()).expect("valid table");
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn rejects_a_missing_required_field() {
        let mut table = table();
        table["2"].as_object_mut().expect("row").remove("DefPara");
        let error = validate_table(&table).expect_err("missing field must fail");
        assert!(error.contains("row 2 field DefPara is missing"));
    }

    #[test]
    fn rejects_a_primary_key_mismatch() {
        let mut table = table();
        table["3"]["Id"] = json!(4);
        let error = validate_table(&table).expect_err("key mismatch must fail");
        assert!(error.contains("row key 3 does not match Id 4"));
    }

    #[test]
    fn rejects_non_numeric_nested_values() {
        let mut table = table();
        table["1"]["SeasonHeal"] = json!([[51.0, "not-a-number"]]);
        let error = validate_table(&table).expect_err("bad numeric tree must fail");
        assert!(error.contains("field SeasonHeal must be a numeric array tree"));
    }
}
