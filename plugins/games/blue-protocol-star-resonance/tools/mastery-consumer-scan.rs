use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    profession: PathBuf,
    talent_stage: PathBuf,
    baseline_profession: Option<PathBuf>,
    game_build: String,
    output: PathBuf,
    overwrite: bool,
}

#[derive(Debug, Serialize)]
struct Inventory {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    game_build: String,
    promotion_state: &'static str,
    policy: Policy,
    inputs: Inputs,
    summary: Summary,
    consumers: Vec<Consumer>,
    unpaired_descriptors: Vec<UnpairedDescriptor>,
    unpaired_talent_stages: Vec<UnpairedTalentStage>,
}

#[derive(Debug, Serialize)]
struct Policy {
    localized_descriptions_are_runtime_authority: bool,
    changed_descriptions_auto_promote_formulas: bool,
    unresolved_descriptors_hidden: bool,
    matching_build_packet_replay_required: bool,
    old_logs_reinterpreted_by_new_build: bool,
    purpose: &'static str,
}

#[derive(Debug, Serialize)]
struct Inputs {
    profession_system_table: SourceFile,
    talent_stage_table: SourceFile,
    baseline_profession_system_table: Option<SourceFile>,
}

#[derive(Debug, Serialize)]
struct SourceFile {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Summary {
    profession_rows: usize,
    paired_consumers: usize,
    text_changed_from_baseline: usize,
    text_unchanged_from_baseline: usize,
    numeric_literals_changed_from_baseline: usize,
    numeric_literals_unchanged_from_baseline: usize,
    descriptions_without_baseline: usize,
    unpaired_descriptors: usize,
    unpaired_talent_stages: usize,
}

#[derive(Debug, Serialize)]
struct Consumer {
    profession_id: i64,
    profession_name: String,
    element_id: Option<i64>,
    descriptor_index: usize,
    talent_stage_id: i64,
    talent_stage_name: String,
    talent_stage_name_raw: Value,
    bd_type: Option<i64>,
    attack_attribute_id: Option<i64>,
    primary_attribute_id: Option<i64>,
    description: String,
    numeric_literals: Vec<String>,
    baseline_description: Option<String>,
    baseline_numeric_literals: Option<Vec<String>>,
    text_changed_from_baseline: Option<bool>,
    numeric_literals_changed_from_baseline: Option<bool>,
    evidence_state: &'static str,
    runtime_transfer_enabled: bool,
    required_next_evidence: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct UnpairedDescriptor {
    profession_id: i64,
    profession_name: String,
    descriptor_index: usize,
    description: String,
    reason: &'static str,
}

#[derive(Debug, Serialize)]
struct UnpairedTalentStage {
    profession_id: i64,
    profession_name: String,
    descriptor_index: usize,
    talent_stage_id: i64,
    reason: String,
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(values: Vec<String>) -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(values)?;
    if arguments.output.exists() && !arguments.overwrite {
        return Err(format!(
            "output already exists: {} (pass --overwrite to replace it)",
            arguments.output.display()
        )
        .into());
    }

    let profession_bytes = fs::read(&arguments.profession)?;
    let talent_bytes = fs::read(&arguments.talent_stage)?;
    let profession: Value = serde_json::from_slice(&profession_bytes)?;
    let talent_stage: Value = serde_json::from_slice(&talent_bytes)?;
    let baseline_bytes = arguments
        .baseline_profession
        .as_ref()
        .map(fs::read)
        .transpose()?;
    let baseline = baseline_bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()?;

    let inventory = build_inventory(
        &arguments,
        &profession,
        &talent_stage,
        baseline.as_ref(),
        &profession_bytes,
        &talent_bytes,
        baseline_bytes.as_deref(),
    )?;

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(&arguments.output)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &inventory)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "wrote {} paired Mastery consumers to {}",
        inventory.summary.paired_consumers,
        arguments.output.display()
    );
    Ok(())
}

fn build_inventory(
    arguments: &Arguments,
    profession: &Value,
    talent_stage: &Value,
    baseline: Option<&Value>,
    profession_bytes: &[u8],
    talent_bytes: &[u8],
    baseline_bytes: Option<&[u8]>,
) -> Result<Inventory, Box<dyn Error>> {
    let professions = profession
        .as_object()
        .ok_or("ProfessionSystemTable root must be an object")?;
    let talent_stages = talent_stage
        .as_object()
        .ok_or("TalentStageTable root must be an object")?;
    let baseline_professions = baseline.and_then(Value::as_object);

    let mut profession_rows = professions.values().collect::<Vec<_>>();
    profession_rows.sort_by_key(|row| integer(row, "Id").unwrap_or(i64::MAX));

    let mut consumers = Vec::new();
    let mut unpaired_descriptors = Vec::new();
    let mut paired_stage_ids = BTreeSet::new();

    for row in &profession_rows {
        let profession_id = integer(row, "Id").ok_or("profession row is missing Id")?;
        let profession_name = string(row, "Name")
            .unwrap_or("Unnamed profession")
            .to_owned();
        let element_id = integer(row, "Element");
        let stages = integer_array(row.get("ShowTalentStage"));
        let descriptions = mastery_descriptions(row);
        let baseline_descriptions = baseline_professions
            .and_then(|rows| rows.get(&profession_id.to_string()))
            .map(mastery_descriptions)
            .unwrap_or_default();
        let attack_by_bd_type = keyed_attribute_map(row.get("AttackShow"));
        let primary_by_bd_type = keyed_attribute_map(row.get("StrOrIntOrDexShow"));

        for (descriptor_index, description) in descriptions.iter().enumerate() {
            let Some(talent_stage_id) = stages.get(descriptor_index).copied() else {
                unpaired_descriptors.push(UnpairedDescriptor {
                    profession_id,
                    profession_name: profession_name.clone(),
                    descriptor_index,
                    description: description.clone(),
                    reason: "MasteryDes has no same-index ShowTalentStage entry",
                });
                continue;
            };
            let Some(stage) = talent_stages.get(&talent_stage_id.to_string()) else {
                unpaired_descriptors.push(UnpairedDescriptor {
                    profession_id,
                    profession_name: profession_name.clone(),
                    descriptor_index,
                    description: description.clone(),
                    reason: "ShowTalentStage ID is absent from TalentStageTable",
                });
                continue;
            };
            paired_stage_ids.insert(talent_stage_id);
            let bd_type = integer(stage, "BdType");
            let stage_name_raw = stage.get("Name").cloned().unwrap_or(Value::Null);
            let stage_name = display_name(&stage_name_raw)
                .unwrap_or_else(|| format!("Talent stage {talent_stage_id}"));
            let baseline_description = baseline_descriptions.get(descriptor_index).cloned();
            let text_changed_from_baseline = baseline_description
                .as_ref()
                .map(|baseline| normalize_markup(baseline) != normalize_markup(description));
            let current_numeric_literals = numeric_literals(description);
            let baseline_numeric_literals = baseline_description
                .as_ref()
                .map(|description| numeric_literals(description));
            let numeric_literals_changed_from_baseline = baseline_numeric_literals
                .as_ref()
                .map(|baseline| baseline != &current_numeric_literals);

            consumers.push(Consumer {
                profession_id,
                profession_name: profession_name.clone(),
                element_id,
                descriptor_index,
                talent_stage_id,
                talent_stage_name: stage_name,
                talent_stage_name_raw: stage_name_raw,
                bd_type,
                attack_attribute_id: bd_type.and_then(|key| attack_by_bd_type.get(&key).copied()),
                primary_attribute_id: bd_type.and_then(|key| primary_by_bd_type.get(&key).copied()),
                description: description.clone(),
                numeric_literals: current_numeric_literals,
                baseline_description,
                baseline_numeric_literals,
                text_changed_from_baseline,
                numeric_literals_changed_from_baseline,
                evidence_state: "static-client-description-only",
                runtime_transfer_enabled: false,
                required_next_evidence: vec![
                    "matching-build packet attribute window",
                    "matching-build damage or action consumer correlation",
                    "exact provider-removed counterfactual with conservation",
                ],
            });
        }
    }

    let mut unpaired_talent_stages = Vec::new();
    for row in &profession_rows {
        let profession_id = integer(row, "Id").ok_or("profession row is missing Id")?;
        let profession_name = string(row, "Name")
            .unwrap_or("Unnamed profession")
            .to_owned();
        for (descriptor_index, talent_stage_id) in integer_array(row.get("ShowTalentStage"))
            .into_iter()
            .enumerate()
        {
            if !paired_stage_ids.contains(&talent_stage_id) {
                let reason = if talent_stages.contains_key(&talent_stage_id.to_string()) {
                    "ShowTalentStage has no same-index MasteryDes entry"
                } else {
                    "ShowTalentStage ID is absent from TalentStageTable"
                };
                unpaired_talent_stages.push(UnpairedTalentStage {
                    profession_id,
                    profession_name: profession_name.clone(),
                    descriptor_index,
                    talent_stage_id,
                    reason: reason.to_owned(),
                });
            }
        }
    }

    consumers.sort_by_key(|consumer| (consumer.profession_id, consumer.descriptor_index));
    unpaired_descriptors.sort_by_key(|entry| (entry.profession_id, entry.descriptor_index));
    unpaired_talent_stages.sort_by_key(|entry| (entry.profession_id, entry.descriptor_index));

    let changed = consumers
        .iter()
        .filter(|consumer| consumer.text_changed_from_baseline == Some(true))
        .count();
    let unchanged = consumers
        .iter()
        .filter(|consumer| consumer.text_changed_from_baseline == Some(false))
        .count();
    let numeric_changed = consumers
        .iter()
        .filter(|consumer| consumer.numeric_literals_changed_from_baseline == Some(true))
        .count();
    let numeric_unchanged = consumers
        .iter()
        .filter(|consumer| consumer.numeric_literals_changed_from_baseline == Some(false))
        .count();
    let without_baseline = consumers
        .iter()
        .filter(|consumer| consumer.text_changed_from_baseline.is_none())
        .count();

    Ok(Inventory {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-mastery-consumer-scan",
        game: "blue-protocol-star-resonance",
        game_build: arguments.game_build.clone(),
        promotion_state: "audit-only-no-runtime-authority",
        policy: Policy {
            localized_descriptions_are_runtime_authority: false,
            changed_descriptions_auto_promote_formulas: false,
            unresolved_descriptors_hidden: false,
            matching_build_packet_replay_required: true,
            old_logs_reinterpreted_by_new_build: false,
            purpose: "versioned discovery and drift detection for packet-proven Mastery consumers",
        },
        inputs: Inputs {
            profession_system_table: source_file(&arguments.profession, profession_bytes)?,
            talent_stage_table: source_file(&arguments.talent_stage, talent_bytes)?,
            baseline_profession_system_table: arguments
                .baseline_profession
                .as_ref()
                .zip(baseline_bytes)
                .map(|(path, bytes)| source_file(path, bytes))
                .transpose()?,
        },
        summary: Summary {
            profession_rows: profession_rows.len(),
            paired_consumers: consumers.len(),
            text_changed_from_baseline: changed,
            text_unchanged_from_baseline: unchanged,
            numeric_literals_changed_from_baseline: numeric_changed,
            numeric_literals_unchanged_from_baseline: numeric_unchanged,
            descriptions_without_baseline: without_baseline,
            unpaired_descriptors: unpaired_descriptors.len(),
            unpaired_talent_stages: unpaired_talent_stages.len(),
        },
        consumers,
        unpaired_descriptors,
        unpaired_talent_stages,
    })
}

fn mastery_descriptions(row: &Value) -> Vec<String> {
    row.get("MasteryDes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| {
            entry
                .as_array()
                .and_then(|parts| parts.iter().rev().find_map(Value::as_str))
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

fn keyed_attribute_map(value: Option<&Value>) -> BTreeMap<i64, i64> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let pair = entry.as_array()?;
            Some((pair.first()?.as_i64()?, pair.get(1)?.as_i64()?))
        })
        .collect()
}

fn integer_array(value: Option<&Value>) -> Vec<i64> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect()
}

fn display_name(value: &Value) -> Option<String> {
    match value {
        Value::String(name) => Some(name.clone()),
        Value::Array(names) => names
            .iter()
            .rev()
            .find_map(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn normalize_markup(value: &str) -> String {
    value
        .replace("<br>", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn numeric_literals(value: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        if character.is_ascii_digit() || (character == '.' && !current.is_empty()) {
            current.push(character);
        } else if !current.is_empty() {
            literals.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        literals.push(current);
    }
    literals
}

fn source_file(path: &Path, bytes: &[u8]) -> Result<SourceFile, Box<dyn Error>> {
    Ok(SourceFile {
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
        sha256: format!("sha256:{:x}", Sha256::digest(bytes)),
    })
}

fn parse_arguments(values: Vec<String>) -> Result<Arguments, Box<dyn Error>> {
    let mut options = Map::<String, Value>::new();
    let mut overwrite = false;
    let mut iterator = values.into_iter();
    while let Some(argument) = iterator.next() {
        if argument == "--overwrite" {
            overwrite = true;
            continue;
        }
        let Some(key) = argument.strip_prefix("--") else {
            return Err(format!("unexpected positional argument: {argument}").into());
        };
        let value = iterator
            .next()
            .ok_or_else(|| format!("missing value for --{key}"))?;
        options.insert(key.to_owned(), Value::String(value));
    }

    let required = |key: &str| -> Result<String, Box<dyn Error>> {
        options
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("missing --{key}\n{}", usage()).into())
    };
    let game_build = required("build")?;
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build must contain only decimal digits".into());
    }

    Ok(Arguments {
        profession: PathBuf::from(required("profession")?),
        talent_stage: PathBuf::from(required("talent-stage")?),
        baseline_profession: options
            .get("baseline-profession")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        game_build,
        output: PathBuf::from(required("output")?),
        overwrite,
    })
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-mastery-consumer-scan --profession <ProfessionSystemTable.json> --talent-stage <TalentStageTable.json> [--baseline-profession <older ProfessionSystemTable.json>] --build <digits> --output <inventory.json> [--overwrite]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mastery_descriptions_preserve_every_descriptor() {
        let row = serde_json::json!({"MasteryDes": [["", "one"], ["", "two"]]});
        assert_eq!(mastery_descriptions(&row), vec!["one", "two"]);
    }

    #[test]
    fn display_name_prefers_specialization_name() {
        assert_eq!(
            display_name(&serde_json::json!(["Expertise II", "Falconry Spec"])),
            Some("Falconry Spec".to_owned())
        );
    }

    #[test]
    fn markup_normalization_only_removes_formatting_differences() {
        assert_eq!(
            normalize_markup("Each 1% Mastery<br> grants 0.6% Light Bonus"),
            "Each 1% Mastery grants 0.6% Light Bonus"
        );
    }

    #[test]
    fn numeric_literals_distinguish_formula_drift_from_wording_drift() {
        assert_eq!(
            numeric_literals("Each 1% Mastery grants 0.65% Ice Bonus"),
            vec!["1", "0.65"]
        );
        assert_eq!(
            numeric_literals("Each 1% Expertise grants 0.65% Ice Element bonus"),
            vec!["1", "0.65"]
        );
        assert_ne!(
            numeric_literals("Each 1% Mastery grants 0.35% Wind Bonus"),
            numeric_literals("Each 1% Expertise grants 0.65% Wind Element bonus")
        );
    }
}
