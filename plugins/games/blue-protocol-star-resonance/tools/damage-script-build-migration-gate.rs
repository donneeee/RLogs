use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    worklist: PathBuf,
    ctb_diff: PathBuf,
    current_table: PathBuf,
    baseline_table: Option<PathBuf>,
    baseline_build: String,
    build: String,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct FamilyWorklist {
    schema_version: u16,
    game_build: String,
    summary: WorklistSummary,
    families: Vec<ScriptFamily>,
}

#[derive(Debug, Deserialize)]
struct WorklistSummary {
    candidate_rows: usize,
    formula_signatures: usize,
}

#[derive(Debug, Deserialize)]
struct ScriptFamily {
    damage_script: String,
    summary: FamilySummary,
    formula_signatures: Vec<SignatureGroup>,
}

#[derive(Debug, Deserialize)]
struct FamilySummary {
    candidate_rows: usize,
    formula_signatures: usize,
}

#[derive(Debug, Deserialize)]
struct SignatureGroup {
    work_items: Vec<WorkItem>,
}

#[derive(Debug, Deserialize)]
struct WorkItem {
    damage_attr: DamageAttrIdentity,
}

#[derive(Debug, Deserialize)]
struct DamageAttrIdentity {
    damage_attr_id: i64,
}

#[derive(Debug, Deserialize)]
struct CtbBuildDiff {
    schema_version: u16,
    baseline_build_id: String,
    build_id: String,
    changes: Vec<CtbChange>,
}

#[derive(Debug, Deserialize)]
struct CtbChange {
    stable_key: String,
    change: String,
    baseline: Option<CtbVersion>,
    current: Option<CtbVersion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CtbVersion {
    bytes: u64,
    sha256: String,
    shape: CtbShape,
}

type RawTableChangeResult = (String, Option<CtbVersion>, Option<CtbVersion>, i64);

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CtbShape {
    rows: usize,
    row_size: usize,
    row_data_bytes: usize,
    pool_lengths: Value,
}

#[derive(Debug, Serialize)]
struct MigrationGate {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    baseline_build_id: String,
    build_id: String,
    promotion_state: &'static str,
    inputs: Vec<InputArtifact>,
    policy: Policy,
    raw_table_change: RawTableChange,
    decoded_row_comparison: DecodedRowComparison,
    summary: Summary,
    families: Vec<FamilyMigration>,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    role: &'static str,
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Policy {
    historical_packet_evidence_is_current_formula_authority: bool,
    unchanged_decoded_row_is_server_operator_proof: bool,
    unresolved_evidence_hidden: bool,
    absent_baseline_is_treated_as_unchanged: bool,
    current_decoded_candidate_required: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct RawTableChange {
    stable_key: &'static str,
    change: String,
    baseline: Option<CtbVersion>,
    current: Option<CtbVersion>,
    net_row_change: i64,
}

#[derive(Debug, Serialize)]
struct DecodedRowComparison {
    baseline_decoded_table_available: bool,
    comparison_scope: &'static str,
    canonical_json_object_equality: bool,
    unchanged_candidate_rows: usize,
    changed_candidate_rows: usize,
    added_candidate_rows: usize,
    missing_current_candidate_rows: usize,
    uncomparable_candidate_rows: usize,
    changed_candidate_ids: Vec<i64>,
    added_candidate_ids: Vec<i64>,
    missing_current_candidate_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct Summary {
    script_families: usize,
    current_candidate_rows: usize,
    full_semantic_signatures: usize,
    historical_static_rows_eligible_as_replay_leads: usize,
    historical_static_rows_eligible_as_current_authority: usize,
    families_requiring_same_build_packet_replay: usize,
}

#[derive(Debug, Serialize)]
struct FamilyMigration {
    damage_script: String,
    current_candidate_rows: usize,
    full_semantic_signatures: usize,
    unchanged_decoded_rows: usize,
    changed_or_added_decoded_rows: usize,
    uncomparable_decoded_rows: usize,
    historical_evidence_role: &'static str,
    current_formula_state: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("damage script build-migration gate failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }
    let worklist: FamilyWorklist = read_json(&args.worklist)?;
    let ctb_diff: CtbBuildDiff = read_json(&args.ctb_diff)?;
    validate_headers(&worklist, &ctb_diff, &args)?;

    let candidate_ids_by_family = candidate_ids_by_family(&worklist)?;
    let all_candidate_ids = candidate_ids_by_family
        .values()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    if all_candidate_ids.len() != worklist.summary.candidate_rows {
        return Err("candidate DamageAttr IDs are not globally unique and conserved".into());
    }

    let current_rows = decoded_rows(&args.current_table)?;
    let baseline_rows = args
        .baseline_table
        .as_ref()
        .map(|path| decoded_rows(path))
        .transpose()?;
    let damage_change = ctb_diff
        .changes
        .iter()
        .find(|change| change.stable_key == "ctb.DamageAttrTable");
    let (raw_change, baseline_ctb, current_ctb, net_row_change) =
        raw_table_change(damage_change, &current_rows, baseline_rows.as_ref())?;
    let comparison = compare_rows(&all_candidate_ids, &current_rows, baseline_rows.as_ref());
    if !comparison.missing_current_candidate_ids.is_empty() {
        return Err(format!(
            "{} worklist candidates are absent from current decoded DamageAttrTable",
            comparison.missing_current_candidate_ids.len()
        )
        .into());
    }

    let unchanged = comparison
        .changed_candidate_ids
        .iter()
        .chain(comparison.added_candidate_ids.iter())
        .copied()
        .collect::<BTreeSet<_>>();
    let families = worklist
        .families
        .iter()
        .map(|family| {
            let ids = candidate_ids_by_family
                .get(&family.damage_script)
                .expect("validated family IDs");
            let changed_or_added = ids.intersection(&unchanged).count();
            let comparable = baseline_rows.is_some();
            let unchanged_count = if comparable {
                ids.len() - changed_or_added
            } else {
                0
            };
            FamilyMigration {
                damage_script: family.damage_script.clone(),
                current_candidate_rows: family.summary.candidate_rows,
                full_semantic_signatures: family.summary.formula_signatures,
                unchanged_decoded_rows: unchanged_count,
                changed_or_added_decoded_rows: changed_or_added,
                uncomparable_decoded_rows: if comparable { 0 } else { ids.len() },
                historical_evidence_role: "replay-test-design-and-regression-oracle-only",
                current_formula_state: "same-build-packet-replay-and-server-operator-proof-required",
            }
        })
        .collect::<Vec<_>>();

    let mut inputs = vec![
        input_artifact("current-build-damage-script-worklist", &args.worklist)?,
        input_artifact("raw-ctb-build-diff", &args.ctb_diff)?,
        input_artifact("current-decoded-DamageAttrTable", &args.current_table)?,
    ];
    if let Some(path) = &args.baseline_table {
        inputs.push(input_artifact("baseline-decoded-DamageAttrTable", path)?);
    }

    let baseline_available = baseline_rows.is_some();
    let report = MigrationGate {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-damage-script-build-migration-gate",
        game: "blue-protocol-star-resonance",
        baseline_build_id: args.baseline_build,
        build_id: args.build,
        promotion_state: "blocked-until-same-build-packet-replay-and-server-operator-proof",
        inputs,
        policy: Policy {
            historical_packet_evidence_is_current_formula_authority: false,
            unchanged_decoded_row_is_server_operator_proof: false,
            unresolved_evidence_hidden: false,
            absent_baseline_is_treated_as_unchanged: false,
            current_decoded_candidate_required: true,
            promotion_requirement: "exact current decoded row selection plus sealed same-build packet occurrence, operator-stage proof, provider/recipient lifecycle, and counterfactual conservation",
        },
        raw_table_change: RawTableChange {
            stable_key: "ctb.DamageAttrTable",
            change: raw_change,
            net_row_change,
            baseline: baseline_ctb,
            current: current_ctb,
        },
        decoded_row_comparison: comparison,
        summary: Summary {
            script_families: families.len(),
            current_candidate_rows: worklist.summary.candidate_rows,
            full_semantic_signatures: worklist.summary.formula_signatures,
            historical_static_rows_eligible_as_replay_leads: worklist.summary.candidate_rows,
            historical_static_rows_eligible_as_current_authority: 0,
            families_requiring_same_build_packet_replay: families.len(),
        },
        families,
    };
    if !baseline_available && report.decoded_row_comparison.uncomparable_candidate_rows == 0 {
        return Err("absent baseline decoded table was not retained as uncomparable".into());
    }
    write_json(&args.output, &report)
}

fn raw_table_change(
    change: Option<&CtbChange>,
    current_rows: &BTreeMap<i64, Value>,
    baseline_rows: Option<&BTreeMap<i64, Value>>,
) -> Result<RawTableChangeResult, Box<dyn std::error::Error>> {
    if let Some(change) = change {
        if change.change != "changed" {
            return Err("listed DamageAttrTable must be explicitly classified as changed".into());
        }
        let baseline = change
            .baseline
            .clone()
            .ok_or("DamageAttrTable baseline CTB identity is absent")?;
        let current = change
            .current
            .clone()
            .ok_or("DamageAttrTable current CTB identity is absent")?;
        let net_row_change = current.shape.rows as i64 - baseline.shape.rows as i64;
        return Ok((
            change.change.clone(),
            Some(baseline),
            Some(current),
            net_row_change,
        ));
    }

    let baseline_rows = baseline_rows.ok_or(
        "DamageAttrTable is absent from the change-only CTB diff; an exact baseline decoded table is required",
    )?;
    if baseline_rows != current_rows {
        return Err(
            "DamageAttrTable is absent from the change-only CTB diff but the full decoded tables differ"
                .into(),
        );
    }
    Ok((
        "unchanged-full-decoded-table-equality".to_owned(),
        None,
        None,
        0,
    ))
}

fn validate_headers(
    worklist: &FamilyWorklist,
    ctb_diff: &CtbBuildDiff,
    args: &Arguments,
) -> Result<(), Box<dyn std::error::Error>> {
    if worklist.schema_version != 2 {
        return Err(format!(
            "unsupported family-worklist schema {}",
            worklist.schema_version
        )
        .into());
    }
    if ctb_diff.schema_version != 1 {
        return Err(format!("unsupported CTB-diff schema {}", ctb_diff.schema_version).into());
    }
    if worklist.game_build != args.build || ctb_diff.build_id != args.build {
        return Err("candidate build identity mismatch".into());
    }
    if ctb_diff.baseline_build_id != args.baseline_build {
        return Err("baseline build identity mismatch".into());
    }
    Ok(())
}

fn candidate_ids_by_family(
    worklist: &FamilyWorklist,
) -> Result<BTreeMap<String, BTreeSet<i64>>, Box<dyn std::error::Error>> {
    let mut result = BTreeMap::new();
    for family in &worklist.families {
        let mut ids = BTreeSet::new();
        for group in &family.formula_signatures {
            for item in &group.work_items {
                if !ids.insert(item.damage_attr.damage_attr_id) {
                    return Err(format!(
                        "duplicate DamageAttr {} inside family {}",
                        item.damage_attr.damage_attr_id, family.damage_script
                    )
                    .into());
                }
            }
        }
        if ids.len() != family.summary.candidate_rows {
            return Err(format!(
                "family {} does not conserve candidate rows",
                family.damage_script
            )
            .into());
        }
        if result.insert(family.damage_script.clone(), ids).is_some() {
            return Err(format!("duplicate script family {}", family.damage_script).into());
        }
    }
    Ok(result)
}

fn decoded_rows(path: &Path) -> Result<BTreeMap<i64, Value>, Box<dyn std::error::Error>> {
    let value: Value = read_json(path)?;
    let object = value.as_object().ok_or_else(|| {
        format!(
            "decoded table {} is not an ID-keyed JSON object",
            path.display()
        )
    })?;
    let mut rows = BTreeMap::new();
    for (key, row) in object {
        let id = key.parse::<i64>()?;
        let row_id = row
            .get("Id")
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("decoded row {key} has no integer Id"))?;
        if id != row_id {
            return Err(format!("decoded row key {id} disagrees with Id {row_id}").into());
        }
        rows.insert(id, canonicalize_json(row));
    }
    Ok(rows)
}

fn compare_rows(
    candidate_ids: &BTreeSet<i64>,
    current: &BTreeMap<i64, Value>,
    baseline: Option<&BTreeMap<i64, Value>>,
) -> DecodedRowComparison {
    let mut changed = Vec::new();
    let mut added = Vec::new();
    let mut missing_current = Vec::new();
    let mut unchanged = 0_usize;
    for id in candidate_ids {
        let Some(current_row) = current.get(id) else {
            missing_current.push(*id);
            continue;
        };
        let Some(baseline) = baseline else {
            continue;
        };
        match baseline.get(id) {
            Some(old) if old == current_row => unchanged += 1,
            Some(_) => changed.push(*id),
            None => added.push(*id),
        }
    }
    DecodedRowComparison {
        baseline_decoded_table_available: baseline.is_some(),
        comparison_scope: "current nonstandard-or-missing-script DamageAttr candidates only",
        canonical_json_object_equality: baseline.is_some(),
        unchanged_candidate_rows: unchanged,
        changed_candidate_rows: changed.len(),
        added_candidate_rows: added.len(),
        missing_current_candidate_rows: missing_current.len(),
        uncomparable_candidate_rows: if baseline.is_none() {
            candidate_ids.len() - missing_current.len()
        } else {
            0
        },
        changed_candidate_ids: changed,
        added_candidate_ids: added,
        missing_current_candidate_ids: missing_current,
    }
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_json).collect()),
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonicalize_json(&object[key]));
            }
            Value::Object(canonical)
        }
        other => other.clone(),
    }
}

fn input_artifact(
    role: &'static str,
    path: &Path,
) -> Result<InputArtifact, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    Ok(InputArtifact {
        role,
        file: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<non-utf8>")
            .to_owned(),
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut values = env::args_os().skip(1);
    let mut args = BTreeMap::<OsArg, PathBuf>::new();
    while let Some(flag) = values.next() {
        let flag = flag.to_string_lossy();
        let key = match flag.as_ref() {
            "--worklist" => OsArg::Worklist,
            "--ctb-diff" => OsArg::CtbDiff,
            "--current-table" => OsArg::CurrentTable,
            "--baseline-table" => OsArg::BaselineTable,
            "--baseline-build" => OsArg::BaselineBuild,
            "--build" => OsArg::Build,
            "--output" => OsArg::Output,
            _ => return Err(format!("unknown argument {flag}").into()),
        };
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        args.insert(key, PathBuf::from(value));
    }
    let string_arg = |key: OsArg, name: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(args
            .get(&key)
            .ok_or_else(|| format!("missing {name}"))?
            .to_string_lossy()
            .into_owned())
    };
    Ok(Arguments {
        worklist: required_path(&args, OsArg::Worklist, "--worklist")?,
        ctb_diff: required_path(&args, OsArg::CtbDiff, "--ctb-diff")?,
        current_table: required_path(&args, OsArg::CurrentTable, "--current-table")?,
        baseline_table: args.get(&OsArg::BaselineTable).cloned(),
        baseline_build: string_arg(OsArg::BaselineBuild, "--baseline-build")?,
        build: string_arg(OsArg::Build, "--build")?,
        output: required_path(&args, OsArg::Output, "--output")?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OsArg {
    Worklist,
    CtbDiff,
    CurrentTable,
    BaselineTable,
    BaselineBuild,
    Build,
    Output,
}

fn required_path(
    args: &BTreeMap<OsArg, PathBuf>,
    key: OsArg,
    name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    args.get(&key)
        .cloned()
        .ok_or_else(|| format!("missing {name}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_object_order_is_stable() {
        let left = serde_json::json!({"b": 2, "a": {"z": 1, "y": 0}});
        let right = serde_json::json!({"a": {"y": 0, "z": 1}, "b": 2});
        assert_eq!(canonicalize_json(&left), canonicalize_json(&right));
    }

    #[test]
    fn absent_baseline_never_claims_unchanged_rows() {
        let ids = BTreeSet::from([1, 2]);
        let current = BTreeMap::from([
            (1, serde_json::json!({"Id": 1})),
            (2, serde_json::json!({"Id": 2})),
        ]);
        let result = compare_rows(&ids, &current, None);
        assert_eq!(result.unchanged_candidate_rows, 0);
        assert_eq!(result.uncomparable_candidate_rows, 2);
        assert!(!result.baseline_decoded_table_available);
    }

    #[test]
    fn absent_change_requires_full_decoded_table_equality() {
        let current = BTreeMap::from([(1, serde_json::json!({"Id": 1, "Value": 10}))]);
        let baseline = current.clone();
        let (change, baseline_ctb, current_ctb, net_rows) =
            raw_table_change(None, &current, Some(&baseline)).unwrap();
        assert_eq!(change, "unchanged-full-decoded-table-equality");
        assert!(baseline_ctb.is_none());
        assert!(current_ctb.is_none());
        assert_eq!(net_rows, 0);
    }

    #[test]
    fn absent_change_rejects_decoded_table_drift() {
        let current = BTreeMap::from([(1, serde_json::json!({"Id": 1, "Value": 10}))]);
        let baseline = BTreeMap::from([(1, serde_json::json!({"Id": 1, "Value": 11}))]);
        let error = raw_table_change(None, &current, Some(&baseline)).unwrap_err();
        assert!(error.to_string().contains("full decoded tables differ"));
    }
}
