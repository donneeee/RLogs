use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    current: PathBuf,
    baseline_indexed: Option<PathBuf>,
    baseline_named: PathBuf,
    baseline_unknown: PathBuf,
    baseline_build: String,
    identity_overlay: Option<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CurrentInventory {
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    source: CurrentSource,
    tables: Vec<CurrentTable>,
}

#[derive(Debug, Deserialize)]
struct CurrentSource {
    package_relative_path: String,
}

#[derive(Debug, Deserialize)]
struct CurrentTable {
    address_keys: Vec<AddressKey>,
    offset: u64,
    bytes: u64,
    sha256: String,
    shape: Shape,
}

#[derive(Debug, Deserialize)]
struct AddressKey {
    key: u32,
}

#[derive(Debug, Deserialize)]
struct IdentityOverlay {
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    identities: Vec<CurrentIdentity>,
}

#[derive(Debug, Clone, Deserialize)]
struct CurrentIdentity {
    table_key: u32,
    table_name: String,
    stable_key: String,
    domain: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Shape {
    rows: u32,
    row_size: u32,
    row_data_bytes: u32,
    pool_lengths: Vec<PoolLength>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct PoolLength {
    r#type: u32,
    bytes: u32,
}

#[derive(Debug, Deserialize)]
struct BaselineTable {
    table_key: u32,
    stable_key: String,
    names: Vec<String>,
    state: String,
    domain: String,
    source: BaselineSource,
    shape: Shape,
}

#[derive(Debug, Deserialize)]
struct BaselineSource {
    relative_path: String,
    offset: u64,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct BuildDiff {
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
    affected_proof_suites: Vec<&'static str>,
    changes: Vec<TableChange>,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_key_identity_required: bool,
    location_proximity_is_identity: bool,
    current_identity_overlay_applied: bool,
    changed_tables_auto_promoted: bool,
    unresolved_tables_hidden: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct Summary {
    baseline_tables: usize,
    current_tables: usize,
    unchanged_tables: usize,
    changed_tables: usize,
    added_tables: usize,
    removed_tables: usize,
    changed_named_tables: usize,
    changed_unresolved_tables: usize,
}

#[derive(Debug, Serialize)]
struct TableChange {
    table_key: u32,
    table_key_hex: String,
    stable_key: String,
    names: Vec<String>,
    baseline_state: Option<String>,
    domain: String,
    change: &'static str,
    baseline: Option<TableVersion>,
    current: Option<TableVersion>,
    shape_changes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct TableVersion {
    relative_path: String,
    offset: u64,
    bytes: u64,
    sha256: String,
    shape: Shape,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("CTB build diff failed: {error}");
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

    let current: CurrentInventory = serde_json::from_slice(&fs::read(&arguments.current)?)?;
    if current.build_id == arguments.baseline_build {
        return Err("current and baseline build IDs are identical".into());
    }
    let deployment_id = current.deployment_id;
    let channel = current.channel;
    let distribution_app_id = current.distribution_app_id;
    let build_id = current.build_id;
    let current_package_relative_path = current.source.package_relative_path;
    let identity_baseline = load_baseline(&arguments.baseline_named, &arguments.baseline_unknown)?;
    let mut baseline = match arguments.baseline_indexed.as_deref() {
        Some(path) => load_adjacent_baseline(path, &arguments.baseline_build, &identity_baseline)?,
        None => identity_baseline,
    };
    let current = current_by_key(current.tables)?;
    let current_identities = match arguments.identity_overlay.as_deref() {
        Some(path) => {
            let overlay: IdentityOverlay = serde_json::from_slice(&fs::read(path)?)?;
            validate_identity_overlay(
                &overlay,
                &deployment_id,
                &channel,
                &distribution_app_id,
                &build_id,
                &current,
            )?;
            let identities = keyed_identities(overlay.identities)?;
            apply_identity_overlay(&mut baseline, &identities);
            identities
        }
        None => BTreeMap::new(),
    };

    let all_keys = baseline
        .keys()
        .chain(current.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();
    let mut unchanged_tables = 0_usize;
    let mut changed_tables = 0_usize;
    let mut added_tables = 0_usize;
    let mut removed_tables = 0_usize;
    let mut changed_named_tables = 0_usize;
    let mut changed_unresolved_tables = 0_usize;

    for key in all_keys {
        let old = baseline.get(&key);
        let new = current.get(&key);
        if old.is_some_and(|old| new.is_some_and(|new| old.source.sha256 == new.sha256)) {
            unchanged_tables += 1;
            continue;
        }

        let change = match (old, new) {
            (Some(_), Some(_)) => {
                changed_tables += 1;
                if old.is_some_and(|table| table.state == "named") {
                    changed_named_tables += 1;
                } else {
                    changed_unresolved_tables += 1;
                }
                "changed"
            }
            (None, Some(_)) => {
                added_tables += 1;
                "added"
            }
            (Some(_), None) => {
                removed_tables += 1;
                "removed"
            }
            (None, None) => unreachable!(),
        };
        changes.push(table_change(
            key,
            change,
            old,
            new,
            current_identities.get(&key),
            &current_package_relative_path,
        ));
    }

    let affected_proof_suites = if changed_tables + added_tables + removed_tables == 0 {
        Vec::new()
    } else {
        vec![
            "combat-table-diff",
            "schema-diff",
            "packet-replay",
            "runtime-conservation",
        ]
    };
    let diff = BuildDiff {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-ctb-build-diff",
        game: "blue-protocol-star-resonance",
        deployment_id,
        channel,
        distribution_app_id,
        baseline_build_id: arguments.baseline_build,
        build_id,
        policy: Policy {
            exact_key_identity_required: true,
            location_proximity_is_identity: false,
            current_identity_overlay_applied: arguments.identity_overlay.is_some(),
            changed_tables_auto_promoted: false,
            unresolved_tables_hidden: false,
            promotion_requirement: "matching-build row/schema review, packet replay, and exact conservation proof",
        },
        summary: Summary {
            baseline_tables: baseline.len(),
            current_tables: current.len(),
            unchanged_tables,
            changed_tables,
            added_tables,
            removed_tables,
            changed_named_tables,
            changed_unresolved_tables,
        },
        affected_proof_suites,
        changes,
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &diff)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn load_baseline(
    named: &Path,
    unknown: &Path,
) -> Result<BTreeMap<u32, BaselineTable>, Box<dyn std::error::Error>> {
    let mut tables = BTreeMap::new();
    for directory in [named, unknown] {
        let mut files = fs::read_dir(directory)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort();
        for file in files {
            if file.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let shard: Vec<BaselineTable> = serde_json::from_slice(&fs::read(&file)?)?;
            for table in shard {
                if tables.insert(table.table_key, table).is_some() {
                    return Err(
                        format!("duplicate baseline table key in {}", file.display()).into(),
                    );
                }
            }
        }
    }
    Ok(tables)
}

fn load_adjacent_baseline(
    indexed: &Path,
    expected_build: &str,
    identities: &BTreeMap<u32, BaselineTable>,
) -> Result<BTreeMap<u32, BaselineTable>, Box<dyn std::error::Error>> {
    let inventory: CurrentInventory = serde_json::from_slice(&fs::read(indexed)?)?;
    if inventory.build_id != expected_build {
        return Err(format!(
            "adjacent baseline inventory build {} does not match --baseline-build {}",
            inventory.build_id, expected_build
        )
        .into());
    }
    let package_relative_path = inventory.source.package_relative_path;
    let indexed = current_by_key(inventory.tables)?;
    Ok(indexed
        .into_iter()
        .map(|(key, table)| {
            let identity = identities.get(&key);
            (
                key,
                BaselineTable {
                    table_key: key,
                    stable_key: identity
                        .map(|value| value.stable_key.clone())
                        .unwrap_or_else(|| format!("ctb.unknown-{key:08x}")),
                    names: identity
                        .map(|value| value.names.clone())
                        .unwrap_or_default(),
                    state: identity
                        .map(|value| value.state.clone())
                        .unwrap_or_else(|| "unresolved".to_owned()),
                    domain: identity
                        .map(|value| value.domain.clone())
                        .unwrap_or_else(|| "unknown".to_owned()),
                    source: BaselineSource {
                        relative_path: package_relative_path.clone(),
                        offset: table.offset,
                        bytes: table.bytes,
                        sha256: table.sha256,
                    },
                    shape: table.shape,
                },
            )
        })
        .collect())
}

fn current_by_key(
    tables: Vec<CurrentTable>,
) -> Result<BTreeMap<u32, CurrentTable>, Box<dyn std::error::Error>> {
    let mut keyed = BTreeMap::new();
    for table in tables {
        if table.address_keys.len() != 1 {
            return Err(format!(
                "current table at offset {} has {} address keys",
                table.offset,
                table.address_keys.len()
            )
            .into());
        }
        let key = table.address_keys[0].key;
        if keyed.insert(key, table).is_some() {
            return Err(format!("duplicate current table key {key}").into());
        }
    }
    Ok(keyed)
}

fn validate_identity_overlay(
    overlay: &IdentityOverlay,
    deployment_id: &str,
    channel: &str,
    distribution_app_id: &str,
    build_id: &str,
    current: &BTreeMap<u32, CurrentTable>,
) -> Result<(), String> {
    if overlay.deployment_id != deployment_id
        || overlay.channel != channel
        || overlay.distribution_app_id != distribution_app_id
        || overlay.build_id != build_id
    {
        return Err(
            "identity overlay deployment/build metadata does not match current inventory".into(),
        );
    }
    for identity in &overlay.identities {
        if !current.contains_key(&identity.table_key) {
            return Err(format!(
                "identity overlay key {} is absent from current inventory",
                identity.table_key
            ));
        }
        if !identity.table_name.ends_with(".ctb") {
            return Err(format!(
                "identity overlay name {} does not end in .ctb",
                identity.table_name
            ));
        }
        if hash33(&identity.table_name) != identity.table_key {
            return Err(format!(
                "identity overlay name {} does not hash to key {}",
                identity.table_name, identity.table_key
            ));
        }
        let expected_stable_key = format!("ctb.{}", identity.table_name.trim_end_matches(".ctb"));
        if identity.stable_key != expected_stable_key {
            return Err(format!(
                "identity overlay stable key {} does not match {}",
                identity.stable_key, expected_stable_key
            ));
        }
    }
    Ok(())
}

fn keyed_identities(
    identities: Vec<CurrentIdentity>,
) -> Result<BTreeMap<u32, CurrentIdentity>, String> {
    let mut keyed = BTreeMap::new();
    for identity in identities {
        let key = identity.table_key;
        if keyed.insert(key, identity).is_some() {
            return Err(format!("duplicate current identity key {key}"));
        }
    }
    Ok(keyed)
}

fn apply_identity_overlay(
    baseline: &mut BTreeMap<u32, BaselineTable>,
    identities: &BTreeMap<u32, CurrentIdentity>,
) {
    for (key, identity) in identities {
        let Some(table) = baseline.get_mut(key) else {
            continue;
        };
        table.stable_key.clone_from(&identity.stable_key);
        table.names = vec![identity.table_name.clone()];
        table.state = "named".to_owned();
        if table.domain == "unknown" || table.domain == "unreviewed" {
            table.domain.clone_from(&identity.domain);
        }
    }
}

fn hash33(value: &str) -> u32 {
    value.chars().fold(5381_u32, |hash, character| {
        hash.wrapping_mul(33).wrapping_add(character as u32)
    })
}

fn table_change(
    key: u32,
    change: &'static str,
    old: Option<&BaselineTable>,
    new: Option<&CurrentTable>,
    current_identity: Option<&CurrentIdentity>,
    current_package_relative_path: &str,
) -> TableChange {
    let shape_changes = match (old, new) {
        (Some(old), Some(new)) => shape_changes(&old.shape, &new.shape),
        _ => Vec::new(),
    };
    TableChange {
        table_key: key,
        table_key_hex: format!("0x{key:08x}"),
        stable_key: current_identity
            .map(|identity| identity.stable_key.clone())
            .or_else(|| old.map(|table| table.stable_key.clone()))
            .unwrap_or_else(|| format!("ctb.unknown-{key:08x}")),
        names: current_identity
            .map(|identity| vec![identity.table_name.clone()])
            .or_else(|| old.map(|table| table.names.clone()))
            .unwrap_or_default(),
        baseline_state: old.map(|table| table.state.clone()),
        domain: current_identity
            .map(|identity| identity.domain.clone())
            .or_else(|| old.map(|table| table.domain.clone()))
            .unwrap_or_else(|| "unknown".to_owned()),
        change,
        baseline: old.map(|table| TableVersion {
            relative_path: table.source.relative_path.clone(),
            offset: table.source.offset,
            bytes: table.source.bytes,
            sha256: table.source.sha256.clone(),
            shape: table.shape.clone(),
        }),
        current: new.map(|table| TableVersion {
            relative_path: current_package_relative_path.to_owned(),
            offset: table.offset,
            bytes: table.bytes,
            sha256: table.sha256.clone(),
            shape: table.shape.clone(),
        }),
        shape_changes,
    }
}

fn shape_changes(old: &Shape, new: &Shape) -> Vec<&'static str> {
    let mut changes = Vec::new();
    if old.rows != new.rows {
        changes.push("rows");
    }
    if old.row_size != new.row_size {
        changes.push("row_size");
    }
    if old.row_data_bytes != new.row_data_bytes {
        changes.push("row_data_bytes");
    }
    if old.pool_lengths != new.pool_lengths {
        changes.push("pool_lengths");
    }
    changes
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut current = None;
    let mut baseline_indexed = None;
    let mut baseline_named = None;
    let mut baseline_unknown = None;
    let mut baseline_build = None;
    let mut identity_overlay = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--current" => current = Some(PathBuf::from(next_value(&mut args, "--current")?)),
            "--baseline-indexed" => {
                baseline_indexed = Some(PathBuf::from(next_value(&mut args, "--baseline-indexed")?))
            }
            "--baseline-named" => {
                baseline_named = Some(PathBuf::from(next_value(&mut args, "--baseline-named")?))
            }
            "--baseline-unknown" => {
                baseline_unknown = Some(PathBuf::from(next_value(&mut args, "--baseline-unknown")?))
            }
            "--baseline-build" => {
                baseline_build = Some(
                    next_value(&mut args, "--baseline-build")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--identity-overlay" => {
                identity_overlay = Some(PathBuf::from(next_value(&mut args, "--identity-overlay")?))
            }
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        current: current.ok_or("missing --current")?,
        baseline_indexed,
        baseline_named: baseline_named.ok_or("missing --baseline-named")?,
        baseline_unknown: baseline_unknown.ok_or("missing --baseline-unknown")?,
        baseline_build: baseline_build.ok_or("missing --baseline-build")?,
        identity_overlay,
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

    fn shape(rows: u32, row_size: u32, pool_bytes: u32) -> Shape {
        Shape {
            rows,
            row_size,
            row_data_bytes: rows * row_size,
            pool_lengths: vec![PoolLength {
                r#type: 1,
                bytes: pool_bytes,
            }],
        }
    }

    fn current_table(key: u32) -> CurrentTable {
        CurrentTable {
            address_keys: vec![AddressKey { key }],
            offset: 100,
            bytes: 200,
            sha256: "current-sha256".to_owned(),
            shape: shape(10, 8, 2),
        }
    }

    fn unresolved_baseline(key: u32) -> BaselineTable {
        BaselineTable {
            table_key: key,
            stable_key: format!("ctb.unknown-{key:08x}"),
            names: Vec::new(),
            state: "unresolved".to_owned(),
            domain: "unknown".to_owned(),
            source: BaselineSource {
                relative_path: "container/m0.pkg".to_owned(),
                offset: 90,
                bytes: 200,
                sha256: "baseline-sha256".to_owned(),
            },
            shape: shape(10, 8, 2),
        }
    }

    #[test]
    fn shape_diff_names_only_changed_dimensions() {
        let old = shape(10, 8, 2);
        let new = shape(11, 8, 3);
        assert_eq!(
            shape_changes(&old, &new),
            vec!["rows", "row_data_bytes", "pool_lengths"]
        );
    }

    #[test]
    fn unchanged_shape_has_no_shape_changes() {
        let value = shape(10, 8, 2);
        assert!(shape_changes(&value, &value).is_empty());
    }

    #[test]
    fn exact_identity_overlay_promotes_an_unresolved_baseline_identity() {
        let key = hash33("ItemTempTable.ctb");
        let mut baseline = BTreeMap::from([(key, unresolved_baseline(key))]);
        let identities = BTreeMap::from([(
            key,
            CurrentIdentity {
                table_key: key,
                table_name: "ItemTempTable.ctb".to_owned(),
                stable_key: "ctb.ItemTempTable".to_owned(),
                domain: "items-and-equipment".to_owned(),
            },
        )]);

        apply_identity_overlay(&mut baseline, &identities);

        let table = baseline.get(&key).expect("baseline table");
        assert_eq!(table.stable_key, "ctb.ItemTempTable");
        assert_eq!(table.names, ["ItemTempTable.ctb"]);
        assert_eq!(table.state, "named");
        assert_eq!(table.domain, "items-and-equipment");
    }

    #[test]
    fn identity_overlay_rejects_a_name_that_does_not_hash_to_its_key() {
        let key = hash33("ItemTempTable.ctb");
        let current = BTreeMap::from([(key, current_table(key))]);
        let overlay = IdentityOverlay {
            deployment_id: "global".to_owned(),
            channel: "steam".to_owned(),
            distribution_app_id: "2425200".to_owned(),
            build_id: "24609362".to_owned(),
            identities: vec![CurrentIdentity {
                table_key: key,
                table_name: "SkillTable.ctb".to_owned(),
                stable_key: "ctb.SkillTable".to_owned(),
                domain: "combat".to_owned(),
            }],
        };

        let error =
            validate_identity_overlay(&overlay, "global", "steam", "2425200", "24609362", &current)
                .expect_err("mismatched name must fail");

        assert!(error.contains("does not hash to key"));
    }
}
