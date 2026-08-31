use std::{
    collections::BTreeSet,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    baseline_root: PathBuf,
    candidate_root: PathBuf,
    diff: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct DecodedTableDiff {
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    baseline_build_id: String,
    build_id: String,
    tables: Vec<TableDiff>,
}

#[derive(Debug, Deserialize)]
struct TableDiff {
    table_key: u32,
    stable_key: String,
    baseline_sha256: Option<String>,
    candidate_sha256: Option<String>,
    changed_row_ids: Vec<String>,
    added_row_ids: Vec<String>,
    removed_row_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FieldDiffCatalog {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    baseline_build_id: String,
    build_id: String,
    policy: Policy,
    summary: Summary,
    tables: Vec<TableFieldDiff>,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_table_key_required: bool,
    unchanged_rows_embedded: bool,
    absolute_paths_embedded: bool,
    unresolved_changes_hidden: bool,
    changed_rows_auto_promoted: bool,
    added_or_removed_rows_require_separate_review: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    tables_reviewed: usize,
    tables_with_semantic_changes: usize,
    tables_without_semantic_changes: usize,
    changed_rows_reviewed: usize,
    changed_fields: usize,
}

#[derive(Debug, Serialize)]
struct TableFieldDiff {
    table_key: u32,
    table_key_hex: String,
    stable_key: String,
    decoded_table_name: String,
    baseline_sha256: Option<String>,
    candidate_sha256: Option<String>,
    semantic_change: bool,
    changed_row_count: usize,
    changed_field_count: usize,
    rows: Vec<RowFieldDiff>,
}

#[derive(Debug, Serialize)]
struct RowFieldDiff {
    row_id: String,
    fields: Vec<FieldChange>,
}

#[derive(Debug, PartialEq, Serialize)]
struct FieldChange {
    path: String,
    baseline: Option<Value>,
    candidate: Option<Value>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("decoded row field diff failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    if arguments.output.exists() {
        return Err(format!(
            "refusing to overwrite existing output {}",
            arguments.output.display()
        )
        .into());
    }
    let diff: DecodedTableDiff = serde_json::from_slice(&fs::read(&arguments.diff)?)?;
    let (tables, summary) = review_tables(
        &arguments.baseline_root,
        &arguments.candidate_root,
        diff.tables,
    )?;
    let output = FieldDiffCatalog {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-decoded-row-field-diff",
        game: "blue-protocol-star-resonance",
        deployment_id: diff.deployment_id,
        channel: diff.channel,
        distribution_app_id: diff.distribution_app_id,
        baseline_build_id: diff.baseline_build_id,
        build_id: diff.build_id,
        policy: Policy {
            exact_table_key_required: true,
            unchanged_rows_embedded: false,
            absolute_paths_embedded: false,
            unresolved_changes_hidden: false,
            changed_rows_auto_promoted: false,
            added_or_removed_rows_require_separate_review: true,
            promotion_requirement: "matching-build semantic classification, packet replay, and exact conservation proof",
        },
        summary,
        tables,
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &output)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn review_tables(
    baseline_root: &Path,
    candidate_root: &Path,
    mut diffs: Vec<TableDiff>,
) -> Result<(Vec<TableFieldDiff>, Summary), Box<dyn std::error::Error>> {
    diffs.sort_by_key(|table| table.table_key);
    let mut summary = Summary::default();
    let mut tables = Vec::with_capacity(diffs.len());
    for diff in diffs {
        if !diff.added_row_ids.is_empty() || !diff.removed_row_ids.is_empty() {
            return Err(format!(
                "{} has added or removed rows; use a separate row-presence review",
                diff.stable_key
            )
            .into());
        }
        let table_name = decoded_table_name(&diff.stable_key)?;
        let baseline = read_table(baseline_root, &table_name)?;
        let candidate = read_table(candidate_root, &table_name)?;
        let mut rows = Vec::with_capacity(diff.changed_row_ids.len());
        let mut changed_field_count = 0_usize;
        for row_id in &diff.changed_row_ids {
            let old = baseline
                .get(row_id)
                .ok_or_else(|| format!("baseline {table_name} is missing changed row {row_id}"))?;
            let new = candidate
                .get(row_id)
                .ok_or_else(|| format!("candidate {table_name} is missing changed row {row_id}"))?;
            let mut fields = Vec::new();
            diff_value("", Some(old), Some(new), &mut fields);
            if fields.is_empty() {
                return Err(format!(
                    "{table_name} row {row_id} was marked changed but has no field changes"
                )
                .into());
            }
            changed_field_count += fields.len();
            rows.push(RowFieldDiff {
                row_id: row_id.clone(),
                fields,
            });
        }
        let semantic_change = !rows.is_empty();
        summary.tables_reviewed += 1;
        summary.changed_rows_reviewed += rows.len();
        summary.changed_fields += changed_field_count;
        if semantic_change {
            summary.tables_with_semantic_changes += 1;
        } else {
            summary.tables_without_semantic_changes += 1;
        }
        tables.push(TableFieldDiff {
            table_key: diff.table_key,
            table_key_hex: format!("0x{:08x}", diff.table_key),
            stable_key: diff.stable_key,
            decoded_table_name: table_name,
            baseline_sha256: diff.baseline_sha256,
            candidate_sha256: diff.candidate_sha256,
            semantic_change,
            changed_row_count: rows.len(),
            changed_field_count,
            rows,
        });
    }
    Ok((tables, summary))
}

fn decoded_table_name(stable_key: &str) -> Result<String, Box<dyn std::error::Error>> {
    let name = stable_key
        .strip_prefix("ctb.")
        .ok_or_else(|| format!("stable key {stable_key} is not a CTB identity"))?;
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(format!("unsafe decoded table name in {stable_key}").into());
    }
    Ok(name.to_owned())
}

fn read_table(
    root: &Path,
    table_name: &str,
) -> Result<Map<String, Value>, Box<dyn std::error::Error>> {
    let path = root.join(format!("{table_name}.json"));
    let decoded: Value = serde_json::from_slice(&fs::read(&path)?)?;
    decoded
        .as_object()
        .cloned()
        .ok_or_else(|| format!("decoded table {table_name} is not a JSON object").into())
}

fn diff_value(
    path: &str,
    baseline: Option<&Value>,
    candidate: Option<&Value>,
    output: &mut Vec<FieldChange>,
) {
    if baseline == candidate {
        return;
    }
    match (baseline, candidate) {
        (Some(Value::Object(old)), Some(Value::Object(new))) => {
            let keys = old
                .keys()
                .chain(new.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for key in keys {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                diff_value(&child, old.get(&key), new.get(&key), output);
            }
        }
        // Arrays are retained as exact values. Index-only diffs lose ordering context.
        _ => output.push(FieldChange {
            path: path.to_owned(),
            baseline: baseline.cloned(),
            candidate: candidate.cloned(),
        }),
    }
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut baseline_root = None;
    let mut candidate_root = None;
    let mut diff = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--baseline-root" => {
                baseline_root = Some(PathBuf::from(next_value(&mut args, "--baseline-root")?))
            }
            "--candidate-root" => {
                candidate_root = Some(PathBuf::from(next_value(&mut args, "--candidate-root")?))
            }
            "--diff" => diff = Some(PathBuf::from(next_value(&mut args, "--diff")?)),
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        baseline_root: baseline_root.ok_or("missing --baseline-root")?,
        candidate_root: candidate_root.ok_or("missing --candidate-root")?,
        diff: diff.ok_or("missing --diff")?,
        output: output.ok_or("missing --output")?,
    })
}

fn next_value(
    args: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> Result<OsString, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_objects_emit_exact_leaf_changes_and_arrays_stay_whole() {
        let old = json!({"scalar": 1, "nested": {"same": 2, "changed": 3}, "list": [0, 0, 0]});
        let new = json!({"scalar": 1, "nested": {"same": 2, "changed": 4}, "list": [0, 0, 20]});
        let mut changes = Vec::new();
        diff_value("", Some(&old), Some(&new), &mut changes);
        assert_eq!(
            changes,
            vec![
                FieldChange {
                    path: "list".to_owned(),
                    baseline: Some(json!([0, 0, 0])),
                    candidate: Some(json!([0, 0, 20])),
                },
                FieldChange {
                    path: "nested.changed".to_owned(),
                    baseline: Some(json!(3)),
                    candidate: Some(json!(4)),
                },
            ]
        );
    }

    #[test]
    fn added_and_removed_fields_are_never_hidden() {
        let old = json!({"removed": 1});
        let new = json!({"added": 2});
        let mut changes = Vec::new();
        diff_value("", Some(&old), Some(&new), &mut changes);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, "added");
        assert_eq!(changes[0].baseline, None);
        assert_eq!(changes[1].path, "removed");
        assert_eq!(changes[1].candidate, None);
    }

    #[test]
    fn stable_identity_cannot_escape_decoded_root() {
        assert_eq!(decoded_table_name("ctb.SkillTable").unwrap(), "SkillTable");
        assert!(decoded_table_name("ctb...\\secret").is_err());
        assert!(decoded_table_name("SkillTable").is_err());
    }
}
