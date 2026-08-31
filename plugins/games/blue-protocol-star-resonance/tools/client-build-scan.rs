use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineFile {
    relative_path: String,
    bytes: u64,
    #[serde(rename = "modified_utc")]
    _modified_utc: String,
    stable_during_scan: bool,
    #[serde(rename = "extension")]
    _extension: String,
    #[serde(rename = "signature")]
    _signature: String,
    sha256: String,
}

#[derive(Debug, Clone)]
struct BaselineEntry {
    family: String,
    file: BaselineFile,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct BuildSourceDiff {
    schema_version: u16,
    generated_by: String,
    game: String,
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    baseline_build_id: String,
    build_id: String,
    policy: ScanPolicy,
    summary: DiffSummary,
    changed_families: Vec<String>,
    required_followup_suites: Vec<String>,
    added: Vec<FileChange>,
    removed: Vec<FileChange>,
    changed: Vec<FileChange>,
    unstable: Vec<FileChange>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ScanPolicy {
    read_only: bool,
    extraction_outside_live_parser: bool,
    candidate_never_auto_promoted: bool,
    packet_replay_required: bool,
    unresolved_evidence_retained: bool,
    volatile_content_hashed: bool,
    absolute_paths_exported: bool,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct DiffSummary {
    baseline_files: usize,
    current_files: usize,
    ignored_volatile_files: usize,
    unchanged_files: usize,
    added_files: usize,
    removed_files: usize,
    changed_files: usize,
    unstable_files: usize,
    bytes_hashed: u64,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct FileChange {
    relative_path: String,
    family: String,
    old_bytes: Option<u64>,
    new_bytes: Option<u64>,
    old_sha256: Option<String>,
    new_sha256: Option<String>,
    stable_during_scan: bool,
}

#[derive(Debug)]
struct CurrentFile {
    relative_path: String,
    path: PathBuf,
    bytes: u64,
}

#[derive(Debug)]
struct HashedFile {
    bytes: u64,
    sha256: String,
    stable: bool,
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let options = parse_options(&arguments)?;
    let install_root = required_path(&options, "install-root")?;
    let manifest_path = required_path(&options, "steam-manifest")?;
    let baseline_physical = required_path(&options, "baseline-physical")?;
    let baseline_build = required(&options, "baseline-build")?.to_owned();
    let deployment = required(&options, "deployment")?.to_owned();
    let channel = required(&options, "channel")?.to_owned();
    let output = required_path(&options, "output")?;

    if output.exists() {
        return Err(format!("refusing to overwrite existing scan: {}", output.display()).into());
    }
    if !install_root.is_dir() {
        return Err(format!(
            "install root is not a directory: {}",
            install_root.display()
        )
        .into());
    }

    let manifest = fs::read_to_string(&manifest_path)?;
    let build_id = acf_value(&manifest, "buildid")
        .ok_or("Steam manifest does not contain an exact buildid")?;
    let app_id =
        acf_value(&manifest, "appid").ok_or("Steam manifest does not contain an exact appid")?;
    validate_build_id(&build_id)?;
    if build_id == baseline_build {
        return Err("candidate build matches the baseline build; no update scan is needed".into());
    }

    let baseline = load_baseline(&baseline_physical)?;
    let current = enumerate_current(&install_root)?;
    let current_by_path: BTreeMap<_, _> = current
        .iter()
        .map(|file| (file.relative_path.as_str(), file))
        .collect();
    let ignored_volatile_files = current
        .iter()
        .filter(|file| is_volatile_path(&file.relative_path))
        .count();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unstable = Vec::new();
    let mut unchanged_files = 0_usize;
    let mut bytes_hashed = 0_u64;
    let mut changed_families = BTreeSet::new();

    for (relative_path, old) in &baseline {
        let Some(new) = current_by_path.get(relative_path.as_str()) else {
            changed_families.insert(old.family.clone());
            removed.push(change(Some(old), None, true));
            continue;
        };
        if old.family == "volatile_private_log" {
            continue;
        }
        let hashed = hash_stable(&new.path)?;
        bytes_hashed = bytes_hashed.saturating_add(hashed.bytes);
        let record = change_with_hash(old, new, &hashed);
        if !hashed.stable {
            changed_families.insert(old.family.clone());
            unstable.push(record);
        } else if old.file.bytes == hashed.bytes
            && normalize_hash(&old.file.sha256) == hashed.sha256
        {
            unchanged_files += 1;
        } else {
            changed_families.insert(old.family.clone());
            changed.push(record);
        }
    }

    for new in &current {
        if baseline.contains_key(&new.relative_path) || is_volatile_path(&new.relative_path) {
            continue;
        }
        let hashed = hash_stable(&new.path)?;
        bytes_hashed = bytes_hashed.saturating_add(hashed.bytes);
        let family = classify_new_file(&new.relative_path);
        changed_families.insert(family.clone());
        let record = FileChange {
            relative_path: new.relative_path.clone(),
            family,
            old_bytes: None,
            new_bytes: Some(hashed.bytes),
            old_sha256: None,
            new_sha256: Some(hashed.sha256),
            stable_during_scan: hashed.stable,
        };
        if hashed.stable {
            added.push(record);
        } else {
            unstable.push(record);
        }
    }

    sort_changes(&mut added);
    sort_changes(&mut removed);
    sort_changes(&mut changed);
    sort_changes(&mut unstable);
    let required_followup_suites = followup_suites(&changed_families);
    let report = BuildSourceDiff {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-client-build-scan".into(),
        game: "blue-protocol-star-resonance".into(),
        deployment_id: deployment,
        channel,
        distribution_app_id: app_id,
        baseline_build_id: baseline_build,
        build_id,
        policy: ScanPolicy {
            read_only: true,
            extraction_outside_live_parser: true,
            candidate_never_auto_promoted: true,
            packet_replay_required: true,
            unresolved_evidence_retained: true,
            volatile_content_hashed: false,
            absolute_paths_exported: false,
        },
        summary: DiffSummary {
            baseline_files: baseline.len(),
            current_files: current.len(),
            ignored_volatile_files,
            unchanged_files,
            added_files: added.len(),
            removed_files: removed.len(),
            changed_files: changed.len(),
            unstable_files: unstable.len(),
            bytes_hashed,
        },
        changed_families: changed_families.into_iter().collect(),
        required_followup_suites,
        added,
        removed,
        changed,
        unstable,
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", output.display());
    Ok(())
}

fn load_baseline(root: &Path) -> Result<BTreeMap<String, BaselineEntry>, Box<dyn Error>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let family = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("baseline family file has no UTF-8 stem")?
            .to_owned();
        let files: Vec<BaselineFile> = serde_json::from_slice(&fs::read(&path)?)?;
        for file in files {
            if !file.stable_during_scan {
                return Err(
                    format!("baseline contains unstable file {}", file.relative_path).into(),
                );
            }
            let key = normalize_relative(&file.relative_path);
            if result
                .insert(
                    key.clone(),
                    BaselineEntry {
                        family: family.clone(),
                        file,
                    },
                )
                .is_some()
            {
                return Err(format!("duplicate baseline path {key}").into());
            }
        }
    }
    if result.is_empty() {
        return Err("baseline physical inventory is empty".into());
    }
    Ok(result)
}

fn enumerate_current(root: &Path) -> Result<Vec<CurrentFile>, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_current(root, root, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn collect_current(
    root: &Path,
    directory: &Path,
    output: &mut Vec<CurrentFile>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_current(root, &path, output)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            output.push(CurrentFile {
                relative_path: normalize_relative(&relative),
                bytes: entry.metadata()?.len(),
                path,
            });
        }
    }
    Ok(())
}

fn hash_stable(path: &Path) -> Result<HashedFile, Box<dyn Error>> {
    let before = fs::metadata(path)?;
    let before_modified = before.modified().ok();
    let mut reader = BufReader::with_capacity(8 * 1024 * 1024, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 8 * 1024 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let after = fs::metadata(path)?;
    let stable = before.len() == after.len() && before_modified == after.modified().ok();
    Ok(HashedFile {
        bytes: after.len(),
        sha256: digest_hex(&hasher.finalize()),
        stable,
    })
}

fn change(old: Option<&BaselineEntry>, new: Option<&CurrentFile>, stable: bool) -> FileChange {
    FileChange {
        relative_path: old
            .map(|entry| normalize_relative(&entry.file.relative_path))
            .or_else(|| new.map(|entry| entry.relative_path.clone()))
            .expect("change has one side"),
        family: old
            .map(|entry| entry.family.clone())
            .unwrap_or_else(|| classify_new_file(&new.expect("new file").relative_path)),
        old_bytes: old.map(|entry| entry.file.bytes),
        new_bytes: new.map(|entry| entry.bytes),
        old_sha256: old.map(|entry| normalize_hash(&entry.file.sha256)),
        new_sha256: None,
        stable_during_scan: stable,
    }
}

fn change_with_hash(old: &BaselineEntry, new: &CurrentFile, hashed: &HashedFile) -> FileChange {
    let mut record = change(Some(old), Some(new), hashed.stable);
    record.new_bytes = Some(hashed.bytes);
    record.new_sha256 = Some(hashed.sha256.clone());
    record
}

fn followup_suites(families: &BTreeSet<String>) -> Vec<String> {
    let mut suites = BTreeSet::from(["build-identity".to_owned(), "packet-replay".to_owned()]);
    for family in families {
        match family.as_str() {
            "container_index" | "container_package" | "container_auxiliary" => {
                suites.extend([
                    "asset-catalog-diff".into(),
                    "combat-table-diff".into(),
                    "localization-diff".into(),
                    "schema-diff".into(),
                ]);
            }
            "il2cpp_native_code"
            | "il2cpp_metadata"
            | "protected_client_base"
            | "native_executable" => {
                suites.extend([
                    "formula-surface-diff".into(),
                    "protobuf-coverage".into(),
                    "runtime-conservation".into(),
                ]);
            }
            "unity_player_data" => {
                suites.extend(["asset-catalog-diff".into(), "schema-diff".into()]);
            }
            _ => {}
        }
    }
    suites.into_iter().collect()
}

fn classify_new_file(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.contains("/streamingassets/container/m") && lower.ends_with(".pkg") {
        "container_package"
    } else if lower.ends_with("/streamingassets/container/meta.pkg") {
        "container_index"
    } else if lower.ends_with("gameassembly.dll") {
        "il2cpp_native_code"
    } else if lower.ends_with("global-metadata.dat") {
        "il2cpp_metadata"
    } else if lower.ends_with(".assets") || lower.contains("globalgamemanagers") {
        "unity_player_data"
    } else if lower.ends_with(".exe") {
        "native_executable"
    } else if lower.ends_with(".dll") {
        "native_plugin"
    } else {
        "unclassified_new_file"
    }
    .into()
}

fn is_volatile_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".log")
        || lower.contains("/logs/")
        || lower.contains("/log/")
        || lower.contains("/gvoicetqos/")
}

fn normalize_relative(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

fn normalize_hash(value: &str) -> String {
    value
        .strip_prefix("sha256:")
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn sort_changes(changes: &mut [FileChange]) {
    changes.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
}

fn acf_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let trimmed = line.trim();
        let pieces: Vec<_> = trimmed.split('"').collect();
        let found_key = *pieces.get(1)?;
        let value = *pieces.get(3)?;
        (found_key == key).then(|| value.trim().to_owned())
    })
}

fn validate_build_id(value: &str) -> Result<(), Box<dyn Error>> {
    if value.is_empty() || !value.chars().all(|character| character.is_ascii_digit()) {
        return Err("Steam buildid must contain only decimal digits".into());
    }
    Ok(())
}

fn parse_options(arguments: &[String]) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        let name = arguments[index]
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected argument {}", arguments[index]))?;
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| format!("missing value for --{name}"))?;
        if options.insert(name.to_owned(), value.clone()).is_some() {
            return Err(format!("duplicate option --{name}").into());
        }
        index += 2;
    }
    Ok(options)
}

fn required<'a>(
    options: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, Box<dyn Error>> {
    options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("missing --{name}").into())
}

fn required_path(
    options: &BTreeMap<String, String>,
    name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(required(options, name)?))
}

fn digest_hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exact_manifest_keys() {
        let manifest =
            "\"appid\"\t\t\"3681810\"\n\"buildid\"\t\t\"24568685\"\n\"TargetBuildID\"\t\t\"999\"";
        assert_eq!(acf_value(manifest, "appid").as_deref(), Some("3681810"));
        assert_eq!(acf_value(manifest, "buildid").as_deref(), Some("24568685"));
    }

    #[test]
    fn classifies_update_critical_sources() {
        assert_eq!(
            classify_new_file("bpsr/BPSR_STEAM_Data/StreamingAssets/container/m4.pkg"),
            "container_package"
        );
        assert_eq!(
            classify_new_file("bpsr/GameAssembly.dll"),
            "il2cpp_native_code"
        );
        assert_eq!(
            classify_new_file("bpsr/BPSR_STEAM_Data/resources.assets"),
            "unity_player_data"
        );
    }

    #[test]
    fn routes_container_and_native_changes_to_required_proofs() {
        let suites = followup_suites(&BTreeSet::from([
            "container_package".into(),
            "il2cpp_native_code".into(),
        ]));
        assert!(suites.contains(&"combat-table-diff".into()));
        assert!(suites.contains(&"formula-surface-diff".into()));
        assert!(suites.contains(&"packet-replay".into()));
        assert!(suites.contains(&"runtime-conservation".into()));
    }

    #[test]
    fn volatile_content_is_not_a_build_input() {
        assert!(is_volatile_path("bpsr/logs/output.log"));
        assert!(is_volatile_path("bpsr/GVoiceTQos/Room_6001_396288366.tdr"));
        assert!(!is_volatile_path("bpsr/GameAssembly.dll"));
    }
}
