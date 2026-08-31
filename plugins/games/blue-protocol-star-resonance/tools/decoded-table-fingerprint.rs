use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    decoded_root: PathBuf,
    worklist: PathBuf,
    source_inventory: Option<PathBuf>,
    build: Option<String>,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ProofWorklist {
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    entries: Vec<WorkEntry>,
}

#[derive(Debug, Deserialize)]
struct WorkEntry {
    route: String,
    table_key: u32,
    stable_key: String,
    names: Vec<String>,
    current: Option<TableVersion>,
}

#[derive(Debug, Deserialize)]
struct SourceInventory {
    build_id: String,
    source: SourceInventoryIdentity,
    tables: Vec<SourceInventoryTable>,
}

#[derive(Debug, Deserialize)]
struct SourceInventoryIdentity {
    package_relative_path: String,
}

#[derive(Debug, Deserialize)]
struct SourceInventoryTable {
    address_keys: Vec<SourceInventoryKey>,
    offset: u64,
    bytes: u64,
    sha256: String,
    shape: Shape,
}

#[derive(Debug, Deserialize)]
struct SourceInventoryKey {
    key: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TableVersion {
    relative_path: String,
    offset: u64,
    bytes: u64,
    sha256: String,
    shape: Shape,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Shape {
    rows: u32,
    row_size: u32,
    row_data_bytes: u32,
    pool_lengths: Vec<PoolLength>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PoolLength {
    r#type: u32,
    bytes: u32,
}

#[derive(Debug, Serialize)]
struct FingerprintCatalog {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    policy: Policy,
    summary: Summary,
    tables: Vec<TableFingerprint>,
}

#[derive(Debug, Serialize)]
struct Policy {
    raw_rows_embedded: bool,
    absolute_paths_embedded: bool,
    row_hashes_are_runtime_authority: bool,
    decoded_count_must_match_structural_count: bool,
    changed_rows_auto_promoted: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct Summary {
    direct_tables: usize,
    decoded_rows: usize,
    schema_fields: usize,
}

#[derive(Debug, Serialize)]
struct TableFingerprint {
    table_key: u32,
    table_key_hex: String,
    stable_key: String,
    name: String,
    source: TableVersion,
    decoded_rows: usize,
    schema: BTreeMap<String, BTreeMap<String, usize>>,
    row_fingerprints: BTreeMap<String, String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("decoded table fingerprint failed: {error}");
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

    let worklist: ProofWorklist = serde_json::from_slice(&fs::read(&arguments.worklist)?)?;
    let (build_id, source_versions) = source_versions(&arguments, &worklist)?;
    let mut entries = worklist
        .entries
        .into_iter()
        .filter(|entry| is_direct_route(&entry.route))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.stable_key.cmp(&right.stable_key));

    let mut tables = Vec::with_capacity(entries.len());
    let mut decoded_rows = 0_usize;
    let mut schema_fields = 0_usize;
    for entry in entries {
        let name = exact_table_name(&entry)?;
        let path = safe_table_path(&arguments.decoded_root, &name)?;
        let decoded: Value = serde_json::from_slice(&fs::read(&path)?)?;
        let rows = decoded
            .as_object()
            .ok_or_else(|| format!("decoded table {name} is not a JSON object"))?;
        let source = match source_versions.as_ref() {
            Some(versions) => versions.get(&entry.table_key).cloned().ok_or_else(|| {
                format!(
                    "source inventory has no exact table key {}",
                    entry.table_key
                )
            })?,
            None => entry
                .current
                .ok_or_else(|| format!("direct table {name} has no current version"))?,
        };
        if rows.len() != source.shape.rows as usize {
            return Err(format!(
                "decoded row count mismatch for {name}: structural {}, decoded {}",
                source.shape.rows,
                rows.len()
            )
            .into());
        }

        let (schema, row_fingerprints) = fingerprint_rows(rows)?;
        decoded_rows += rows.len();
        schema_fields += schema.len();
        tables.push(TableFingerprint {
            table_key: entry.table_key,
            table_key_hex: format!("0x{:08x}", entry.table_key),
            stable_key: entry.stable_key,
            name,
            source,
            decoded_rows: rows.len(),
            schema,
            row_fingerprints,
        });
    }

    let catalog = FingerprintCatalog {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-decoded-table-fingerprint",
        game: "blue-protocol-star-resonance",
        deployment_id: worklist.deployment_id,
        channel: worklist.channel,
        distribution_app_id: worklist.distribution_app_id,
        build_id,
        policy: Policy {
            raw_rows_embedded: false,
            absolute_paths_embedded: false,
            row_hashes_are_runtime_authority: false,
            decoded_count_must_match_structural_count: true,
            changed_rows_auto_promoted: false,
            promotion_requirement: "matching-build field review, packet replay, and exact conservation proof",
        },
        summary: Summary {
            direct_tables: tables.len(),
            decoded_rows,
            schema_fields,
        },
        tables,
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &catalog)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn source_versions(
    arguments: &Arguments,
    worklist: &ProofWorklist,
) -> Result<(String, Option<BTreeMap<u32, TableVersion>>), Box<dyn std::error::Error>> {
    let Some(path) = arguments.source_inventory.as_deref() else {
        if let Some(build) = arguments.build.as_deref() {
            if build != worklist.build_id {
                return Err(
                    "--build without --source-inventory must match the worklist build".into(),
                );
            }
        }
        return Ok((worklist.build_id.clone(), None));
    };
    let inventory: SourceInventory = serde_json::from_slice(&fs::read(path)?)?;
    if let Some(build) = arguments.build.as_deref() {
        if build != inventory.build_id {
            return Err(format!(
                "--build {build} does not match source inventory build {}",
                inventory.build_id
            )
            .into());
        }
    }
    let mut versions = BTreeMap::new();
    for table in inventory.tables {
        if table.address_keys.len() != 1 {
            return Err(format!(
                "source table at offset {} has {} address keys",
                table.offset,
                table.address_keys.len()
            )
            .into());
        }
        let key = table.address_keys[0].key;
        if versions
            .insert(
                key,
                TableVersion {
                    relative_path: inventory.source.package_relative_path.clone(),
                    offset: table.offset,
                    bytes: table.bytes,
                    sha256: table.sha256,
                    shape: table.shape,
                },
            )
            .is_some()
        {
            return Err(format!("duplicate source inventory table key {key}").into());
        }
    }
    Ok((inventory.build_id, Some(versions)))
}

fn is_direct_route(route: &str) -> bool {
    matches!(
        route,
        "formula-inputs" | "ability-effect-origin" | "equipment-state"
    )
}

fn exact_table_name(entry: &WorkEntry) -> Result<String, Box<dyn std::error::Error>> {
    if entry.names.len() != 1 {
        return Err(format!(
            "direct table {} requires exactly one proven name, found {}",
            entry.stable_key,
            entry.names.len()
        )
        .into());
    }
    let name = entry.names[0]
        .strip_suffix(".ctb")
        .ok_or_else(|| format!("table name {} lacks .ctb suffix", entry.names[0]))?;
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(format!("unsafe table name {}", entry.names[0]).into());
    }
    Ok(name.to_owned())
}

fn safe_table_path(root: &Path, name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = root.join(format!("{name}.json"));
    if !path.starts_with(root) {
        return Err(format!("decoded table path escaped root for {name}").into());
    }
    Ok(path)
}

fn fingerprint_rows(
    rows: &Map<String, Value>,
) -> Result<
    (
        BTreeMap<String, BTreeMap<String, usize>>,
        BTreeMap<String, String>,
    ),
    Box<dyn std::error::Error>,
> {
    let mut schema = BTreeMap::<String, BTreeMap<String, usize>>::new();
    let mut fingerprints = BTreeMap::new();
    for (key, row) in rows {
        let object = row
            .as_object()
            .ok_or_else(|| format!("row {key} is not a JSON object"))?;
        for (field, value) in object {
            *schema
                .entry(field.clone())
                .or_default()
                .entry(value_kind(value).to_owned())
                .or_insert(0) += 1;
        }
        let canonical = canonicalize(row);
        let bytes = serde_json::to_vec(&canonical)?;
        fingerprints.insert(key.clone(), format!("sha256:{:x}", Sha256::digest(bytes)));
    }
    Ok((schema, fingerprints))
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let sorted = values
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        _ => value.clone(),
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut decoded_root = None;
    let mut worklist = None;
    let mut source_inventory = None;
    let mut build = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--decoded-root" => {
                decoded_root = Some(PathBuf::from(next_value(&mut args, "--decoded-root")?))
            }
            "--worklist" => worklist = Some(PathBuf::from(next_value(&mut args, "--worklist")?)),
            "--source-inventory" => {
                source_inventory = Some(PathBuf::from(next_value(&mut args, "--source-inventory")?))
            }
            "--build" => {
                build = Some(
                    next_value(&mut args, "--build")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        decoded_root: decoded_root.ok_or("missing --decoded-root")?,
        worklist: worklist.ok_or("missing --worklist")?,
        source_inventory,
        build,
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

    #[test]
    fn canonical_hash_ignores_object_field_order() {
        let left = serde_json::json!({"a": 1, "b": {"x": 2, "y": 3}});
        let right = serde_json::json!({"b": {"y": 3, "x": 2}, "a": 1});
        assert_eq!(
            serde_json::to_vec(&canonicalize(&left)).unwrap(),
            serde_json::to_vec(&canonicalize(&right)).unwrap()
        );
    }

    #[test]
    fn schema_counts_observed_value_kinds() {
        let rows = serde_json::json!({
            "1": {"Id": 1, "Name": "one", "Tags": []},
            "2": {"Id": 2, "Name": null, "Tags": [1]}
        });
        let (schema, fingerprints) = fingerprint_rows(rows.as_object().unwrap()).unwrap();
        assert_eq!(schema["Id"]["number"], 2);
        assert_eq!(schema["Name"]["string"], 1);
        assert_eq!(schema["Name"]["null"], 1);
        assert_eq!(schema["Tags"]["array"], 2);
        assert_eq!(fingerprints.len(), 2);
    }

    #[test]
    fn only_proven_direct_routes_are_fingerprinted() {
        assert!(is_direct_route("formula-inputs"));
        assert!(is_direct_route("ability-effect-origin"));
        assert!(is_direct_route("equipment-state"));
        assert!(!is_direct_route("identity-resolution"));
        assert!(!is_direct_route("retained-secondary-review"));
    }

    #[test]
    fn table_names_cannot_escape_the_decoded_root() {
        let entry = WorkEntry {
            route: "formula-inputs".to_owned(),
            table_key: 1,
            stable_key: "ctb.Bad".to_owned(),
            names: vec!["../Bad.ctb".to_owned()],
            current: None,
        };
        assert!(exact_table_name(&entry).is_err());
    }
}
