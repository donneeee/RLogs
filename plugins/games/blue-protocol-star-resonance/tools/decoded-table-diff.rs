use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    baseline: PathBuf,
    candidate: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct FingerprintCatalog {
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    tables: Vec<TableFingerprint>,
}

#[derive(Debug, Deserialize)]
struct TableFingerprint {
    table_key: u32,
    stable_key: String,
    source: TableVersion,
    schema: BTreeMap<String, BTreeMap<String, usize>>,
    row_fingerprints: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct TableVersion {
    sha256: String,
}

#[derive(Debug, Serialize)]
struct DecodedTableDiff {
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
    tables: Vec<TableDiff>,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_table_key_required: bool,
    raw_rows_embedded: bool,
    unresolved_changes_hidden: bool,
    changed_rows_auto_promoted: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    baseline_tables: usize,
    candidate_tables: usize,
    unchanged_tables: usize,
    changed_tables: usize,
    added_tables: usize,
    removed_tables: usize,
    unchanged_rows: usize,
    changed_rows: usize,
    added_rows: usize,
    removed_rows: usize,
}

#[derive(Debug, Serialize)]
struct TableDiff {
    table_key: u32,
    table_key_hex: String,
    stable_key: String,
    change: &'static str,
    baseline_sha256: Option<String>,
    candidate_sha256: Option<String>,
    schema_changed: bool,
    unchanged_row_count: usize,
    changed_row_ids: Vec<String>,
    added_row_ids: Vec<String>,
    removed_row_ids: Vec<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("decoded table diff failed: {error}");
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
    let baseline: FingerprintCatalog = serde_json::from_slice(&fs::read(&arguments.baseline)?)?;
    let candidate: FingerprintCatalog = serde_json::from_slice(&fs::read(&arguments.candidate)?)?;
    validate_catalog_pair(&baseline, &candidate)?;

    let baseline_tables = keyed_tables(baseline.tables)?;
    let candidate_tables = keyed_tables(candidate.tables)?;
    let all_keys = baseline_tables
        .keys()
        .chain(candidate_tables.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut summary = Summary {
        baseline_tables: baseline_tables.len(),
        candidate_tables: candidate_tables.len(),
        ..Summary::default()
    };
    let mut tables = Vec::new();
    for key in all_keys {
        let old = baseline_tables.get(&key);
        let new = candidate_tables.get(&key);
        if old.is_some_and(|old| new.is_some_and(|new| old.source.sha256 == new.source.sha256)) {
            summary.unchanged_tables += 1;
            summary.unchanged_rows += old.map_or(0, |table| table.row_fingerprints.len());
            continue;
        }
        let table = diff_table(key, old, new)?;
        match table.change {
            "changed" => summary.changed_tables += 1,
            "added" => summary.added_tables += 1,
            "removed" => summary.removed_tables += 1,
            _ => unreachable!(),
        }
        summary.unchanged_rows += table.unchanged_row_count;
        summary.changed_rows += table.changed_row_ids.len();
        summary.added_rows += table.added_row_ids.len();
        summary.removed_rows += table.removed_row_ids.len();
        tables.push(table);
    }

    let output = DecodedTableDiff {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-decoded-table-diff",
        game: "blue-protocol-star-resonance",
        deployment_id: candidate.deployment_id,
        channel: candidate.channel,
        distribution_app_id: candidate.distribution_app_id,
        baseline_build_id: baseline.build_id,
        build_id: candidate.build_id,
        policy: Policy {
            exact_table_key_required: true,
            raw_rows_embedded: false,
            unresolved_changes_hidden: false,
            changed_rows_auto_promoted: false,
            promotion_requirement: "matching-build field review, packet replay, and exact conservation proof",
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

fn validate_catalog_pair(
    baseline: &FingerprintCatalog,
    candidate: &FingerprintCatalog,
) -> Result<(), Box<dyn std::error::Error>> {
    if baseline.build_id == candidate.build_id {
        return Err("baseline and candidate build IDs are identical".into());
    }
    if baseline.deployment_id != candidate.deployment_id
        || baseline.channel != candidate.channel
        || baseline.distribution_app_id != candidate.distribution_app_id
    {
        return Err("baseline and candidate deployment identities differ".into());
    }
    Ok(())
}

fn keyed_tables(
    tables: Vec<TableFingerprint>,
) -> Result<BTreeMap<u32, TableFingerprint>, Box<dyn std::error::Error>> {
    let mut keyed = BTreeMap::new();
    for table in tables {
        if keyed.insert(table.table_key, table).is_some() {
            return Err("duplicate exact table key in fingerprint catalog".into());
        }
    }
    Ok(keyed)
}

fn diff_table(
    key: u32,
    old: Option<&TableFingerprint>,
    new: Option<&TableFingerprint>,
) -> Result<TableDiff, Box<dyn std::error::Error>> {
    if let (Some(old), Some(new)) = (old, new) {
        if old.stable_key != new.stable_key {
            return Err(format!(
                "stable identity changed for exact table key {key}: {} -> {}",
                old.stable_key, new.stable_key
            )
            .into());
        }
    }
    let stable_key = old
        .map(|table| table.stable_key.clone())
        .or_else(|| new.map(|table| table.stable_key.clone()))
        .ok_or("table diff has neither baseline nor candidate")?;
    let row_keys = old
        .into_iter()
        .flat_map(|table| table.row_fingerprints.keys())
        .chain(
            new.into_iter()
                .flat_map(|table| table.row_fingerprints.keys()),
        )
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut unchanged_row_count = 0_usize;
    let mut changed_row_ids = Vec::new();
    let mut added_row_ids = Vec::new();
    let mut removed_row_ids = Vec::new();
    for row_key in row_keys {
        match (
            old.and_then(|table| table.row_fingerprints.get(&row_key)),
            new.and_then(|table| table.row_fingerprints.get(&row_key)),
        ) {
            (Some(left), Some(right)) if left == right => unchanged_row_count += 1,
            (Some(_), Some(_)) => changed_row_ids.push(row_key),
            (None, Some(_)) => added_row_ids.push(row_key),
            (Some(_), None) => removed_row_ids.push(row_key),
            (None, None) => unreachable!(),
        }
    }
    Ok(TableDiff {
        table_key: key,
        table_key_hex: format!("0x{key:08x}"),
        stable_key,
        change: match (old, new) {
            (Some(_), Some(_)) => "changed",
            (None, Some(_)) => "added",
            (Some(_), None) => "removed",
            (None, None) => unreachable!(),
        },
        baseline_sha256: old.map(|table| table.source.sha256.clone()),
        candidate_sha256: new.map(|table| table.source.sha256.clone()),
        schema_changed: match (old, new) {
            (Some(old), Some(new)) => old.schema != new.schema,
            _ => true,
        },
        unchanged_row_count,
        changed_row_ids,
        added_row_ids,
        removed_row_ids,
    })
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut baseline = None;
    let mut candidate = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--baseline" => baseline = Some(PathBuf::from(next_value(&mut args, "--baseline")?)),
            "--candidate" => candidate = Some(PathBuf::from(next_value(&mut args, "--candidate")?)),
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        baseline: baseline.ok_or("missing --baseline")?,
        candidate: candidate.ok_or("missing --candidate")?,
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

    fn table(key: u32, stable_key: &str, sha: &str, rows: &[(&str, &str)]) -> TableFingerprint {
        TableFingerprint {
            table_key: key,
            stable_key: stable_key.to_owned(),
            source: TableVersion {
                sha256: sha.to_owned(),
            },
            schema: BTreeMap::new(),
            row_fingerprints: rows
                .iter()
                .map(|(id, digest)| ((*id).to_owned(), (*digest).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn row_diff_is_exact_and_conserving() {
        let old = table(1, "ctb.Test", "old", &[("1", "a"), ("2", "b"), ("3", "c")]);
        let new = table(1, "ctb.Test", "new", &[("1", "a"), ("2", "z"), ("4", "d")]);
        let diff = diff_table(1, Some(&old), Some(&new)).unwrap();
        assert_eq!(diff.unchanged_row_count, 1);
        assert_eq!(diff.changed_row_ids, vec!["2"]);
        assert_eq!(diff.added_row_ids, vec!["4"]);
        assert_eq!(diff.removed_row_ids, vec!["3"]);
    }

    #[test]
    fn exact_key_cannot_silently_change_stable_identity() {
        let old = table(1, "ctb.Left", "old", &[]);
        let new = table(1, "ctb.Right", "new", &[]);
        assert!(diff_table(1, Some(&old), Some(&new)).is_err());
    }

    #[test]
    fn identical_builds_are_rejected() {
        let catalog = |build: &str| FingerprintCatalog {
            deployment_id: "global".to_owned(),
            channel: "steam".to_owned(),
            distribution_app_id: "3681810".to_owned(),
            build_id: build.to_owned(),
            tables: Vec::new(),
        };
        assert!(validate_catalog_pair(&catalog("1"), &catalog("1")).is_err());
    }
}
