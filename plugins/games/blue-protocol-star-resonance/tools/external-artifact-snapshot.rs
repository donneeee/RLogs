use std::{
    collections::BTreeMap,
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const MANIFEST_SCHEMA_VERSION: u16 = 1;
const SNAPSHOT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u16,
    source_name: String,
    policy: ManifestPolicy,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestPolicy {
    extraction_runs_outside_live_parser: bool,
    fingerprints_are_build_scoped: bool,
    absolute_paths_exported: bool,
    missing_required_artifacts_fail: bool,
    artifacts_are_runtime_authority: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    id: String,
    path: String,
    role: String,
    required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u16,
    generated_by: String,
    source_name: String,
    game_build: String,
    promotion_state: String,
    policy: ManifestPolicy,
    summary: SnapshotSummary,
    aggregate_sha256: String,
    artifacts: Vec<Artifact>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotSummary {
    declared_artifacts: usize,
    present_artifacts: usize,
    missing_optional_artifacts: usize,
    total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    id: String,
    path: String,
    role: String,
    required: bool,
    bytes: u64,
    sha256: String,
    json_metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct SnapshotDiff {
    schema_version: u16,
    generated_by: &'static str,
    baseline_build: String,
    candidate_build: String,
    build_identity_changed: bool,
    summary: DiffSummary,
    added: Vec<ArtifactChange>,
    removed: Vec<ArtifactChange>,
    changed: Vec<ArtifactChange>,
    unchanged_artifact_ids: Vec<String>,
    requires_reproof: bool,
    runtime_promotion_allowed: bool,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct DiffSummary {
    baseline_artifacts: usize,
    candidate_artifacts: usize,
    added_artifacts: usize,
    removed_artifacts: usize,
    changed_artifacts: usize,
    unchanged_artifacts: usize,
    baseline_bytes: u64,
    candidate_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactChange {
    id: String,
    path: String,
    role: String,
    old_bytes: Option<u64>,
    new_bytes: Option<u64>,
    old_sha256: Option<String>,
    new_sha256: Option<String>,
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let options = parse_options(&arguments)?;
    if options.contains_key("baseline") || options.contains_key("candidate") {
        return diff_snapshots(&options);
    }
    let manifest_path = required_path(&options, "manifest")?;
    let source_root = required_path(&options, "root")?;
    let game_build = required(&options, "build")?.to_owned();
    let output_path = required_path(&options, "output")?;
    validate_build(&game_build)?;

    let manifest: Manifest = read_json(&manifest_path)?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "unsupported manifest schema {}; expected {MANIFEST_SCHEMA_VERSION}",
            manifest.schema_version
        )
        .into());
    }
    if manifest.policy.absolute_paths_exported
        || manifest.policy.artifacts_are_runtime_authority
        || !manifest.policy.extraction_runs_outside_live_parser
        || !manifest.policy.fingerprints_are_build_scoped
    {
        return Err("manifest violates the external candidate-artifact boundary".into());
    }

    let mut artifacts = Vec::new();
    let mut missing_optional_artifacts = 0usize;
    let mut total_bytes = 0u64;
    let mut aggregate = Sha256::new();

    for declared in manifest.files {
        validate_relative_path(&declared.path)?;
        let path = source_root.join(Path::new(&declared.path));
        if !path.is_file() {
            if declared.required && manifest.policy.missing_required_artifacts_fail {
                return Err(
                    format!("required external artifact is missing: {}", declared.path).into(),
                );
            }
            missing_optional_artifacts += 1;
            continue;
        }

        let (bytes, sha256) = hash_file(&path)?;
        let metadata = read_top_level_metadata(&path)?;
        aggregate.update(declared.id.as_bytes());
        aggregate.update([0]);
        aggregate.update(declared.path.as_bytes());
        aggregate.update([0]);
        aggregate.update(bytes.to_le_bytes());
        aggregate.update([0]);
        aggregate.update(sha256.as_bytes());
        aggregate.update([0]);
        total_bytes = total_bytes.saturating_add(bytes);
        artifacts.push(Artifact {
            id: declared.id,
            path: declared.path.replace('\\', "/"),
            role: declared.role,
            required: declared.required,
            bytes,
            sha256,
            json_metadata: metadata,
        });
    }

    artifacts.sort_by(|left, right| left.id.cmp(&right.id));
    let snapshot = Snapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-external-artifact-snapshot".to_owned(),
        source_name: manifest.source_name,
        game_build,
        promotion_state: "candidate-only-not-runtime-authority".to_owned(),
        policy: manifest.policy,
        summary: SnapshotSummary {
            declared_artifacts: artifacts.len() + missing_optional_artifacts,
            present_artifacts: artifacts.len(),
            missing_optional_artifacts,
            total_bytes,
        },
        aggregate_sha256: hex(&aggregate.finalize()),
        artifacts,
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(output_path)?);
    serde_json::to_writer_pretty(&mut writer, &snapshot)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn diff_snapshots(options: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let baseline_path = required_path(options, "baseline")?;
    let candidate_path = required_path(options, "candidate")?;
    let output_path = required_path(options, "output")?;
    let baseline: Snapshot = read_json(&baseline_path)?;
    let candidate: Snapshot = read_json(&candidate_path)?;

    let baseline_by_id = baseline
        .artifacts
        .iter()
        .map(|artifact| (artifact.id.as_str(), artifact))
        .collect::<BTreeMap<_, _>>();
    let candidate_by_id = candidate
        .artifacts
        .iter()
        .map(|artifact| (artifact.id.as_str(), artifact))
        .collect::<BTreeMap<_, _>>();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged_artifact_ids = Vec::new();

    for (id, current) in &candidate_by_id {
        let Some(previous) = baseline_by_id.get(id) else {
            added.push(change(None, Some(current)));
            continue;
        };
        if previous.sha256 == current.sha256 {
            unchanged_artifact_ids.push((*id).to_owned());
        } else {
            changed.push(change(Some(previous), Some(current)));
        }
    }
    for (id, previous) in &baseline_by_id {
        if !candidate_by_id.contains_key(id) {
            removed.push(change(Some(previous), None));
        }
    }

    let requires_reproof = baseline.game_build != candidate.game_build
        || !added.is_empty()
        || !removed.is_empty()
        || !changed.is_empty();
    let diff = SnapshotDiff {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-external-artifact-snapshot",
        baseline_build: baseline.game_build.clone(),
        candidate_build: candidate.game_build.clone(),
        build_identity_changed: baseline.game_build != candidate.game_build,
        summary: DiffSummary {
            baseline_artifacts: baseline.artifacts.len(),
            candidate_artifacts: candidate.artifacts.len(),
            added_artifacts: added.len(),
            removed_artifacts: removed.len(),
            changed_artifacts: changed.len(),
            unchanged_artifacts: unchanged_artifact_ids.len(),
            baseline_bytes: baseline.summary.total_bytes,
            candidate_bytes: candidate.summary.total_bytes,
        },
        added,
        removed,
        changed,
        unchanged_artifact_ids,
        requires_reproof,
        runtime_promotion_allowed: false,
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(output_path)?);
    serde_json::to_writer_pretty(&mut writer, &diff)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn change(previous: Option<&Artifact>, current: Option<&Artifact>) -> ArtifactChange {
    let identity = current.or(previous).expect("change has an artifact");
    ArtifactChange {
        id: identity.id.clone(),
        path: identity.path.clone(),
        role: identity.role.clone(),
        old_bytes: previous.map(|artifact| artifact.bytes),
        new_bytes: current.map(|artifact| artifact.bytes),
        old_sha256: previous.map(|artifact| artifact.sha256.clone()),
        new_sha256: current.map(|artifact| artifact.sha256.clone()),
    }
}

fn parse_options(arguments: &[String]) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let mut options = BTreeMap::new();
    let mut index = 0usize;
    while index < arguments.len() {
        let key = arguments[index]
            .strip_prefix("--")
            .ok_or_else(usage)?
            .to_owned();
        let value = arguments.get(index + 1).ok_or_else(usage)?.to_owned();
        if options.insert(key, value).is_some() {
            return Err("duplicate option".into());
        }
        index += 2;
    }
    Ok(options)
}

fn required<'a>(
    options: &'a BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, Box<dyn Error>> {
    options
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| usage().into())
}

fn required_path(options: &BTreeMap<String, String>, key: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(required(options, key)?))
}

fn validate_build(build: &str) -> Result<(), Box<dyn Error>> {
    if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("build must contain ASCII digits only".into());
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), Box<dyn Error>> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("artifact paths must stay relative to the declared root".into());
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(u64, String), Box<dyn Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut bytes = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        bytes = bytes.saturating_add(u64::try_from(read)?);
    }
    Ok((bytes, hex(&digest.finalize())))
}

fn read_top_level_metadata(path: &Path) -> Result<BTreeMap<String, Value>, Box<dyn Error>> {
    let value: Value = read_json(path)?;
    let mut metadata = BTreeMap::new();
    let Some(object) = value.as_object() else {
        return Ok(metadata);
    };
    for key in [
        "schema_version",
        "schemaVersion",
        "generated_by",
        "generatedBy",
        "game_build",
        "gameBuild",
        "client_build",
        "clientBuild",
        "policy",
        "summary",
        "stats",
    ] {
        if let Some(value) = object.get(key) {
            metadata.insert(key.to_owned(), value.clone());
        }
    }
    Ok(metadata)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn usage() -> &'static str {
    "usage:\n  rlogs-bpsr-external-artifact-snapshot --manifest <manifest.json> --root <external-output-directory> --build <client-build> --output <snapshot.json>\n  rlogs-bpsr-external-artifact-snapshot --baseline <snapshot.json> --candidate <snapshot.json> --output <diff.json>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_traversal() {
        assert!(validate_relative_path("../output.json").is_err());
        assert!(validate_relative_path("nested/output.json").is_ok());
    }

    #[test]
    fn build_is_numeric() {
        assert!(validate_build("24568685").is_ok());
        assert!(validate_build("steam-24568685").is_err());
    }
}
