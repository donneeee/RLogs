use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const PLAN_SCHEMA_VERSION: u16 = 1;
const SNAPSHOT_SCHEMA_VERSION: u16 = 1;
const DIFF_SCHEMA_VERSION: u16 = 1;
const PROOF_MANIFEST_SCHEMA_VERSION: u16 = 1;
const GATE_SCHEMA_VERSION: u16 = 1;
const UPDATE_WORKLIST_SCHEMA_VERSION: u16 = 1;
const PREFLIGHT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditPlan {
    schema_version: u16,
    game: String,
    deployment: String,
    channel: String,
    policy: PlanPolicy,
    inputs: Vec<PlanInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanPolicy {
    extraction_runs_outside_live_parser: bool,
    candidate_data_never_auto_promoted: bool,
    packet_replay_required_for_runtime_rules: bool,
    exact_party_conservation_required: bool,
    canonical_events_retained: bool,
    unresolved_events_hidden: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanInput {
    id: String,
    path: String,
    role: String,
    #[serde(default)]
    identity_manifest: Option<String>,
    #[serde(default = "default_input_domain")]
    domain: String,
    #[serde(default)]
    change_actions: Vec<String>,
    required: bool,
    runtime_authority: bool,
    proof_suites: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct InputSnapshot {
    id: String,
    path: String,
    role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity_manifest: Option<String>,
    #[serde(default = "default_input_domain")]
    domain: String,
    #[serde(default)]
    change_actions: Vec<String>,
    required: bool,
    runtime_authority: bool,
    proof_suites: Vec<String>,
    file_count: usize,
    byte_count: u64,
    sha256: String,
    json_metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildSnapshot {
    schema_version: u16,
    generated_by: String,
    game: String,
    deployment: String,
    channel: String,
    game_build: String,
    promotion_state: String,
    policy: SnapshotPolicy,
    inputs: Vec<InputSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotPolicy {
    extraction_runs_outside_live_parser: bool,
    candidate_data_never_auto_promoted: bool,
    packet_replay_required_for_runtime_rules: bool,
    exact_party_conservation_required: bool,
    canonical_events_retained: bool,
    unresolved_events_hidden: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildDiff {
    schema_version: u16,
    generated_by: String,
    baseline_build: String,
    candidate_build: String,
    build_identity_changed: bool,
    added_inputs: Vec<InputChange>,
    removed_inputs: Vec<InputChange>,
    changed_inputs: Vec<InputChange>,
    #[serde(default)]
    changed_domains: Vec<String>,
    #[serde(default)]
    domain_actions: BTreeMap<String, Vec<String>>,
    unchanged_input_count: usize,
    requires_reproof: bool,
    required_proof_suites: Vec<String>,
    runtime_promotion_allowed: bool,
    candidate_snapshot: BuildSnapshot,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputChange {
    id: String,
    #[serde(default = "default_input_domain")]
    domain: String,
    #[serde(default)]
    change_actions: Vec<String>,
    old_sha256: Option<String>,
    new_sha256: Option<String>,
    proof_suites: Vec<String>,
    metadata_changes: BTreeMap<String, MetadataChange>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataChange {
    old: Option<Value>,
    new: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofManifest {
    schema_version: u16,
    game_build: String,
    review_state: String,
    canonical_events_retained: bool,
    unresolved_events_hidden: bool,
    suites: Vec<ProofSuite>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofSuite {
    id: String,
    status: String,
    exact_party_conservation: bool,
    observed_event_count: u64,
    report_path: String,
    report_sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct PromotionGate {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    required_proof_suites: Vec<String>,
    verified_proof_suites: Vec<String>,
    canonical_events_retained: bool,
    unresolved_events_hidden: bool,
    exact_party_conservation: bool,
    runtime_promotion_allowed: bool,
    blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct UpdateWorklist {
    schema_version: u16,
    generated_by: String,
    baseline_build: String,
    candidate_build: String,
    candidate_snapshot_path: String,
    build_diff_path: String,
    build_identity_changed: bool,
    changed_input_ids: Vec<String>,
    changed_domains: Vec<String>,
    domain_actions: BTreeMap<String, Vec<String>>,
    runtime_authority_changed_input_ids: Vec<String>,
    required_proof_suites: Vec<String>,
    runtime_data_review_required: bool,
    stable_algorithm_review_required: bool,
    runtime_promotion_allowed: bool,
    next_actions: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct BuildPreflight {
    schema_version: u16,
    generated_by: String,
    game: String,
    deployment: String,
    channel: String,
    game_build: String,
    policy: PreflightPolicy,
    summary: PreflightSummary,
    inputs: Vec<PreflightInput>,
    required_proof_suites_from_missing_inputs: Vec<String>,
    ready_for_snapshot: bool,
    runtime_promotion_allowed: bool,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct PreflightPolicy {
    hashes_or_decodes_artifacts: bool,
    missing_required_inputs_hidden: bool,
    missing_optional_inputs_are_promotion_proof: bool,
    purpose: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct PreflightSummary {
    planned_inputs: usize,
    present_required_inputs: usize,
    missing_required_inputs: usize,
    present_optional_inputs: usize,
    missing_optional_inputs: usize,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct PreflightInput {
    id: String,
    path: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_manifest: Option<String>,
    domain: String,
    change_actions: Vec<String>,
    required: bool,
    runtime_authority: bool,
    proof_suites: Vec<String>,
    status: String,
    filesystem_kind: Option<String>,
}

fn default_input_domain() -> String {
    "shared".into()
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage().into());
    };
    let options = parse_options(&arguments[1..])?;
    match command {
        "preflight" => preflight_command(&options),
        "prepare" => prepare_command(&options),
        "snapshot" => snapshot_command(&options),
        "diff" => diff_command(&options),
        "gate" => gate_command(&options),
        _ => Err(usage().into()),
    }
}

fn preflight_command(options: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let plan_path = required_path(options, "plan")?;
    let root = required_path(options, "root")?;
    let output = required_path(options, "output")?;
    let game_build = required(options, "build")?.to_owned();
    validate_build_path_component(&game_build)?;
    let plan: AuditPlan = read_json(&plan_path)?;
    validate_plan(&plan)?;

    let preflight = build_preflight(plan, &root, game_build)?;
    write_json(&output, &preflight)?;
    println!("{}", output.display());
    Ok(())
}

fn build_preflight(
    plan: AuditPlan,
    root: &Path,
    game_build: String,
) -> Result<BuildPreflight, Box<dyn Error>> {
    let mut summary = PreflightSummary {
        planned_inputs: plan.inputs.len(),
        ..PreflightSummary::default()
    };
    let mut inputs = Vec::with_capacity(plan.inputs.len());
    let mut missing_required_suites = BTreeSet::new();
    let mut seen = BTreeSet::new();

    for input in plan.inputs {
        if !seen.insert(input.id.clone()) {
            return Err(format!("duplicate audit input id {}", input.id).into());
        }
        let resolved_input_path = input.path.replace("{build}", &game_build);
        let path = root.join(&resolved_input_path);
        let identity_manifest_present = input
            .identity_manifest
            .as_deref()
            .is_none_or(|manifest| path.is_dir() && path.join(manifest).is_file());
        let (status, filesystem_kind) = if path.is_file() {
            if !identity_manifest_present {
                if input.required {
                    summary.missing_required_inputs += 1;
                    missing_required_suites.extend(input.proof_suites.iter().cloned());
                } else {
                    summary.missing_optional_inputs += 1;
                }
                ("missing-identity-manifest", Some("file".to_owned()))
            } else if input.required {
                summary.present_required_inputs += 1;
                ("present", Some("file".to_owned()))
            } else {
                summary.present_optional_inputs += 1;
                ("present", Some("file".to_owned()))
            }
        } else if path.is_dir() {
            if !identity_manifest_present {
                if input.required {
                    summary.missing_required_inputs += 1;
                    missing_required_suites.extend(input.proof_suites.iter().cloned());
                } else {
                    summary.missing_optional_inputs += 1;
                }
                ("missing-identity-manifest", Some("directory".to_owned()))
            } else if input.required {
                summary.present_required_inputs += 1;
                ("present", Some("directory".to_owned()))
            } else {
                summary.present_optional_inputs += 1;
                ("present", Some("directory".to_owned()))
            }
        } else {
            if input.required {
                summary.missing_required_inputs += 1;
                missing_required_suites.extend(input.proof_suites.iter().cloned());
            } else {
                summary.missing_optional_inputs += 1;
            }
            ("missing", None)
        };
        inputs.push(PreflightInput {
            id: input.id,
            path: resolved_input_path,
            role: input.role,
            identity_manifest: input.identity_manifest,
            domain: input.domain,
            change_actions: sorted_unique(input.change_actions),
            required: input.required,
            runtime_authority: input.runtime_authority,
            proof_suites: sorted_unique(input.proof_suites),
            status: status.to_owned(),
            filesystem_kind,
        });
    }
    inputs.sort_by(|left, right| left.id.cmp(&right.id));
    let ready_for_snapshot = summary.missing_required_inputs == 0;
    Ok(BuildPreflight {
        schema_version: PREFLIGHT_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-build-audit".into(),
        game: plan.game,
        deployment: plan.deployment,
        channel: plan.channel,
        game_build,
        policy: PreflightPolicy {
            hashes_or_decodes_artifacts: false,
            missing_required_inputs_hidden: false,
            missing_optional_inputs_are_promotion_proof: false,
            purpose: "path-completeness inventory before the fail-closed snapshot and diff".into(),
        },
        summary,
        inputs,
        required_proof_suites_from_missing_inputs: missing_required_suites.into_iter().collect(),
        ready_for_snapshot,
        runtime_promotion_allowed: false,
    })
}

fn prepare_command(options: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let plan = required(options, "plan")?.to_owned();
    let root = required(options, "root")?.to_owned();
    let baseline = required(options, "baseline")?.to_owned();
    let game_build = required(options, "build")?.to_owned();
    let output_root = required_path(options, "output-dir")?;
    validate_build_path_component(&game_build)?;

    let build_output = output_root.join(&game_build);
    let candidate_path = build_output.join("candidate-snapshot.json");
    let diff_path = build_output.join("build-diff.json");
    let worklist_path = build_output.join("proof-worklist.json");

    let snapshot_options = BTreeMap::from([
        ("plan".into(), plan),
        ("root".into(), root),
        ("build".into(), game_build),
        ("state".into(), "candidate".into()),
        (
            "output".into(),
            candidate_path.to_string_lossy().into_owned(),
        ),
    ]);
    snapshot_command(&snapshot_options)?;

    let diff_options = BTreeMap::from([
        ("baseline".into(), baseline.clone()),
        (
            "candidate".into(),
            candidate_path.to_string_lossy().into_owned(),
        ),
        ("output".into(), diff_path.to_string_lossy().into_owned()),
    ]);
    diff_command(&diff_options)?;

    let baseline_snapshot: BuildSnapshot = read_json(Path::new(&baseline))?;
    let diff: BuildDiff = read_json(&diff_path)?;
    let worklist = update_worklist(&baseline_snapshot, &diff);
    write_json(&worklist_path, &worklist)?;
    println!("{}", worklist_path.display());
    Ok(())
}

fn validate_build_path_component(game_build: &str) -> Result<(), Box<dyn Error>> {
    if game_build.is_empty()
        || matches!(game_build, "." | "..")
        || !game_build.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err("--build must be a safe build identifier".into());
    }
    Ok(())
}

fn update_worklist(baseline: &BuildSnapshot, diff: &BuildDiff) -> UpdateWorklist {
    let changes: Vec<_> = diff
        .added_inputs
        .iter()
        .chain(&diff.removed_inputs)
        .chain(&diff.changed_inputs)
        .collect();
    let changed_input_ids: Vec<_> = changes
        .iter()
        .map(|change| change.id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let changed_domains = changes
        .iter()
        .map(|change| change.domain.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut domain_actions = BTreeMap::<String, BTreeSet<String>>::new();
    for change in &changes {
        domain_actions
            .entry(change.domain.clone())
            .or_default()
            .extend(change.change_actions.iter().cloned());
    }
    let domain_actions = domain_actions
        .into_iter()
        .map(|(domain, actions)| (domain, actions.into_iter().collect()))
        .collect::<BTreeMap<_, _>>();
    let runtime_authority: BTreeMap<_, _> = baseline
        .inputs
        .iter()
        .chain(&diff.candidate_snapshot.inputs)
        .map(|input| (input.id.as_str(), input.runtime_authority))
        .collect();
    let runtime_authority_changed_input_ids: Vec<_> = changed_input_ids
        .iter()
        .filter(|id| runtime_authority.get(id.as_str()).copied().unwrap_or(false))
        .cloned()
        .collect();
    let stable_algorithm_review_required = changed_input_ids.iter().any(|id| {
        matches!(
            id.as_str(),
            "formula-proof-ledgers"
                | "game-data-build-pipeline"
                | "rdps-proof-tooling"
                | "rdps-formula-algorithms"
                | "rdps-state-projector"
                | "rdps-runtime-pack-validator"
                | "damage-attribute-stage-runtime"
                | "external-state-runtime"
                | "target-vulnerability-runtime"
        )
    });
    let runtime_data_review_required =
        diff.build_identity_changed || !runtime_authority_changed_input_ids.is_empty();
    let mut next_actions = vec![
        "Review changed-input metadata and retained unresolved evidence.".into(),
        "Run every required proof suite against sealed canonical captures for the candidate build."
            .into(),
        "Create a digest-pinned proof manifest; do not edit the approved runtime pack in place."
            .into(),
        "Run the gate command and promote only when exact party conservation passes.".into(),
    ];
    if stable_algorithm_review_required {
        next_actions.insert(
            1,
            "Review whether a formula stage changed shape; add a versioned algorithm variant instead of altering historical behavior.".into(),
        );
    }
    for (domain, actions) in &domain_actions {
        for action in actions {
            next_actions.push(format!("[{domain}] {action}"));
        }
    }
    UpdateWorklist {
        schema_version: UPDATE_WORKLIST_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-build-audit".into(),
        baseline_build: diff.baseline_build.clone(),
        candidate_build: diff.candidate_build.clone(),
        candidate_snapshot_path: "candidate-snapshot.json".into(),
        build_diff_path: "build-diff.json".into(),
        build_identity_changed: diff.build_identity_changed,
        changed_input_ids,
        changed_domains,
        domain_actions,
        runtime_authority_changed_input_ids,
        required_proof_suites: diff.required_proof_suites.clone(),
        runtime_data_review_required,
        stable_algorithm_review_required,
        runtime_promotion_allowed: false,
        next_actions,
    }
}

fn snapshot_command(options: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let plan_path = required_path(options, "plan")?;
    let root = required_path(options, "root")?;
    let output = required_path(options, "output")?;
    let game_build = required(options, "build")?.to_owned();
    let snapshot_state = options
        .get("state")
        .map(String::as_str)
        .unwrap_or("candidate");
    if !matches!(snapshot_state, "candidate" | "reviewed-baseline") {
        return Err("--state must be candidate or reviewed-baseline".into());
    }
    let plan: AuditPlan = read_json(&plan_path)?;
    validate_plan(&plan)?;

    let mut seen = BTreeSet::new();
    let mut inputs = Vec::with_capacity(plan.inputs.len());
    let mut identity_errors = Vec::new();
    for input in plan.inputs {
        if !seen.insert(input.id.clone()) {
            return Err(format!("duplicate audit input id {}", input.id).into());
        }
        let resolved_input_path = input.path.replace("{build}", &game_build);
        let path = root.join(&resolved_input_path);
        if !path.exists() {
            if input.required {
                return Err(
                    format!("required rDPS audit input is missing: {}", path.display()).into(),
                );
            }
            continue;
        }
        if let Some(identity_manifest) = input.identity_manifest.as_deref() {
            if !path.is_dir() {
                return Err(format!(
                    "rDPS audit input {} declares identity manifest {identity_manifest}, but its path is not a directory",
                    input.id
                )
                .into());
            }
            let manifest_path = path.join(identity_manifest);
            if !manifest_path.is_file() {
                return Err(format!(
                    "rDPS audit input {} is missing identity manifest: {}",
                    input.id,
                    manifest_path.display()
                )
                .into());
            }
        }
        let artifact = artifact_snapshot(&path)?;
        if let Err(error) = validate_artifact_identity(
            &input.id,
            &plan.deployment,
            &plan.channel,
            &game_build,
            &artifact.json_metadata,
            input.identity_manifest.as_deref(),
        ) {
            identity_errors.push(error.to_string());
            continue;
        }
        inputs.push(InputSnapshot {
            id: input.id,
            path: resolved_input_path,
            role: input.role,
            identity_manifest: input.identity_manifest,
            domain: input.domain,
            change_actions: sorted_unique(input.change_actions),
            required: input.required,
            runtime_authority: input.runtime_authority,
            proof_suites: sorted_unique(input.proof_suites),
            file_count: artifact.file_count,
            byte_count: artifact.byte_count,
            sha256: artifact.sha256,
            json_metadata: artifact.json_metadata,
        });
    }
    if !identity_errors.is_empty() {
        return Err(format!(
            "rDPS snapshot rejected {} audit inputs with stale or cross-build artifact identities:\n{}",
            identity_errors.len(),
            identity_errors.join("\n")
        )
        .into());
    }
    inputs.sort_by(|left, right| left.id.cmp(&right.id));
    let snapshot = BuildSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-build-audit".into(),
        game: plan.game,
        deployment: plan.deployment,
        channel: plan.channel,
        game_build,
        promotion_state: snapshot_state.into(),
        policy: SnapshotPolicy {
            extraction_runs_outside_live_parser: plan.policy.extraction_runs_outside_live_parser,
            candidate_data_never_auto_promoted: plan.policy.candidate_data_never_auto_promoted,
            packet_replay_required_for_runtime_rules: plan
                .policy
                .packet_replay_required_for_runtime_rules,
            exact_party_conservation_required: plan.policy.exact_party_conservation_required,
            canonical_events_retained: plan.policy.canonical_events_retained,
            unresolved_events_hidden: plan.policy.unresolved_events_hidden,
        },
        inputs,
    };
    write_json(&output, &snapshot)
}

fn diff_command(options: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let baseline: BuildSnapshot = read_json(&required_path(options, "baseline")?)?;
    let candidate: BuildSnapshot = read_json(&required_path(options, "candidate")?)?;
    let output = required_path(options, "output")?;
    validate_snapshot(&baseline)?;
    validate_snapshot(&candidate)?;
    if baseline.promotion_state != "reviewed-baseline" || candidate.promotion_state != "candidate" {
        return Err("diff requires a reviewed-baseline and a candidate snapshot".into());
    }
    if baseline.game != candidate.game
        || baseline.deployment != candidate.deployment
        || baseline.channel != candidate.channel
    {
        return Err("baseline and candidate identify different game deployments".into());
    }

    let baseline_inputs: BTreeMap<_, _> = baseline
        .inputs
        .iter()
        .map(|input| (input.id.as_str(), input))
        .collect();
    let candidate_inputs: BTreeMap<_, _> = candidate
        .inputs
        .iter()
        .map(|input| (input.id.as_str(), input))
        .collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = 0;
    let mut proof_suites = BTreeSet::new();

    for (id, input) in &candidate_inputs {
        match baseline_inputs.get(id) {
            None => {
                proof_suites.extend(input.proof_suites.iter().cloned());
                added.push(input_change(None, Some(input)));
            }
            Some(old) if input_changed(old, input) => {
                proof_suites.extend(old.proof_suites.iter().cloned());
                proof_suites.extend(input.proof_suites.iter().cloned());
                changed.push(input_change(Some(old), Some(input)));
            }
            Some(_) => unchanged += 1,
        }
    }
    for (id, input) in &baseline_inputs {
        if !candidate_inputs.contains_key(id) {
            proof_suites.extend(input.proof_suites.iter().cloned());
            removed.push(input_change(Some(input), None));
        }
    }
    let build_identity_changed = baseline.game_build != candidate.game_build;
    if build_identity_changed {
        proof_suites.extend(build_change_proof_suites(&candidate));
    }
    let requires_reproof =
        build_identity_changed || !added.is_empty() || !removed.is_empty() || !changed.is_empty();
    let all_changes = added.iter().chain(&removed).chain(&changed);
    let changed_domains = all_changes
        .clone()
        .map(|change| change.domain.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut domain_actions = BTreeMap::<String, BTreeSet<String>>::new();
    for change in all_changes {
        domain_actions
            .entry(change.domain.clone())
            .or_default()
            .extend(change.change_actions.iter().cloned());
    }
    let domain_actions = domain_actions
        .into_iter()
        .map(|(domain, actions)| (domain, actions.into_iter().collect()))
        .collect();
    let diff = BuildDiff {
        schema_version: DIFF_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-build-audit".into(),
        baseline_build: baseline.game_build,
        candidate_build: candidate.game_build.clone(),
        build_identity_changed,
        added_inputs: added,
        removed_inputs: removed,
        changed_inputs: changed,
        changed_domains,
        domain_actions,
        unchanged_input_count: unchanged,
        requires_reproof,
        required_proof_suites: proof_suites.into_iter().collect(),
        runtime_promotion_allowed: false,
        candidate_snapshot: candidate,
    };
    write_json(&output, &diff)
}

fn input_changed(old: &InputSnapshot, new: &InputSnapshot) -> bool {
    old.sha256 != new.sha256
        || old.path != new.path
        || old.role != new.role
        || old.domain != new.domain
        || old.change_actions != new.change_actions
        || old.required != new.required
        || old.runtime_authority != new.runtime_authority
        || old.proof_suites != new.proof_suites
}

fn build_change_proof_suites(candidate: &BuildSnapshot) -> BTreeSet<String> {
    candidate
        .inputs
        .iter()
        .flat_map(|input| input.proof_suites.iter().cloned())
        .collect()
}

fn gate_command(options: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    let diff_path = required_path(options, "diff")?;
    let proof_path = required_path(options, "proof-manifest")?;
    let output = required_path(options, "output")?;
    let diff: BuildDiff = read_json(&diff_path)?;
    let proof: ProofManifest = read_json(&proof_path)?;
    if diff.schema_version != DIFF_SCHEMA_VERSION
        || proof.schema_version != PROOF_MANIFEST_SCHEMA_VERSION
    {
        return Err("unsupported rDPS diff or proof-manifest schema".into());
    }

    let mut blockers = Vec::new();
    if proof.game_build != diff.candidate_build {
        blockers.push("proof manifest does not match candidate build".into());
    }
    if proof.review_state != "approved" {
        blockers.push("proof manifest has not been reviewed and approved".into());
    }
    if !proof.canonical_events_retained || proof.unresolved_events_hidden {
        blockers.push("proof manifest violates canonical evidence retention policy".into());
    }
    let proof_root = proof_path.parent().unwrap_or_else(|| Path::new("."));
    let suites: BTreeMap<_, _> = proof
        .suites
        .iter()
        .map(|suite| (suite.id.as_str(), suite))
        .collect();
    if suites.len() != proof.suites.len() {
        blockers.push("proof manifest contains duplicate suite IDs".into());
    }
    let mut verified = Vec::new();
    for required_suite in &diff.required_proof_suites {
        let Some(suite) = suites.get(required_suite.as_str()) else {
            blockers.push(format!("missing required proof suite {required_suite}"));
            continue;
        };
        if suite.status != "passed"
            || !suite.exact_party_conservation
            || suite.observed_event_count == 0
        {
            blockers.push(format!(
                "proof suite {required_suite} did not pass exact replay"
            ));
            continue;
        }
        let report_path = proof_root.join(&suite.report_path);
        let report = match fs::read(&report_path) {
            Ok(report) => report,
            Err(error) => {
                blockers.push(format!(
                    "proof suite {required_suite} report cannot be read: {error}"
                ));
                continue;
            }
        };
        if sha256_hex(&report) != suite.report_sha256 {
            blockers.push(format!("proof suite {required_suite} report hash changed"));
            continue;
        }
        let report_metadata = json_metadata(&report);
        let report_builds: Vec<_> = report_metadata
            .iter()
            .filter(|(pointer, _)| {
                pointer.ends_with("/game_build")
                    || pointer.ends_with("/client_build")
                    || pointer.ends_with("/target_pack_id")
            })
            .filter_map(|(_, value)| value.as_str())
            .collect();
        if report_builds.is_empty()
            || report_builds
                .iter()
                .any(|actual| !actual.contains(&diff.candidate_build))
        {
            blockers.push(format!(
                "proof suite {required_suite} report does not identify the candidate build"
            ));
            continue;
        }
        verified.push(required_suite.clone());
    }
    let allowed = blockers.is_empty();
    let gate = PromotionGate {
        schema_version: GATE_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-build-audit".into(),
        game_build: diff.candidate_build,
        required_proof_suites: diff.required_proof_suites,
        verified_proof_suites: verified,
        canonical_events_retained: proof.canonical_events_retained,
        unresolved_events_hidden: proof.unresolved_events_hidden,
        exact_party_conservation: allowed,
        runtime_promotion_allowed: allowed,
        blockers,
    };
    write_json(&output, &gate)
}

fn validate_plan(plan: &AuditPlan) -> Result<(), Box<dyn Error>> {
    if plan.schema_version != PLAN_SCHEMA_VERSION
        || plan.game.trim().is_empty()
        || plan.deployment.trim().is_empty()
        || plan.channel.trim().is_empty()
        || plan.inputs.is_empty()
        || !plan.policy.extraction_runs_outside_live_parser
        || !plan.policy.candidate_data_never_auto_promoted
        || !plan.policy.packet_replay_required_for_runtime_rules
        || !plan.policy.exact_party_conservation_required
        || !plan.policy.canonical_events_retained
        || plan.policy.unresolved_events_hidden
    {
        return Err("rDPS audit plan is not fail-closed".into());
    }
    for input in &plan.inputs {
        if input.id.trim().is_empty()
            || input.path.trim().is_empty()
            || input.role.trim().is_empty()
            || input.domain.trim().is_empty()
            || input
                .change_actions
                .iter()
                .any(|action| action.trim().is_empty())
            || input
                .identity_manifest
                .as_deref()
                .is_some_and(|manifest| !is_safe_relative_manifest_path(manifest))
            || input.proof_suites.is_empty()
        {
            return Err("rDPS audit plan contains an incomplete input".into());
        }
    }
    Ok(())
}

fn is_safe_relative_manifest_path(value: &str) -> bool {
    !value.trim().is_empty()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn validate_snapshot(snapshot: &BuildSnapshot) -> Result<(), Box<dyn Error>> {
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION
        || snapshot.generated_by != "rlogs-bpsr-rdps-build-audit"
        || snapshot.game_build.trim().is_empty()
        || snapshot.inputs.is_empty()
        || !matches!(
            snapshot.promotion_state.as_str(),
            "candidate" | "reviewed-baseline"
        )
        || !snapshot.policy.candidate_data_never_auto_promoted
        || !snapshot.policy.packet_replay_required_for_runtime_rules
        || !snapshot.policy.exact_party_conservation_required
        || !snapshot.policy.canonical_events_retained
        || snapshot.policy.unresolved_events_hidden
    {
        return Err("rDPS build snapshot is not a valid fail-closed candidate".into());
    }
    Ok(())
}

fn validate_artifact_identity(
    input_id: &str,
    expected_deployment: &str,
    expected_channel: &str,
    expected_build: &str,
    metadata: &BTreeMap<String, Value>,
    identity_manifest: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let mut identity_errors = Vec::new();
    if let Some(manifest) = identity_manifest {
        let manifest = escape_pointer(&manifest.replace('\\', "/"));
        let current_entry_pointer = format!("/{manifest}/summary/current_build_entry_count");
        match metadata
            .get(&current_entry_pointer)
            .and_then(Value::as_u64)
        {
            Some(count) if count > 0 => {}
            Some(_) => identity_errors.push(format!(
                "{input_id} identity manifest has no current-build catalog entries at {current_entry_pointer}"
            )),
            None => identity_errors.push(format!(
                "{input_id} identity manifest lacks a numeric current-build entry count at {current_entry_pointer}"
            )),
        }
    }
    for (pointer, value) in metadata {
        if !is_authoritative_artifact_identity_pointer(pointer, identity_manifest) {
            // Nested identities describe a source, baseline, historical
            // comparison, or other retained evidence inside the artifact.
            // They remain hashed and diff-visible, but only the JSON
            // document's root identity is authoritative for snapshot routing.
            continue;
        }
        if pointer.ends_with("/deployment_id") || pointer.ends_with("/deployment") {
            let Some(actual_deployment) = value.as_str() else {
                identity_errors.push(format!(
                    "{input_id} has a non-string deployment identity at {pointer}"
                ));
                continue;
            };
            if actual_deployment != expected_deployment {
                identity_errors.push(format!(
                    "{input_id} targets deployment {actual_deployment} at {pointer}, expected {expected_deployment}"
                ));
            }
        } else if pointer.ends_with("/channel") {
            let Some(actual_channel) = value.as_str() else {
                identity_errors.push(format!("{input_id} has a non-string channel at {pointer}"));
                continue;
            };
            if actual_channel != expected_channel {
                identity_errors.push(format!(
                    "{input_id} targets channel {actual_channel} at {pointer}, expected {expected_channel}"
                ));
            }
        } else if pointer.ends_with("/game_build")
            || pointer.ends_with("/client_build")
            || pointer.ends_with("/build_id")
        {
            let Some(actual_build) = value.as_str() else {
                identity_errors.push(format!(
                    "{input_id} has a non-string build identity at {pointer}"
                ));
                continue;
            };
            if !actual_build.contains(expected_build) {
                identity_errors.push(format!(
                    "{input_id} is stale: {pointer} identifies {actual_build}, expected {expected_build}"
                ));
            }
        }
    }
    if identity_errors.is_empty() {
        Ok(())
    } else {
        Err(identity_errors.join("\n").into())
    }
}

fn is_authoritative_artifact_identity_pointer(
    pointer: &str,
    identity_manifest: Option<&str>,
) -> bool {
    // File inputs use `/game_build`. Directory inputs prefix every pointer
    // with one escaped relative-file segment, for example
    // `/report.json/game_build`. A third segment is necessarily nested data.
    let segments = pointer
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    match identity_manifest {
        Some(manifest) => {
            segments.len() == 2 && segments[0] == escape_pointer(&manifest.replace('\\', "/"))
        }
        None => matches!(segments.len(), 1 | 2),
    }
}

fn input_change(old: Option<&InputSnapshot>, new: Option<&InputSnapshot>) -> InputChange {
    let mut proof_suites = BTreeSet::new();
    for input in [old, new].into_iter().flatten() {
        proof_suites.extend(input.proof_suites.iter().cloned());
    }
    let domain = new
        .or(old)
        .expect("change must have one side")
        .domain
        .clone();
    let change_actions = [old, new]
        .into_iter()
        .flatten()
        .flat_map(|input| input.change_actions.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let old_metadata = old.map(|input| &input.json_metadata);
    let new_metadata = new.map(|input| &input.json_metadata);
    let keys: BTreeSet<_> = old_metadata
        .into_iter()
        .flat_map(|metadata| metadata.keys().cloned())
        .chain(
            new_metadata
                .into_iter()
                .flat_map(|metadata| metadata.keys().cloned()),
        )
        .collect();
    let metadata_changes = keys
        .into_iter()
        .filter_map(|key| {
            let old_value = old.and_then(|input| input.json_metadata.get(&key)).cloned();
            let new_value = new.and_then(|input| input.json_metadata.get(&key)).cloned();
            (old_value != new_value).then_some((
                key,
                MetadataChange {
                    old: old_value,
                    new: new_value,
                },
            ))
        })
        .collect();
    InputChange {
        id: new.or(old).expect("change must have one side").id.clone(),
        domain,
        change_actions,
        old_sha256: old.map(|input| input.sha256.clone()),
        new_sha256: new.map(|input| input.sha256.clone()),
        proof_suites: proof_suites.into_iter().collect(),
        metadata_changes,
    }
}

fn json_metadata(bytes: &[u8]) -> BTreeMap<String, Value> {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return BTreeMap::new();
    };
    let mut metadata = BTreeMap::new();
    collect_metadata(&value, "", 0, &mut metadata);
    metadata
}

struct ArtifactSnapshot {
    file_count: usize,
    byte_count: u64,
    sha256: String,
    json_metadata: BTreeMap<String, Value>,
}

fn artifact_snapshot(path: &Path) -> Result<ArtifactSnapshot, Box<dyn Error>> {
    if path.is_file() {
        let bytes = fs::read(path)?;
        return Ok(ArtifactSnapshot {
            file_count: 1,
            byte_count: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
            json_metadata: json_metadata(&bytes),
        });
    }
    if !path.is_dir() {
        return Err(format!("unsupported rDPS audit input: {}", path.display()).into());
    }
    let mut files = Vec::new();
    collect_files(path, path, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    if files.is_empty() {
        return Err(format!("rDPS audit directory is empty: {}", path.display()).into());
    }
    let mut hasher = Sha256::new();
    let mut byte_count = 0_u64;
    let mut metadata = BTreeMap::new();
    for (relative, file_path) in &files {
        let bytes = fs::read(file_path)?;
        byte_count += bytes.len() as u64;
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        for (pointer, value) in json_metadata(&bytes) {
            metadata.insert(
                format!(
                    "/{}/{}",
                    escape_pointer(relative),
                    pointer.trim_start_matches('/')
                ),
                value,
            );
        }
    }
    Ok(ArtifactSnapshot {
        file_count: files.len(),
        byte_count,
        sha256: digest_hex(&hasher.finalize()),
        json_metadata: metadata,
    })
}

fn collect_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<(String, PathBuf)>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, output)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            output.push((relative, path));
        }
    }
    Ok(())
}

fn collect_metadata(
    value: &Value,
    pointer: &str,
    depth: usize,
    output: &mut BTreeMap<String, Value>,
) {
    if depth > 2 {
        return;
    }
    let Value::Object(object) = value else {
        return;
    };
    for (key, child) in object {
        let child_pointer = format!("{pointer}/{}", escape_pointer(key));
        match child {
            Value::Array(values) => {
                output.insert(
                    format!("{child_pointer}/@length"),
                    Value::from(values.len()),
                );
            }
            Value::Object(_) => collect_metadata(child, &child_pointer, depth + 1, output),
            _ if is_identity_or_count_key(key) => {
                output.insert(child_pointer, child.clone());
            }
            _ => {}
        }
    }
}

fn is_identity_or_count_key(key: &str) -> bool {
    matches!(
        key,
        "schema_version"
            | "deployment"
            | "deployment_id"
            | "channel"
            | "build_id"
            | "game_build"
            | "client_build"
            | "target_pack_id"
            | "source_table_hash"
            | "table_hash"
            | "row_count"
            | "source_rows"
            | "unique_lookup_keys"
            | "ambiguous_lookup_keys"
            | "standard_attack_rules"
            | "standard_magic_attack_rules"
            | "standard_rules"
    ) || key.ends_with("_count")
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(&Sha256::digest(bytes))
}

fn digest_hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sorted_unique(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn usage() -> &'static str {
    "usage:\n  rlogs-bpsr-rdps-build-audit preflight --plan <plan.json> --root <artifact-root> --build <client-build> --output <preflight.json>\n  rlogs-bpsr-rdps-build-audit prepare --plan <plan.json> --root <artifact-root> --baseline <snapshot.json> --build <client-build> --output-dir <audit-root>\n  rlogs-bpsr-rdps-build-audit snapshot --plan <plan.json> --root <artifact-root> --build <client-build> --state <candidate|reviewed-baseline> --output <snapshot.json>\n  rlogs-bpsr-rdps-build-audit diff --baseline <snapshot.json> --candidate <snapshot.json> --output <diff.json>\n  rlogs-bpsr-rdps-build-audit gate --diff <diff.json> --proof-manifest <proofs.json> --output <gate.json>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_keeps_identity_counts_and_array_sizes() {
        let metadata = json_metadata(
            br#"{"schema_version":1,"deployment_id":"global","channel":"steam","build_id":"42","game_build":"42","rules":[{},{}],"summary":{"row_count":5},"ignored":"x"}"#,
        );
        assert_eq!(metadata.get("/schema_version"), Some(&Value::from(1)));
        assert_eq!(metadata.get("/deployment_id"), Some(&Value::from("global")));
        assert_eq!(metadata.get("/channel"), Some(&Value::from("steam")));
        assert_eq!(metadata.get("/build_id"), Some(&Value::from("42")));
        assert_eq!(metadata.get("/game_build"), Some(&Value::from("42")));
        assert_eq!(metadata.get("/rules/@length"), Some(&Value::from(2)));
        assert_eq!(metadata.get("/summary/row_count"), Some(&Value::from(5)));
        assert!(!metadata.contains_key("/ignored"));
    }

    #[test]
    fn artifact_identity_rejects_cross_deployment_or_stale_build_data() {
        let metadata = BTreeMap::from([
            ("/deployment_id".into(), Value::from("global")),
            ("/channel".into(), Value::from("steam")),
            ("/build_id".into(), Value::from("24252055")),
            ("/game_build".into(), Value::from("24252055")),
        ]);
        assert!(
            validate_artifact_identity("runtime", "global", "steam", "24252055", &metadata, None,)
                .is_ok()
        );
        assert!(
            validate_artifact_identity("runtime", "cn", "steam", "24252055", &metadata, None,)
                .is_err()
        );
        assert!(
            validate_artifact_identity(
                "runtime",
                "global",
                "standalone",
                "24252055",
                &metadata,
                None,
            )
            .is_err()
        );
        let stale_error =
            validate_artifact_identity("runtime", "global", "steam", "next", &metadata, None)
                .expect_err("both stale root build identities must be rejected")
                .to_string();
        assert!(stale_error.contains("/build_id"));
        assert!(stale_error.contains("/game_build"));
    }

    #[test]
    fn artifact_identity_ignores_nested_historical_references_but_not_directory_roots() {
        let nested_historical = BTreeMap::from([
            ("/frontier.json/game_build".into(), Value::from("24687926")),
            (
                "/frontier.json/historical_build_observation/game_build".into(),
                Value::from("24252055"),
            ),
        ]);
        assert!(
            validate_artifact_identity(
                "research",
                "global",
                "steam",
                "24687926",
                &nested_historical,
                None,
            )
            .is_ok()
        );

        let stale_directory_root =
            BTreeMap::from([("/frontier.json/game_build".into(), Value::from("24252055"))]);
        assert!(
            validate_artifact_identity(
                "research",
                "global",
                "steam",
                "24687926",
                &stale_directory_root,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn directory_identity_manifest_keeps_other_root_files_hashed_but_non_authoritative() {
        let metadata = BTreeMap::from([
            (
                "/current-build-manifest.v1.json/game_build".into(),
                Value::from("24687926"),
            ),
            (
                "/legacy-rule.json/game_build".into(),
                Value::from("24252055"),
            ),
            (
                "/current-build-manifest.v1.json/historical/game_build".into(),
                Value::from("24252055"),
            ),
            (
                "/current-build-manifest.v1.json/summary/current_build_entry_count".into(),
                Value::from(3),
            ),
        ]);
        assert!(
            validate_artifact_identity(
                "mixed-catalog",
                "global",
                "steam",
                "24687926",
                &metadata,
                Some("current-build-manifest.v1.json"),
            )
            .is_ok()
        );
        assert!(is_safe_relative_manifest_path(
            "subdirectory/current-build-manifest.v1.json"
        ));
        assert!(!is_safe_relative_manifest_path("../manifest.json"));
        assert!(!is_safe_relative_manifest_path("C:/manifest.json"));

        let empty = BTreeMap::from([
            (
                "/current-build-manifest.v1.json/game_build".into(),
                Value::from("24687926"),
            ),
            (
                "/current-build-manifest.v1.json/summary/current_build_entry_count".into(),
                Value::from(0),
            ),
        ]);
        assert!(
            validate_artifact_identity(
                "empty-current-catalog",
                "global",
                "steam",
                "24687926",
                &empty,
                Some("current-build-manifest.v1.json"),
            )
            .is_err()
        );
    }

    #[test]
    fn changed_input_routes_every_affected_proof_suite() {
        let old = InputSnapshot {
            id: "formula".into(),
            path: "old.json".into(),
            role: "runtime".into(),
            identity_manifest: None,
            domain: "formulas-scaling".into(),
            change_actions: vec!["Rebuild formula surfaces.".into()],
            required: true,
            runtime_authority: true,
            proof_suites: vec!["conservation".into()],
            file_count: 1,
            byte_count: 1,
            sha256: "old".into(),
            json_metadata: BTreeMap::from([("/rules/@length".into(), Value::from(1))]),
        };
        let mut new = old.clone();
        new.sha256 = "new".into();
        new.proof_suites.push("event-coverage".into());
        new.json_metadata
            .insert("/rules/@length".into(), Value::from(2));
        let change = input_change(Some(&old), Some(&new));
        assert_eq!(
            change.proof_suites,
            vec!["conservation".to_owned(), "event-coverage".to_owned()]
        );
        assert!(change.metadata_changes.contains_key("/rules/@length"));
    }

    #[test]
    fn new_build_replays_every_suite_even_when_input_digests_match() {
        let input = |id: &str, suites: &[&str]| InputSnapshot {
            id: id.into(),
            path: format!("{id}.json"),
            role: "proof input".into(),
            identity_manifest: None,
            domain: "shared".into(),
            change_actions: Vec::new(),
            required: true,
            runtime_authority: false,
            proof_suites: suites.iter().map(|suite| (*suite).into()).collect(),
            file_count: 1,
            byte_count: 1,
            sha256: "same-digest".into(),
            json_metadata: BTreeMap::new(),
        };
        let candidate = BuildSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generated_by: "test".into(),
            game: "blue-protocol-star-resonance".into(),
            deployment: "global".into(),
            channel: "steam".into(),
            game_build: "next-build".into(),
            promotion_state: "candidate".into(),
            policy: SnapshotPolicy {
                extraction_runs_outside_live_parser: true,
                candidate_data_never_auto_promoted: true,
                packet_replay_required_for_runtime_rules: true,
                exact_party_conservation_required: true,
                canonical_events_retained: true,
                unresolved_events_hidden: false,
            },
            inputs: vec![
                input("inventory", &["schema-diff", "conservation"]),
                input("runtime", &["formula-replay", "conservation"]),
            ],
        };

        assert_eq!(
            build_change_proof_suites(&candidate),
            BTreeSet::from([
                "conservation".to_owned(),
                "formula-replay".to_owned(),
                "schema-diff".to_owned(),
            ])
        );
    }

    #[test]
    fn prepare_build_identifiers_cannot_escape_the_audit_root() {
        assert!(validate_build_path_component("24252055").is_ok());
        assert!(validate_build_path_component("steam-24252055.1").is_ok());
        assert!(validate_build_path_component("../candidate").is_err());
        assert!(validate_build_path_component("candidate\\nested").is_err());
        assert!(validate_build_path_component("").is_err());
    }

    #[test]
    fn preflight_keeps_missing_required_and_optional_inputs_visible() {
        let plan = AuditPlan {
            schema_version: PLAN_SCHEMA_VERSION,
            game: "blue-protocol-star-resonance".into(),
            deployment: "global".into(),
            channel: "steam".into(),
            policy: PlanPolicy {
                extraction_runs_outside_live_parser: true,
                candidate_data_never_auto_promoted: true,
                packet_replay_required_for_runtime_rules: true,
                exact_party_conservation_required: true,
                canonical_events_retained: true,
                unresolved_events_hidden: false,
            },
            inputs: vec![
                PlanInput {
                    id: "required".into(),
                    path: "missing-{build}.json".into(),
                    role: "required proof".into(),
                    identity_manifest: None,
                    domain: "imagines".into(),
                    change_actions: vec!["Rebuild Imagine tiers and relationships.".into()],
                    required: true,
                    runtime_authority: false,
                    proof_suites: vec!["conservation".into()],
                },
                PlanInput {
                    id: "optional".into(),
                    path: "optional-{build}.json".into(),
                    role: "optional proof".into(),
                    identity_manifest: None,
                    domain: "localization-references".into(),
                    change_actions: vec!["Refresh localization references.".into()],
                    required: false,
                    runtime_authority: false,
                    proof_suites: vec!["formula".into()],
                },
            ],
        };
        let root = env::temp_dir().join(format!("rlogs-rdps-preflight-{}", std::process::id()));
        let preflight = build_preflight(plan, &root, "24609362".into()).unwrap();
        assert_eq!(preflight.summary.missing_required_inputs, 1);
        assert_eq!(preflight.summary.missing_optional_inputs, 1);
        assert!(!preflight.ready_for_snapshot);
        assert!(!preflight.runtime_promotion_allowed);
        assert_eq!(
            preflight.required_proof_suites_from_missing_inputs,
            vec!["conservation"]
        );
    }

    #[test]
    fn worklist_routes_runtime_and_algorithm_review_without_auto_promotion() {
        let input = |id: &str, runtime_authority: bool| InputSnapshot {
            id: id.into(),
            path: format!("{id}.json"),
            role: "proof input".into(),
            identity_manifest: None,
            domain: if id.contains("formula") {
                "formulas-scaling".into()
            } else {
                "runtime-relationships".into()
            },
            change_actions: vec![format!("Review {id}.")],
            required: true,
            runtime_authority,
            proof_suites: vec!["conservation".into()],
            file_count: 1,
            byte_count: 1,
            sha256: "old".into(),
            json_metadata: BTreeMap::new(),
        };
        let snapshot = |state: &str, build: &str, inputs: Vec<InputSnapshot>| BuildSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generated_by: "rlogs-bpsr-rdps-build-audit".into(),
            game: "blue-protocol-star-resonance".into(),
            deployment: "global".into(),
            channel: "steam".into(),
            game_build: build.into(),
            promotion_state: state.into(),
            policy: SnapshotPolicy {
                extraction_runs_outside_live_parser: true,
                candidate_data_never_auto_promoted: true,
                packet_replay_required_for_runtime_rules: true,
                exact_party_conservation_required: true,
                canonical_events_retained: true,
                unresolved_events_hidden: false,
            },
            inputs,
        };
        let baseline = snapshot(
            "reviewed-baseline",
            "old",
            vec![
                input("formula-proof-ledgers", false),
                input("rdps-formula-algorithms", true),
                input("external-state-runtime", true),
            ],
        );
        let mut candidate_inputs = baseline.inputs.clone();
        candidate_inputs[0].sha256 = "new-formula".into();
        candidate_inputs[1].sha256 = "new-algorithm".into();
        candidate_inputs[2].sha256 = "new-runtime".into();
        let candidate = snapshot("candidate", "new", candidate_inputs);
        let diff = BuildDiff {
            schema_version: DIFF_SCHEMA_VERSION,
            generated_by: "rlogs-bpsr-rdps-build-audit".into(),
            baseline_build: "old".into(),
            candidate_build: "new".into(),
            build_identity_changed: true,
            added_inputs: Vec::new(),
            removed_inputs: Vec::new(),
            changed_inputs: vec![
                input_change(Some(&baseline.inputs[0]), Some(&candidate.inputs[0])),
                input_change(Some(&baseline.inputs[1]), Some(&candidate.inputs[1])),
                input_change(Some(&baseline.inputs[2]), Some(&candidate.inputs[2])),
            ],
            changed_domains: vec!["formulas-scaling".into(), "runtime-relationships".into()],
            domain_actions: BTreeMap::new(),
            unchanged_input_count: 0,
            requires_reproof: true,
            required_proof_suites: vec!["conservation".into()],
            runtime_promotion_allowed: false,
            candidate_snapshot: candidate,
        };

        let worklist = update_worklist(&baseline, &diff);
        assert_eq!(
            worklist.runtime_authority_changed_input_ids,
            vec!["external-state-runtime", "rdps-formula-algorithms"]
        );
        assert!(worklist.runtime_data_review_required);
        assert!(worklist.stable_algorithm_review_required);
        assert!(!worklist.runtime_promotion_allowed);
    }
}
