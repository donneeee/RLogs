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

const SCHEMA_VERSION: u16 = 4;
const MAX_CONTEXT_ARRAY_ITEMS: usize = 32;

#[derive(Debug)]
struct Arguments {
    decoded_root: PathBuf,
    worklist: PathBuf,
    route_proof: Option<PathBuf>,
    target_watchlist: Option<PathBuf>,
    explicit_targets: Vec<i64>,
    expected_build: String,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct BuildWorklist {
    #[serde(alias = "game_build")]
    build_id: String,
}

#[derive(Debug, Deserialize)]
struct FormulaGapWatchlist {
    game_build: String,
    selected_effect_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct RouteProof {
    game_build: String,
    keys: Vec<RouteKey>,
}

#[derive(Debug, Deserialize)]
struct RouteKey {
    lookup_key: String,
    ability_id: i64,
    candidates: Vec<RouteCandidate>,
    resolution_state: String,
}

#[derive(Debug, Deserialize)]
struct RouteCandidate {
    damage_attr_id: i64,
    routes: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct ReferenceCatalog {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    deployment_id: &'static str,
    channel: &'static str,
    build_id: String,
    policy: Policy,
    inputs: Vec<InputArtifact>,
    summary: Summary,
    targets: Vec<TargetReport>,
    table_sources: Vec<TableSource>,
    references: Vec<Reference>,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_build_required: bool,
    unresolved_route_targets_only: bool,
    explicit_target_ids_allowed: bool,
    target_watchlist_allowed: bool,
    target_mode: &'static str,
    raw_rows_embedded: bool,
    direct_references_are_route_authority: bool,
    unresolved_targets_hidden: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    role: &'static str,
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Summary {
    unresolved_lookup_keys: usize,
    distinct_target_values: usize,
    decoded_tables_scanned: usize,
    decoded_rows_scanned: usize,
    direct_scalar_references: usize,
    targets_with_references: usize,
    targets_without_references: usize,
}

#[derive(Debug, Default)]
struct TargetSeed {
    roles: BTreeSet<&'static str>,
    lookup_keys: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct TargetReport {
    value: i64,
    roles: BTreeSet<&'static str>,
    lookup_keys: BTreeSet<String>,
    reference_count: usize,
    referenced_by_tables: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct TableSource {
    table: String,
    file: String,
    bytes: u64,
    sha256: String,
    decoded_rows: usize,
    reference_count: usize,
}

#[derive(Debug, Serialize)]
struct Reference {
    value: i64,
    matched_roles: BTreeSet<&'static str>,
    lookup_keys: BTreeSet<String>,
    table: String,
    row_key: String,
    json_pointer: String,
    value_encoding: &'static str,
    containing_object_pointer: String,
    scalar_context: BTreeMap<String, Value>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("decoded table reference scan failed: {error}");
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
    if arguments.expected_build.is_empty()
        || !arguments
            .expected_build
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return Err("--build requires a numeric client build".into());
    }

    let worklist_bytes = fs::read(&arguments.worklist)?;
    let worklist: BuildWorklist = serde_json::from_slice(&worklist_bytes)?;
    require_build("worklist", &worklist.build_id, &arguments.expected_build)?;
    let (targets, unresolved_lookup_keys, target_mode) =
        if let Some(route_proof_path) = arguments.route_proof.as_deref() {
            let route_bytes = fs::read(route_proof_path)?;
            let route_proof: RouteProof = serde_json::from_slice(&route_bytes)?;
            require_build(
                "route proof",
                &route_proof.game_build,
                &arguments.expected_build,
            )?;
            let (targets, keys) = unresolved_targets(&route_proof)?;
            (targets, keys, "unresolved-route-targets")
        } else if let Some(target_watchlist_path) = arguments.target_watchlist.as_deref() {
            let watchlist_bytes = fs::read(target_watchlist_path)?;
            let watchlist: FormulaGapWatchlist = serde_json::from_slice(&watchlist_bytes)?;
            require_build(
                "formula gap watchlist",
                &watchlist.game_build,
                &arguments.expected_build,
            )?;
            (
                watchlist_targets(&watchlist.selected_effect_ids)?,
                0,
                "formula-gap-watchlist-effect-ids",
            )
        } else {
            (
                explicit_targets(&arguments.explicit_targets)?,
                0,
                "explicit-exact-ids",
            )
        };
    if targets.is_empty() {
        return Err("route proof contains no unresolved route targets".into());
    }

    let mut files = fs::read_dir(&arguments.decoded_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    if files.is_empty() {
        return Err(format!(
            "decoded root {} contains no JSON tables",
            arguments.decoded_root.display()
        )
        .into());
    }

    let mut table_sources = Vec::with_capacity(files.len());
    let mut references = Vec::new();
    let mut decoded_rows_scanned = 0_usize;
    for path in files {
        let bytes = fs::read(&path)?;
        let sha256 = sha256_bytes(&bytes);
        let decoded: Value = serde_json::from_slice(&bytes)?;
        let rows = decoded
            .as_object()
            .ok_or_else(|| format!("decoded table {} is not a JSON object", path.display()))?;
        let table = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("invalid decoded table path {}", path.display()))?
            .to_owned();
        let before = references.len();
        for (row_key, row) in rows {
            scan_value(
                row,
                &targets,
                &table,
                row_key,
                "",
                "",
                None,
                &mut references,
            );
        }
        decoded_rows_scanned = decoded_rows_scanned.saturating_add(rows.len());
        table_sources.push(TableSource {
            table,
            file: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("decoded-table.json")
                .to_owned(),
            bytes: bytes.len() as u64,
            sha256,
            decoded_rows: rows.len(),
            reference_count: references.len().saturating_sub(before),
        });
    }

    references.sort_by(|left, right| {
        left.value
            .cmp(&right.value)
            .then_with(|| left.table.cmp(&right.table))
            .then_with(|| left.row_key.cmp(&right.row_key))
            .then_with(|| left.json_pointer.cmp(&right.json_pointer))
    });
    references.dedup_by(|left, right| {
        left.value == right.value
            && left.table == right.table
            && left.row_key == right.row_key
            && left.json_pointer == right.json_pointer
    });

    let mut target_reports = Vec::with_capacity(targets.len());
    for (value, seed) in &targets {
        let matching = references
            .iter()
            .filter(|reference| reference.value == *value)
            .collect::<Vec<_>>();
        target_reports.push(TargetReport {
            value: *value,
            roles: seed.roles.clone(),
            lookup_keys: seed.lookup_keys.clone(),
            reference_count: matching.len(),
            referenced_by_tables: matching
                .into_iter()
                .map(|reference| reference.table.clone())
                .collect(),
        });
    }
    let targets_with_references = target_reports
        .iter()
        .filter(|target| target.reference_count > 0)
        .count();

    let inputs = input_artifacts(&arguments)?;
    let catalog = ReferenceCatalog {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-decoded-table-reference-scan",
        game: "blue-protocol-star-resonance",
        deployment_id: "global",
        channel: "steam",
        build_id: arguments.expected_build,
        policy: Policy {
            exact_build_required: true,
            unresolved_route_targets_only: arguments.route_proof.is_some(),
            explicit_target_ids_allowed: true,
            target_watchlist_allowed: true,
            target_mode,
            raw_rows_embedded: false,
            direct_references_are_route_authority: false,
            unresolved_targets_hidden: false,
            promotion_requirement: "an explicit owner/reference construction plus matching-build packet damage_source selection and conservation proof",
        },
        inputs,
        summary: Summary {
            unresolved_lookup_keys,
            distinct_target_values: targets.len(),
            decoded_tables_scanned: table_sources.len(),
            decoded_rows_scanned,
            direct_scalar_references: references.len(),
            targets_with_references,
            targets_without_references: targets.len().saturating_sub(targets_with_references),
        },
        targets: target_reports,
        table_sources,
        references,
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &catalog)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "scanned {} tables and {} rows; found {} references across {}/{} targets",
        catalog.summary.decoded_tables_scanned,
        catalog.summary.decoded_rows_scanned,
        catalog.summary.direct_scalar_references,
        catalog.summary.targets_with_references,
        catalog.summary.distinct_target_values,
    );
    Ok(())
}

fn explicit_targets(values: &[i64]) -> Result<BTreeMap<i64, TargetSeed>, String> {
    let mut targets = BTreeMap::new();
    for value in values {
        if *value <= 0 {
            return Err(format!("explicit target ID must be positive, got {value}"));
        }
        let seed = targets.entry(*value).or_insert_with(TargetSeed::default);
        seed.roles.insert("explicit-id");
    }
    if targets.is_empty() {
        return Err("at least one explicit target ID is required".to_owned());
    }
    Ok(targets)
}

fn watchlist_targets(values: &[i64]) -> Result<BTreeMap<i64, TargetSeed>, String> {
    let mut targets = BTreeMap::new();
    for value in values {
        if *value <= 0 {
            return Err(format!(
                "formula gap effect ID must be positive, got {value}"
            ));
        }
        let seed = targets.entry(*value).or_insert_with(TargetSeed::default);
        seed.roles.insert("formula-gap-effect-id");
    }
    if targets.is_empty() {
        return Err("formula gap watchlist contains no selected effect IDs".to_owned());
    }
    Ok(targets)
}

fn unresolved_targets(
    route_proof: &RouteProof,
) -> Result<(BTreeMap<i64, TargetSeed>, usize), String> {
    let mut targets = BTreeMap::<i64, TargetSeed>::new();
    let mut unresolved_lookup_keys = BTreeSet::new();
    for key in &route_proof.keys {
        if key.resolution_state != "unresolved-retained" {
            continue;
        }
        unresolved_lookup_keys.insert(key.lookup_key.clone());
        let ability = targets.entry(key.ability_id).or_default();
        ability.roles.insert("packet-ability-id");
        ability.lookup_keys.insert(key.lookup_key.clone());
        for candidate in &key.candidates {
            let damage_attr = targets.entry(candidate.damage_attr_id).or_default();
            damage_attr.roles.insert(if candidate.routes.is_empty() {
                "unrouted-damage-attribute-id"
            } else {
                "ambiguous-routed-damage-attribute-id"
            });
            damage_attr.lookup_keys.insert(key.lookup_key.clone());
        }
    }
    if unresolved_lookup_keys.is_empty() {
        return Err("route proof has no unresolved-retained keys".to_owned());
    }
    Ok((targets, unresolved_lookup_keys.len()))
}

#[allow(clippy::too_many_arguments)]
fn scan_value(
    value: &Value,
    targets: &BTreeMap<i64, TargetSeed>,
    table: &str,
    row_key: &str,
    pointer: &str,
    containing_object_pointer: &str,
    containing_object: Option<&Map<String, Value>>,
    references: &mut Vec<Reference>,
) {
    if let Some((number, encoding)) = exact_integer(value) {
        if let Some(seed) = targets.get(&number) {
            references.push(Reference {
                value: number,
                matched_roles: seed.roles.clone(),
                lookup_keys: seed.lookup_keys.clone(),
                table: table.to_owned(),
                row_key: row_key.to_owned(),
                json_pointer: if pointer.is_empty() {
                    "/".to_owned()
                } else {
                    pointer.to_owned()
                },
                value_encoding: encoding,
                containing_object_pointer: if containing_object_pointer.is_empty() {
                    "/".to_owned()
                } else {
                    containing_object_pointer.to_owned()
                },
                scalar_context: containing_object.map(scalar_context).unwrap_or_default(),
            });
        }
    }

    match value {
        Value::Object(object) => {
            for (field, child) in object {
                let child_pointer = format!("{pointer}/{}", escape_pointer(field));
                scan_value(
                    child,
                    targets,
                    table,
                    row_key,
                    &child_pointer,
                    pointer,
                    Some(object),
                    references,
                );
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                let child_pointer = format!("{pointer}/{index}");
                scan_value(
                    child,
                    targets,
                    table,
                    row_key,
                    &child_pointer,
                    containing_object_pointer,
                    containing_object,
                    references,
                );
            }
        }
        _ => {}
    }
}

fn scalar_context(object: &Map<String, Value>) -> BTreeMap<String, Value> {
    object
        .iter()
        .filter_map(|(field, value)| {
            if value.is_null() || value.is_boolean() || value.is_number() || value.is_string() {
                return Some((field.clone(), value.clone()));
            }
            let array = value.as_array()?;
            if array.len() <= MAX_CONTEXT_ARRAY_ITEMS
                && array.iter().all(|item| {
                    item.is_null() || item.is_boolean() || item.is_number() || item.is_string()
                })
            {
                Some((field.clone(), value.clone()))
            } else {
                None
            }
        })
        .collect()
}

fn exact_integer(value: &Value) -> Option<(i64, &'static str)> {
    if let Some(number) = value.as_i64() {
        return Some((number, "json-integer"));
    }
    value
        .as_str()
        .and_then(|text| text.parse::<i64>().ok())
        .map(|number| (number, "decimal-string"))
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn require_build(role: &str, actual: &str, expected: &str) -> Result<(), String> {
    if actual != expected {
        return Err(format!(
            "{role} build {actual} does not match --build {expected}"
        ));
    }
    Ok(())
}

fn arguments() -> Result<Arguments, String> {
    let mut decoded_root = None;
    let mut worklist = None;
    let mut route_proof = None;
    let mut target_watchlist = None;
    let mut explicit_targets = Vec::new();
    let mut expected_build = None;
    let mut output = None;
    let mut values = env::args().skip(1);
    while let Some(argument) = values.next() {
        let value = values.next().ok_or_else(usage)?;
        match argument.as_str() {
            "--decoded-root" => decoded_root = Some(PathBuf::from(value)),
            "--worklist" => worklist = Some(PathBuf::from(value)),
            "--route-proof" => route_proof = Some(PathBuf::from(value)),
            "--target-watchlist" => target_watchlist = Some(PathBuf::from(value)),
            "--target" => explicit_targets.push(
                value
                    .parse::<i64>()
                    .map_err(|_| format!("invalid --target value {value}"))?,
            ),
            "--build" => expected_build = Some(value),
            "--output" => output = Some(PathBuf::from(value)),
            _ => return Err(usage()),
        }
    }
    let target_modes = usize::from(route_proof.is_some())
        + usize::from(target_watchlist.is_some())
        + usize::from(!explicit_targets.is_empty());
    if target_modes != 1 {
        return Err(
            "provide exactly one target source: --route-proof, --target-watchlist, or one or more --target values"
                .to_owned(),
        );
    }
    Ok(Arguments {
        decoded_root: decoded_root.ok_or_else(usage)?,
        worklist: worklist.ok_or_else(usage)?,
        route_proof,
        target_watchlist,
        explicit_targets,
        expected_build: expected_build.ok_or_else(usage)?,
        output: output.ok_or_else(usage)?,
    })
}

fn usage() -> String {
    "usage: rlogs-bpsr-decoded-table-reference-scan --decoded-root <Excels> --worklist <ctb-rdps-proof-worklist.json> (--route-proof <damage-source-route-proof.json> | --target-watchlist <formula-magnitude-gap-watchlist.json> | --target <positive-id> [--target <positive-id> ...]) --build <numeric-client-build> --output <reference-scan.json>".to_owned()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn input_artifact(
    role: &'static str,
    path: &Path,
) -> Result<InputArtifact, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes = bytes.saturating_add(count as u64);
    }
    Ok(InputArtifact {
        role,
        file: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("external-artifact")
            .to_owned(),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn input_artifacts(
    arguments: &Arguments,
) -> Result<Vec<InputArtifact>, Box<dyn std::error::Error>> {
    let mut inputs = vec![input_artifact(
        "exact-build-CTB-worklist",
        &arguments.worklist,
    )?];
    if let Some(route_proof) = arguments.route_proof.as_deref() {
        inputs.push(input_artifact("damage-source-route-proof", route_proof)?);
    }
    if let Some(target_watchlist) = arguments.target_watchlist.as_deref() {
        inputs.push(input_artifact(
            "formula-magnitude-gap-watchlist",
            target_watchlist,
        )?);
    }
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::{
        FormulaGapWatchlist, RouteProof, exact_integer, explicit_targets, scalar_context,
        scan_value, unresolved_targets, watchlist_targets,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn derives_every_candidate_for_unresolved_keys_and_classifies_route_state() {
        let proof: RouteProof = serde_json::from_value(json!({
            "game_build": "24568685",
            "keys": [
                {
                    "lookup_key": "100:1",
                    "ability_id": 100,
                    "candidates": [
                        { "damage_attr_id": 10001, "routes": [] },
                        { "damage_attr_id": 10002, "routes": [{"owner": "x"}] }
                    ],
                    "resolution_state": "unresolved-retained"
                }
            ]
        }))
        .unwrap();
        let (targets, keys) = unresolved_targets(&proof).unwrap();
        assert_eq!(keys, 1);
        assert!(targets.contains_key(&100));
        assert!(targets.contains_key(&10_001));
        assert!(targets.contains_key(&10_002));
        assert!(
            targets[&10_001]
                .roles
                .contains("unrouted-damage-attribute-id")
        );
        assert!(
            targets[&10_002]
                .roles
                .contains("ambiguous-routed-damage-attribute-id")
        );
    }

    #[test]
    fn explicit_targets_are_deduplicated_and_never_relabelled_as_routes() {
        let targets = explicit_targets(&[2110078, 2110078, 2110143]).unwrap();
        assert_eq!(targets.len(), 2);
        assert!(targets[&2110078].roles.contains("explicit-id"));
        assert!(targets[&2110078].lookup_keys.is_empty());
        assert!(explicit_targets(&[0]).is_err());
    }

    #[test]
    fn watchlist_targets_are_complete_deduplicated_and_distinctly_labelled() {
        let watchlist: FormulaGapWatchlist = serde_json::from_value(json!({
            "game_build": "24609362",
            "selected_effect_ids": [2110102, 2110102, 3210211]
        }))
        .unwrap();
        let targets = watchlist_targets(&watchlist.selected_effect_ids).unwrap();
        assert_eq!(targets.len(), 2);
        assert!(targets[&2110102].roles.contains("formula-gap-effect-id"));
        assert!(targets[&2110102].lookup_keys.is_empty());
        assert!(watchlist_targets(&[]).is_err());
    }

    #[test]
    fn scans_nested_integer_and_decimal_string_references_with_context() {
        let proof: RouteProof = serde_json::from_value(json!({
            "game_build": "24568685",
            "keys": [{
                "lookup_key": "100:1",
                "ability_id": 100,
                "candidates": [{ "damage_attr_id": 10001, "routes": [] }],
                "resolution_state": "unresolved-retained"
            }]
        }))
        .unwrap();
        let (targets, _) = unresolved_targets(&proof).unwrap();
        let row = json!({
            "Id": 7,
            "Owner": 100,
            "Nested": { "DamageAttrId": "10001", "Other": 3 },
            "Values": [10001, 2]
        });
        let mut references = Vec::new();
        scan_value(
            &row,
            &targets,
            "ExampleTable",
            "7",
            "",
            "",
            None,
            &mut references,
        );
        assert_eq!(references.len(), 3);
        assert!(
            references
                .iter()
                .any(|reference| reference.json_pointer == "/Nested/DamageAttrId")
        );
        assert!(
            references
                .iter()
                .any(|reference| reference.json_pointer == "/Values/0")
        );
    }

    #[test]
    fn exact_integer_rejects_fractional_and_non_decimal_values() {
        assert_eq!(exact_integer(&json!(5)), Some((5, "json-integer")));
        assert_eq!(exact_integer(&json!("5")), Some((5, "decimal-string")));
        assert_eq!(exact_integer(&json!(5.5)), None);
        assert_eq!(exact_integer(&json!("0x5")), None);
    }

    #[test]
    fn context_retains_only_bounded_scalar_fields() {
        let object = json!({
            "Id": 1,
            "Name": "row",
            "Small": [1, 2],
            "Nested": {"Value": 3},
            "Large": (0..40).collect::<Vec<_>>()
        });
        let context = scalar_context(object.as_object().unwrap());
        assert_eq!(context.get("Id"), Some(&json!(1)));
        assert_eq!(context.get("Small"), Some(&json!([1, 2])));
        assert!(!context.contains_key("Nested"));
        assert!(!context.contains_key("Large"));
        let empty = BTreeMap::<String, serde_json::Value>::new();
        assert!(empty.is_empty());
    }
}
