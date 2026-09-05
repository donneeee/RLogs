use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    error::Error,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{CanonicalEvent, EntityRef, StatusEvent, StatusState, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 2;
const GENERATED_BY: &str = "rlogs-bpsr-sealed-rlog-candidate-manifest";
const DEFAULT_MAXIMUM_FILES: usize = 4096;

#[derive(Debug)]
enum Command {
    Generate(Arguments),
    GenerateBatch(BatchArguments),
    Verify { input: PathBuf },
}

#[derive(Debug)]
struct Arguments {
    build: String,
    effect_id: i64,
    damage_relationship: DamageRelationship,
    external_provider_only: bool,
    rlogs: Vec<PathBuf>,
    rlog_dirs: Vec<PathBuf>,
    known_artifacts: Vec<PathBuf>,
    maximum_files: usize,
    output: PathBuf,
}

#[derive(Debug)]
struct BatchArguments {
    build: String,
    effect_ids: BTreeSet<i64>,
    damage_relationship: DamageRelationship,
    external_provider_only: bool,
    rlogs: Vec<PathBuf>,
    rlog_dirs: Vec<PathBuf>,
    known_artifacts: Vec<PathBuf>,
    maximum_files: usize,
    output_directory: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DamageRelationship {
    Source,
    Target,
}

impl DamageRelationship {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "source" => Ok(Self::Source),
            "target" => Ok(Self::Target),
            _ => Err("--damage-relationship must be source or target".to_owned()),
        }
    }

    fn endpoint(self, source: EntityRef, target: EntityRef) -> EntityRef {
        match self {
            Self::Source => source,
            Self::Target => target,
        }
    }

    fn endpoint_role(self) -> &'static str {
        match self {
            Self::Source => "damage_actor",
            Self::Target => "damage_target",
        }
    }

    fn file_token(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Target => "target",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Report {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    effect_id: i64,
    damage_relationship: DamageRelationship,
    selection: Selection,
    policy: Policy,
    discovery: Discovery,
    known_artifacts: Vec<FileReceipt>,
    inputs: ManifestInputs,
    candidate_rlogs: Vec<CandidateRlog>,
    rejected_rlogs: Vec<RejectedRlog>,
    summary: Summary,
    next_stage: NextStage,
    content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Selection {
    external_provider_only: bool,
    qualifying_window: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Policy {
    recursive_directory_discovery_is_bounded: bool,
    partial_rlog_names_are_excluded: bool,
    exact_build_header_is_required: bool,
    canonical_seal_replay_is_required: bool,
    sealed_rlogs_are_streamed_one_event_at_a_time: bool,
    data_gaps_pauses_and_run_boundaries_cut_effect_windows: bool,
    known_candidates_are_deduplicated_by_sealed_content_sha256: bool,
    status_source_and_target_define_provider_relationship: bool,
    external_provider_requires_distinct_observed_entities: bool,
    ambiguous_or_missing_status_sources_are_not_external_providers: bool,
    remote_player_cast_packets_are_required: bool,
    packet_absence_is_zero: bool,
    current_snapshots_may_rewrite_historical_runs: bool,
    candidate_manifest_is_controlled_pair_proof: bool,
    formula_authority: bool,
    runtime_authority: bool,
    ui_display_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Discovery {
    explicit_rlogs: Vec<String>,
    recursive_rlog_directories: Vec<String>,
    maximum_files: usize,
    discovered_sealed_name_candidates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestInputs {
    rlogs: Vec<RlogReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileReceipt {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RlogReceipt {
    path: String,
    bytes: u64,
    sha256: String,
    sealed_content_sha256: String,
    event_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CandidateRlog {
    receipt: RlogReceipt,
    session_id: String,
    protocol_pack_digest: String,
    selected_effect_status_events: u64,
    complete_gap_bounded_lifecycles: u64,
    complete_windows_with_damage: u64,
    damage_events_while_active: u64,
    complete_self_provider_windows_with_damage: u64,
    self_provider_damage_events_while_active: u64,
    complete_external_provider_windows_with_damage: u64,
    external_provider_damage_events_while_active: u64,
    complete_unresolved_provider_windows_with_damage: u64,
    unresolved_provider_damage_events_while_active: u64,
    selected_complete_windows_with_damage: u64,
    selected_damage_events_while_active: u64,
    data_quality_boundaries: u64,
    known_sealed_content: bool,
    new_candidate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RejectedRlog {
    path: String,
    reason: String,
    observed_build: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Summary {
    discovered_sealed_name_candidates: usize,
    exact_build_sealed_rlogs: usize,
    wrong_build_rlogs: usize,
    unsealed_or_unreadable_rlogs: usize,
    exact_build_rlogs_without_selected_effect: usize,
    exact_build_effect_rlogs_without_complete_damage_window: usize,
    exact_build_effect_rlogs_without_selected_damage_window: usize,
    observed_complete_self_provider_windows_with_damage: u64,
    observed_self_provider_damage_events_while_active: u64,
    observed_complete_external_provider_windows_with_damage: u64,
    observed_external_provider_damage_events_while_active: u64,
    observed_complete_unresolved_provider_windows_with_damage: u64,
    observed_unresolved_provider_damage_events_while_active: u64,
    candidate_rlogs: usize,
    known_candidate_rlogs: usize,
    new_candidate_rlogs: usize,
    new_candidate_canonical_events: u64,
    new_candidate_damage_events_while_active: u64,
    formula_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NextStage {
    refresh_required: bool,
    source_manifest_json_pointer: String,
    required_pipeline: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ActiveWindow {
    damage_events: u64,
    provider: Option<EntityRef>,
    recipient: Option<EntityRef>,
    provider_is_ambiguous: bool,
}

#[derive(Debug, Clone, Default)]
struct EffectAudit {
    active: HashMap<(u64, i64, Option<i64>), ActiveWindow>,
    status_events: u64,
    complete: u64,
    complete_with_damage: u64,
    damage_while_active: u64,
    complete_self_with_damage: u64,
    self_damage_while_active: u64,
    complete_external_with_damage: u64,
    external_damage_while_active: u64,
    complete_unresolved_with_damage: u64,
    unresolved_damage_while_active: u64,
}

#[derive(Debug, Clone)]
struct AuditCandidate {
    receipt: RlogReceipt,
    session_id: String,
    protocol_pack_digest: String,
    selected_effect_status_events: u64,
    complete_gap_bounded_lifecycles: u64,
    complete_windows_with_damage: u64,
    damage_events_while_active: u64,
    complete_self_provider_windows_with_damage: u64,
    self_provider_damage_events_while_active: u64,
    complete_external_provider_windows_with_damage: u64,
    external_provider_damage_events_while_active: u64,
    complete_unresolved_provider_windows_with_damage: u64,
    unresolved_provider_damage_events_while_active: u64,
    data_quality_boundaries: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderRelationship {
    SelfProvided,
    ExternalProvider,
    Unresolved,
}

impl ActiveWindow {
    fn from_status(status: &StatusEvent) -> Self {
        Self {
            damage_events: 0,
            provider: status.source,
            recipient: Some(status.target),
            provider_is_ambiguous: false,
        }
    }

    fn observe_status(&mut self, status: &StatusEvent) {
        if self
            .recipient
            .is_some_and(|recipient| recipient.entity_uuid != status.target.entity_uuid)
        {
            self.provider_is_ambiguous = true;
        }
        if let Some(source) = status.source {
            if self
                .provider
                .is_some_and(|provider| provider.entity_uuid != source.entity_uuid)
            {
                self.provider_is_ambiguous = true;
            } else if self.provider.is_none() {
                self.provider = Some(source);
            }
        }
    }

    fn provider_relationship(&self) -> ProviderRelationship {
        if self.provider_is_ambiguous {
            return ProviderRelationship::Unresolved;
        }
        match (self.provider, self.recipient) {
            (Some(provider), Some(recipient)) if provider.entity_uuid == recipient.entity_uuid => {
                ProviderRelationship::SelfProvided
            }
            (Some(_), Some(_)) => ProviderRelationship::ExternalProvider,
            _ => ProviderRelationship::Unresolved,
        }
    }
}

impl EffectAudit {
    fn observe_status(&mut self, status: &StatusEvent) {
        self.status_events = self.status_events.saturating_add(1);
        let key = (
            status.target.actor_id.0,
            status.target.entity_uuid.0,
            status.instance_id.map(|value| value.0),
        );
        match status.state {
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                self.active
                    .entry(key)
                    .and_modify(|window| window.observe_status(status))
                    .or_insert_with(|| ActiveWindow::from_status(status));
            }
            StatusState::Consumed | StatusState::Removed => {
                if let Some(mut window) = self.active.remove(&key) {
                    window.observe_status(status);
                    self.complete = self.complete.saturating_add(1);
                    self.finish_window(window);
                }
            }
        }
    }

    fn observe_damage(&mut self, endpoint: EntityRef) {
        for ((actor_id, entity_uuid, _), window) in &mut self.active {
            if *actor_id == endpoint.actor_id.0 && *entity_uuid == endpoint.entity_uuid.0 {
                window.damage_events = window.damage_events.saturating_add(1);
            }
        }
    }

    fn finish_window(&mut self, window: ActiveWindow) {
        if window.damage_events == 0 {
            return;
        }
        self.complete_with_damage = self.complete_with_damage.saturating_add(1);
        self.damage_while_active = self
            .damage_while_active
            .saturating_add(window.damage_events);
        match window.provider_relationship() {
            ProviderRelationship::SelfProvided => {
                self.complete_self_with_damage = self.complete_self_with_damage.saturating_add(1);
                self.self_damage_while_active = self
                    .self_damage_while_active
                    .saturating_add(window.damage_events);
            }
            ProviderRelationship::ExternalProvider => {
                self.complete_external_with_damage =
                    self.complete_external_with_damage.saturating_add(1);
                self.external_damage_while_active = self
                    .external_damage_while_active
                    .saturating_add(window.damage_events);
            }
            ProviderRelationship::Unresolved => {
                self.complete_unresolved_with_damage =
                    self.complete_unresolved_with_damage.saturating_add(1);
                self.unresolved_damage_while_active = self
                    .unresolved_damage_while_active
                    .saturating_add(window.damage_events);
            }
        }
    }
}

fn selected_window_counts(candidate: &AuditCandidate, external_provider_only: bool) -> (u64, u64) {
    if external_provider_only {
        (
            candidate.complete_external_provider_windows_with_damage,
            candidate.external_provider_damage_events_while_active,
        )
    } else {
        (
            candidate.complete_windows_with_damage,
            candidate.damage_events_while_active,
        )
    }
}

fn candidate_qualifies(candidate: &AuditCandidate, external_provider_only: bool) -> bool {
    candidate.selected_effect_status_events > 0
        && selected_window_counts(candidate, external_provider_only).0 > 0
}

fn candidate_row(
    candidate: AuditCandidate,
    known_hashes: &BTreeSet<String>,
    external_provider_only: bool,
) -> CandidateRlog {
    let known = known_hashes.contains(&candidate.receipt.sealed_content_sha256);
    let (selected_complete_windows_with_damage, selected_damage_events_while_active) =
        selected_window_counts(&candidate, external_provider_only);
    CandidateRlog {
        receipt: candidate.receipt,
        session_id: candidate.session_id,
        protocol_pack_digest: candidate.protocol_pack_digest,
        selected_effect_status_events: candidate.selected_effect_status_events,
        complete_gap_bounded_lifecycles: candidate.complete_gap_bounded_lifecycles,
        complete_windows_with_damage: candidate.complete_windows_with_damage,
        damage_events_while_active: candidate.damage_events_while_active,
        complete_self_provider_windows_with_damage: candidate
            .complete_self_provider_windows_with_damage,
        self_provider_damage_events_while_active: candidate
            .self_provider_damage_events_while_active,
        complete_external_provider_windows_with_damage: candidate
            .complete_external_provider_windows_with_damage,
        external_provider_damage_events_while_active: candidate
            .external_provider_damage_events_while_active,
        complete_unresolved_provider_windows_with_damage: candidate
            .complete_unresolved_provider_windows_with_damage,
        unresolved_provider_damage_events_while_active: candidate
            .unresolved_provider_damage_events_while_active,
        selected_complete_windows_with_damage,
        selected_damage_events_while_active,
        data_quality_boundaries: candidate.data_quality_boundaries,
        known_sealed_content: known,
        new_candidate: !known,
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("sealed RLOG candidate manifest failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match arguments()? {
        Command::Generate(arguments) => generate(arguments),
        Command::GenerateBatch(arguments) => generate_batch(arguments),
        Command::Verify { input } => {
            let report: Report = serde_json::from_reader(BufReader::new(File::open(input)?))?;
            verify_report(&report)?;
            println!(
                "Sealed RLOG candidate manifest verified: {} candidates, {} new; formula authority=false.",
                report.summary.candidate_rlogs, report.summary.new_candidate_rlogs
            );
            Ok(())
        }
    }
}

fn generate(arguments: Arguments) -> Result<(), Box<dyn Error>> {
    if arguments.output.exists() {
        return Err(format!("refusing to overwrite {}", arguments.output.display()).into());
    }
    if arguments.build.is_empty()
        || !arguments.build.bytes().all(|value| value.is_ascii_digit())
        || arguments.effect_id <= 0
        || arguments.maximum_files == 0
    {
        return Err("build, effect ID, or maximum file count is invalid".into());
    }
    let mut paths = BTreeSet::new();
    for path in &arguments.rlogs {
        if is_sealed_name_candidate(path) {
            paths.insert(path.clone());
        }
    }
    for directory in &arguments.rlog_dirs {
        collect_rlogs(directory, &mut paths, arguments.maximum_files)?;
    }
    if paths.is_empty() {
        return Err("no sealed-name RLOG candidates were discovered".into());
    }
    if paths.len() > arguments.maximum_files {
        return Err(format!(
            "discovered {} RLOGs, exceeding --maximum-files {}",
            paths.len(),
            arguments.maximum_files
        )
        .into());
    }

    let (known_hashes, known_artifacts) = load_known_hashes(&arguments.known_artifacts)?;
    let mut candidate_rlogs = Vec::new();
    let mut rejected_rlogs = Vec::new();
    let mut exact_build_sealed_rlogs = 0_usize;
    let mut without_effect = 0_usize;
    let mut without_complete_damage_window = 0_usize;
    let mut without_selected_damage_window = 0_usize;
    let mut observed_self_windows = 0_u64;
    let mut observed_self_damage = 0_u64;
    let mut observed_external_windows = 0_u64;
    let mut observed_external_damage = 0_u64;
    let mut observed_unresolved_windows = 0_u64;
    let mut observed_unresolved_damage = 0_u64;
    for path in &paths {
        match audit_rlog(
            path,
            &arguments.build,
            arguments.effect_id,
            arguments.damage_relationship,
        ) {
            Ok(candidate) => {
                exact_build_sealed_rlogs += 1;
                observed_self_windows = observed_self_windows
                    .saturating_add(candidate.complete_self_provider_windows_with_damage);
                observed_self_damage = observed_self_damage
                    .saturating_add(candidate.self_provider_damage_events_while_active);
                observed_external_windows = observed_external_windows
                    .saturating_add(candidate.complete_external_provider_windows_with_damage);
                observed_external_damage = observed_external_damage
                    .saturating_add(candidate.external_provider_damage_events_while_active);
                observed_unresolved_windows = observed_unresolved_windows
                    .saturating_add(candidate.complete_unresolved_provider_windows_with_damage);
                observed_unresolved_damage = observed_unresolved_damage
                    .saturating_add(candidate.unresolved_provider_damage_events_while_active);
                if candidate.selected_effect_status_events == 0 {
                    without_effect += 1;
                    continue;
                }
                if candidate.complete_windows_with_damage == 0 {
                    without_complete_damage_window += 1;
                }
                if !candidate_qualifies(&candidate, arguments.external_provider_only) {
                    without_selected_damage_window += 1;
                    continue;
                }
                candidate_rlogs.push(candidate_row(
                    candidate,
                    &known_hashes,
                    arguments.external_provider_only,
                ));
            }
            Err(rejected) => rejected_rlogs.push(rejected),
        }
    }
    candidate_rlogs.sort_by(|left, right| left.receipt.path.cmp(&right.receipt.path));
    rejected_rlogs.sort_by(|left, right| left.path.cmp(&right.path));
    let new_receipts = candidate_rlogs
        .iter()
        .filter(|candidate| candidate.new_candidate)
        .map(|candidate| candidate.receipt.clone())
        .collect::<Vec<_>>();
    let wrong_build_rlogs = rejected_rlogs
        .iter()
        .filter(|row| row.reason == "wrong-build")
        .count();
    let unsealed_or_unreadable_rlogs = rejected_rlogs.len() - wrong_build_rlogs;
    let summary = Summary {
        discovered_sealed_name_candidates: paths.len(),
        exact_build_sealed_rlogs,
        wrong_build_rlogs,
        unsealed_or_unreadable_rlogs,
        exact_build_rlogs_without_selected_effect: without_effect,
        exact_build_effect_rlogs_without_complete_damage_window: without_complete_damage_window,
        exact_build_effect_rlogs_without_selected_damage_window: without_selected_damage_window,
        observed_complete_self_provider_windows_with_damage: observed_self_windows,
        observed_self_provider_damage_events_while_active: observed_self_damage,
        observed_complete_external_provider_windows_with_damage: observed_external_windows,
        observed_external_provider_damage_events_while_active: observed_external_damage,
        observed_complete_unresolved_provider_windows_with_damage: observed_unresolved_windows,
        observed_unresolved_provider_damage_events_while_active: observed_unresolved_damage,
        candidate_rlogs: candidate_rlogs.len(),
        known_candidate_rlogs: candidate_rlogs
            .iter()
            .filter(|row| !row.new_candidate)
            .count(),
        new_candidate_rlogs: new_receipts.len(),
        new_candidate_canonical_events: new_receipts.iter().map(|row| row.event_count).sum(),
        new_candidate_damage_events_while_active: candidate_rlogs
            .iter()
            .filter(|row| row.new_candidate)
            .map(|row| row.selected_damage_events_while_active)
            .sum(),
        formula_authority: false,
        provider_rdps_credit_allowed: false,
    };
    let mut report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY.to_owned(),
        game_build: arguments.build,
        effect_id: arguments.effect_id,
        damage_relationship: arguments.damage_relationship,
        selection: Selection {
            external_provider_only: arguments.external_provider_only,
            qualifying_window: if arguments.external_provider_only {
                "complete_gap_bounded_external_provider_window_with_selected_endpoint_damage"
                    .to_owned()
            } else {
                "complete_gap_bounded_window_with_selected_endpoint_damage".to_owned()
            },
        },
        policy: Policy {
            recursive_directory_discovery_is_bounded: true,
            partial_rlog_names_are_excluded: true,
            exact_build_header_is_required: true,
            canonical_seal_replay_is_required: true,
            sealed_rlogs_are_streamed_one_event_at_a_time: true,
            data_gaps_pauses_and_run_boundaries_cut_effect_windows: true,
            known_candidates_are_deduplicated_by_sealed_content_sha256: true,
            status_source_and_target_define_provider_relationship: true,
            external_provider_requires_distinct_observed_entities: true,
            ambiguous_or_missing_status_sources_are_not_external_providers: true,
            remote_player_cast_packets_are_required: false,
            packet_absence_is_zero: false,
            current_snapshots_may_rewrite_historical_runs: false,
            candidate_manifest_is_controlled_pair_proof: false,
            formula_authority: false,
            runtime_authority: false,
            ui_display_authority: false,
            provider_rdps_credit_allowed: false,
        },
        discovery: Discovery {
            explicit_rlogs: arguments.rlogs.iter().map(|path| display_path(path)).collect(),
            recursive_rlog_directories: arguments
                .rlog_dirs
                .iter()
                .map(|path| display_path(path))
                .collect(),
            maximum_files: arguments.maximum_files,
            discovered_sealed_name_candidates: paths.len(),
        },
        known_artifacts,
        inputs: ManifestInputs {
            rlogs: new_receipts,
        },
        candidate_rlogs,
        rejected_rlogs,
        summary,
        next_stage: NextStage {
            refresh_required: false,
            source_manifest_json_pointer: "/inputs/rlogs".to_owned(),
            required_pipeline: vec![
                "rlogs-bpsr-rlog-gap-window-audit generate with the same build, effect, and damage relationship".to_owned(),
                "rlogs-bpsr-rlog-transition-counterfactual-audit generate over the resulting gap-window receipt".to_owned(),
                "run the exact integer candidate evaluator only if new comparison candidates survive".to_owned(),
                "retain formula, runtime, UI, and provider-credit gates until order, rounding, stacking, and conservation are proven".to_owned(),
            ],
        },
        content_sha256: String::new(),
    };
    report.next_stage.refresh_required = report.summary.new_candidate_rlogs > 0;
    report.content_sha256 = report_digest(&report)?;
    verify_report(&report)?;
    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "Discovered {} sealed-name RLOGs: {} exact-build effect candidates, {} new; refresh_required={}; formula authority=false.",
        report.summary.discovered_sealed_name_candidates,
        report.summary.candidate_rlogs,
        report.summary.new_candidate_rlogs,
        report.next_stage.refresh_required
    );
    println!("wrote {}", arguments.output.display());
    Ok(())
}

fn generate_batch(arguments: BatchArguments) -> Result<(), Box<dyn Error>> {
    if arguments.build.is_empty()
        || !arguments.build.bytes().all(|value| value.is_ascii_digit())
        || arguments.effect_ids.is_empty()
        || arguments.effect_ids.iter().any(|effect_id| *effect_id <= 0)
        || arguments.maximum_files == 0
    {
        return Err("build, effect IDs, or maximum file count is invalid".into());
    }
    let mut paths = BTreeSet::new();
    for path in &arguments.rlogs {
        if is_sealed_name_candidate(path) {
            paths.insert(path.clone());
        }
    }
    for directory in &arguments.rlog_dirs {
        collect_rlogs(directory, &mut paths, arguments.maximum_files)?;
    }
    if paths.is_empty() {
        return Err("no sealed-name RLOG candidates were discovered".into());
    }
    if paths.len() > arguments.maximum_files {
        return Err(format!(
            "discovered {} RLOGs, exceeding --maximum-files {}",
            paths.len(),
            arguments.maximum_files
        )
        .into());
    }

    let (known_hashes, known_artifacts) = load_known_hashes(&arguments.known_artifacts)?;
    let mut candidates_by_effect = arguments
        .effect_ids
        .iter()
        .map(|effect_id| (*effect_id, Vec::new()))
        .collect::<BTreeMap<_, Vec<AuditCandidate>>>();
    let mut rejected_rlogs = Vec::new();
    for path in &paths {
        match audit_rlog_batch(
            path,
            &arguments.build,
            &arguments.effect_ids,
            arguments.damage_relationship,
        ) {
            Ok(candidates) => {
                for (effect_id, candidate) in candidates {
                    candidates_by_effect
                        .get_mut(&effect_id)
                        .expect("selected effect must have an audit bucket")
                        .push(candidate);
                }
            }
            Err(rejected) => rejected_rlogs.push(rejected),
        }
    }

    let output_paths = arguments
        .effect_ids
        .iter()
        .map(|effect_id| {
            (
                *effect_id,
                arguments.output_directory.join(format!(
                    "effect-{effect_id}.{}.candidate-manifest.json",
                    arguments.damage_relationship.file_token()
                )),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if let Some(output) = output_paths.values().find(|output| output.exists()) {
        return Err(format!("refusing to overwrite {}", output.display()).into());
    }
    fs::create_dir_all(&arguments.output_directory)?;
    let mut total_candidates = 0_usize;
    let mut total_new_candidates = 0_usize;
    for effect_id in &arguments.effect_ids {
        let output = output_paths
            .get(effect_id)
            .expect("selected effect must have an output path");
        let report = assemble_report(
            &arguments,
            *effect_id,
            candidates_by_effect.remove(effect_id).unwrap_or_default(),
            rejected_rlogs.clone(),
            &known_hashes,
            known_artifacts.clone(),
            paths.len(),
        )?;
        total_candidates = total_candidates.saturating_add(report.summary.candidate_rlogs);
        total_new_candidates =
            total_new_candidates.saturating_add(report.summary.new_candidate_rlogs);
        write_report(output, &report)?;
    }
    println!(
        "Discovered {} sealed-name RLOGs once for {} effects: {} effect/log candidates, {} new; formula authority=false.",
        paths.len(),
        arguments.effect_ids.len(),
        total_candidates,
        total_new_candidates
    );
    println!(
        "wrote manifests under {}",
        arguments.output_directory.display()
    );
    Ok(())
}

fn assemble_report(
    arguments: &BatchArguments,
    effect_id: i64,
    audited_rlogs: Vec<AuditCandidate>,
    mut rejected_rlogs: Vec<RejectedRlog>,
    known_hashes: &BTreeSet<String>,
    known_artifacts: Vec<FileReceipt>,
    discovered_rlogs: usize,
) -> Result<Report, Box<dyn Error>> {
    let exact_build_sealed_rlogs = audited_rlogs.len();
    let without_effect = audited_rlogs
        .iter()
        .filter(|candidate| candidate.selected_effect_status_events == 0)
        .count();
    let without_complete_damage_window = audited_rlogs
        .iter()
        .filter(|candidate| {
            candidate.selected_effect_status_events > 0
                && candidate.complete_windows_with_damage == 0
        })
        .count();
    let without_selected_damage_window = audited_rlogs
        .iter()
        .filter(|candidate| {
            candidate.selected_effect_status_events > 0
                && !candidate_qualifies(candidate, arguments.external_provider_only)
        })
        .count();
    let observed_self_windows = audited_rlogs
        .iter()
        .map(|candidate| candidate.complete_self_provider_windows_with_damage)
        .sum();
    let observed_self_damage = audited_rlogs
        .iter()
        .map(|candidate| candidate.self_provider_damage_events_while_active)
        .sum();
    let observed_external_windows = audited_rlogs
        .iter()
        .map(|candidate| candidate.complete_external_provider_windows_with_damage)
        .sum();
    let observed_external_damage = audited_rlogs
        .iter()
        .map(|candidate| candidate.external_provider_damage_events_while_active)
        .sum();
    let observed_unresolved_windows = audited_rlogs
        .iter()
        .map(|candidate| candidate.complete_unresolved_provider_windows_with_damage)
        .sum();
    let observed_unresolved_damage = audited_rlogs
        .iter()
        .map(|candidate| candidate.unresolved_provider_damage_events_while_active)
        .sum();
    let mut candidate_rlogs = audited_rlogs
        .into_iter()
        .filter(|candidate| candidate_qualifies(candidate, arguments.external_provider_only))
        .map(|candidate| candidate_row(candidate, known_hashes, arguments.external_provider_only))
        .collect::<Vec<_>>();
    candidate_rlogs.sort_by(|left, right| left.receipt.path.cmp(&right.receipt.path));
    rejected_rlogs.sort_by(|left, right| left.path.cmp(&right.path));
    let new_receipts = candidate_rlogs
        .iter()
        .filter(|candidate| candidate.new_candidate)
        .map(|candidate| candidate.receipt.clone())
        .collect::<Vec<_>>();
    let wrong_build_rlogs = rejected_rlogs
        .iter()
        .filter(|row| row.reason == "wrong-build")
        .count();
    let unsealed_or_unreadable_rlogs = rejected_rlogs.len() - wrong_build_rlogs;
    let summary = Summary {
        discovered_sealed_name_candidates: discovered_rlogs,
        exact_build_sealed_rlogs,
        wrong_build_rlogs,
        unsealed_or_unreadable_rlogs,
        exact_build_rlogs_without_selected_effect: without_effect,
        exact_build_effect_rlogs_without_complete_damage_window: without_complete_damage_window,
        exact_build_effect_rlogs_without_selected_damage_window: without_selected_damage_window,
        observed_complete_self_provider_windows_with_damage: observed_self_windows,
        observed_self_provider_damage_events_while_active: observed_self_damage,
        observed_complete_external_provider_windows_with_damage: observed_external_windows,
        observed_external_provider_damage_events_while_active: observed_external_damage,
        observed_complete_unresolved_provider_windows_with_damage: observed_unresolved_windows,
        observed_unresolved_provider_damage_events_while_active: observed_unresolved_damage,
        candidate_rlogs: candidate_rlogs.len(),
        known_candidate_rlogs: candidate_rlogs
            .iter()
            .filter(|row| !row.new_candidate)
            .count(),
        new_candidate_rlogs: new_receipts.len(),
        new_candidate_canonical_events: new_receipts.iter().map(|row| row.event_count).sum(),
        new_candidate_damage_events_while_active: candidate_rlogs
            .iter()
            .filter(|row| row.new_candidate)
            .map(|row| row.selected_damage_events_while_active)
            .sum(),
        formula_authority: false,
        provider_rdps_credit_allowed: false,
    };
    let mut report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY.to_owned(),
        game_build: arguments.build.clone(),
        effect_id,
        damage_relationship: arguments.damage_relationship,
        selection: Selection {
            external_provider_only: arguments.external_provider_only,
            qualifying_window: if arguments.external_provider_only {
                "complete_gap_bounded_external_provider_window_with_selected_endpoint_damage"
                    .to_owned()
            } else {
                "complete_gap_bounded_window_with_selected_endpoint_damage".to_owned()
            },
        },
        policy: Policy {
            recursive_directory_discovery_is_bounded: true,
            partial_rlog_names_are_excluded: true,
            exact_build_header_is_required: true,
            canonical_seal_replay_is_required: true,
            sealed_rlogs_are_streamed_one_event_at_a_time: true,
            data_gaps_pauses_and_run_boundaries_cut_effect_windows: true,
            known_candidates_are_deduplicated_by_sealed_content_sha256: true,
            status_source_and_target_define_provider_relationship: true,
            external_provider_requires_distinct_observed_entities: true,
            ambiguous_or_missing_status_sources_are_not_external_providers: true,
            remote_player_cast_packets_are_required: false,
            packet_absence_is_zero: false,
            current_snapshots_may_rewrite_historical_runs: false,
            candidate_manifest_is_controlled_pair_proof: false,
            formula_authority: false,
            runtime_authority: false,
            ui_display_authority: false,
            provider_rdps_credit_allowed: false,
        },
        discovery: Discovery {
            explicit_rlogs: arguments.rlogs.iter().map(|path| display_path(path)).collect(),
            recursive_rlog_directories: arguments
                .rlog_dirs
                .iter()
                .map(|path| display_path(path))
                .collect(),
            maximum_files: arguments.maximum_files,
            discovered_sealed_name_candidates: discovered_rlogs,
        },
        known_artifacts,
        inputs: ManifestInputs {
            rlogs: new_receipts,
        },
        candidate_rlogs,
        rejected_rlogs,
        summary,
        next_stage: NextStage {
            refresh_required: false,
            source_manifest_json_pointer: "/inputs/rlogs".to_owned(),
            required_pipeline: vec![
                "rlogs-bpsr-rlog-gap-window-audit generate with the same build, effect, and damage relationship".to_owned(),
                "rlogs-bpsr-rlog-transition-counterfactual-audit generate over the resulting gap-window receipt".to_owned(),
                "run the exact integer candidate evaluator only if new comparison candidates survive".to_owned(),
                "retain formula, runtime, UI, and provider-credit gates until order, rounding, stacking, and conservation are proven".to_owned(),
            ],
        },
        content_sha256: String::new(),
    };
    report.next_stage.refresh_required = report.summary.new_candidate_rlogs > 0;
    report.content_sha256 = report_digest(&report)?;
    verify_report(&report)?;
    Ok(report)
}

fn write_report(output: &Path, report: &Report) -> Result<(), Box<dyn Error>> {
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn audit_rlog(
    path: &Path,
    expected_build: &str,
    effect_id: i64,
    damage_relationship: DamageRelationship,
) -> Result<AuditCandidate, RejectedRlog> {
    let reject = |reason: String, observed_build: Option<String>| RejectedRlog {
        path: display_path(path),
        reason,
        observed_build,
    };
    let bytes = fs::metadata(path)
        .map_err(|error| reject(format!("metadata-error:{error}"), None))?
        .len();
    let file = File::open(path).map_err(|error| reject(format!("open-error:{error}"), None))?;
    let mut reader = RlogReader::new(BufReader::new(file), RlogLimits::default())
        .map_err(|error| reject(format!("header-error:{error}"), None))?;
    let observed_build = reader.header().region.client_build.clone();
    if observed_build != expected_build {
        return Err(reject("wrong-build".to_owned(), Some(observed_build)));
    }
    let session_id = reader.header().session_id.clone();
    let protocol_pack_digest = reader.header().region.protocol_pack_digest.clone();
    let mut audit = EffectAudit::default();
    let mut boundaries = 0_u64;
    loop {
        let envelope = reader.next_event().map_err(|error| {
            reject(
                format!("replay-error:{error}"),
                Some(observed_build.clone()),
            )
        })?;
        let Some(envelope) = envelope else { break };
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::DataGap(_)
            | TimelineEventKind::RecorderPause(_)
            | TimelineEventKind::RunBoundary { .. } => {
                boundaries = boundaries.saturating_add(1);
                audit.active.clear();
            }
            TimelineEventKind::Status(status) if status.effect.0 == effect_id => {
                audit.observe_status(status);
            }
            TimelineEventKind::Damage(damage) => {
                let endpoint = damage_relationship.endpoint(damage.source, damage.target);
                audit.observe_damage(endpoint);
            }
            _ => {}
        }
    }
    let replay = reader
        .summary()
        .ok_or_else(|| reject("missing-canonical-seal".to_owned(), Some(observed_build)))?;
    let sha256 = sha256_file(path).map_err(|error| {
        reject(
            format!("hash-error:{error}"),
            Some(expected_build.to_owned()),
        )
    })?;
    Ok(AuditCandidate {
        receipt: RlogReceipt {
            path: display_path(path),
            bytes,
            sha256,
            sealed_content_sha256: replay.content_sha256.clone(),
            event_count: replay.event_count,
        },
        session_id,
        protocol_pack_digest,
        selected_effect_status_events: audit.status_events,
        complete_gap_bounded_lifecycles: audit.complete,
        complete_windows_with_damage: audit.complete_with_damage,
        damage_events_while_active: audit.damage_while_active,
        complete_self_provider_windows_with_damage: audit.complete_self_with_damage,
        self_provider_damage_events_while_active: audit.self_damage_while_active,
        complete_external_provider_windows_with_damage: audit.complete_external_with_damage,
        external_provider_damage_events_while_active: audit.external_damage_while_active,
        complete_unresolved_provider_windows_with_damage: audit.complete_unresolved_with_damage,
        unresolved_provider_damage_events_while_active: audit.unresolved_damage_while_active,
        data_quality_boundaries: boundaries,
    })
}

fn audit_rlog_batch(
    path: &Path,
    expected_build: &str,
    effect_ids: &BTreeSet<i64>,
    damage_relationship: DamageRelationship,
) -> Result<BTreeMap<i64, AuditCandidate>, RejectedRlog> {
    let reject = |reason: String, observed_build: Option<String>| RejectedRlog {
        path: display_path(path),
        reason,
        observed_build,
    };
    let bytes = fs::metadata(path)
        .map_err(|error| reject(format!("metadata-error:{error}"), None))?
        .len();
    let file = File::open(path).map_err(|error| reject(format!("open-error:{error}"), None))?;
    let mut reader = RlogReader::new(BufReader::new(file), RlogLimits::default())
        .map_err(|error| reject(format!("header-error:{error}"), None))?;
    let observed_build = reader.header().region.client_build.clone();
    if observed_build != expected_build {
        return Err(reject("wrong-build".to_owned(), Some(observed_build)));
    }
    let session_id = reader.header().session_id.clone();
    let protocol_pack_digest = reader.header().region.protocol_pack_digest.clone();
    let mut audits = effect_ids
        .iter()
        .map(|effect_id| (*effect_id, EffectAudit::default()))
        .collect::<BTreeMap<_, _>>();
    let mut boundaries = 0_u64;
    loop {
        let envelope = reader.next_event().map_err(|error| {
            reject(
                format!("replay-error:{error}"),
                Some(observed_build.clone()),
            )
        })?;
        let Some(envelope) = envelope else { break };
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::DataGap(_)
            | TimelineEventKind::RecorderPause(_)
            | TimelineEventKind::RunBoundary { .. } => {
                boundaries = boundaries.saturating_add(1);
                for audit in audits.values_mut() {
                    audit.active.clear();
                }
            }
            TimelineEventKind::Status(status) => {
                let Some(audit) = audits.get_mut(&status.effect.0) else {
                    continue;
                };
                audit.observe_status(status);
            }
            TimelineEventKind::Damage(damage) => {
                let endpoint = damage_relationship.endpoint(damage.source, damage.target);
                for audit in audits.values_mut() {
                    audit.observe_damage(endpoint);
                }
            }
            _ => {}
        }
    }
    let replay = reader
        .summary()
        .ok_or_else(|| reject("missing-canonical-seal".to_owned(), Some(observed_build)))?;
    let sha256 = sha256_file(path).map_err(|error| {
        reject(
            format!("hash-error:{error}"),
            Some(expected_build.to_owned()),
        )
    })?;
    let receipt = RlogReceipt {
        path: display_path(path),
        bytes,
        sha256,
        sealed_content_sha256: replay.content_sha256.clone(),
        event_count: replay.event_count,
    };
    Ok(audits
        .into_iter()
        .map(|(effect_id, audit)| {
            (
                effect_id,
                AuditCandidate {
                    receipt: receipt.clone(),
                    session_id: session_id.clone(),
                    protocol_pack_digest: protocol_pack_digest.clone(),
                    selected_effect_status_events: audit.status_events,
                    complete_gap_bounded_lifecycles: audit.complete,
                    complete_windows_with_damage: audit.complete_with_damage,
                    damage_events_while_active: audit.damage_while_active,
                    complete_self_provider_windows_with_damage: audit.complete_self_with_damage,
                    self_provider_damage_events_while_active: audit.self_damage_while_active,
                    complete_external_provider_windows_with_damage: audit
                        .complete_external_with_damage,
                    external_provider_damage_events_while_active: audit
                        .external_damage_while_active,
                    complete_unresolved_provider_windows_with_damage: audit
                        .complete_unresolved_with_damage,
                    unresolved_provider_damage_events_while_active: audit
                        .unresolved_damage_while_active,
                    data_quality_boundaries: boundaries,
                },
            )
        })
        .collect())
}

fn collect_rlogs(
    directory: &Path,
    output: &mut BTreeSet<PathBuf>,
    maximum_files: usize,
) -> Result<(), Box<dyn Error>> {
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && is_sealed_name_candidate(&path) {
                output.insert(path);
                if output.len() > maximum_files {
                    return Err(
                        format!("RLOG discovery exceeded --maximum-files {maximum_files}").into(),
                    );
                }
            }
        }
    }
    Ok(())
}

fn is_sealed_name_candidate(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("rlog")
        && !path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.ends_with(".partial.rlog"))
}

fn load_known_hashes(
    paths: &[PathBuf],
) -> Result<(BTreeSet<String>, Vec<FileReceipt>), Box<dyn Error>> {
    let mut hashes = BTreeSet::new();
    let mut receipts = Vec::new();
    for path in paths {
        let bytes = fs::read(path)?;
        let value: Value = serde_json::from_slice(&bytes)?;
        collect_named_hashes(&value, &mut hashes);
        receipts.push(FileReceipt {
            path: display_path(path),
            bytes: bytes.len() as u64,
            sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
        });
    }
    receipts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok((hashes, receipts))
}

fn collect_named_hashes(value: &Value, output: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "sealed_content_sha256" {
                    if let Some(hash) = child.as_str() {
                        output.insert(hash.to_owned());
                    }
                }
                collect_named_hashes(child, output);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_named_hashes(child, output);
            }
        }
        _ => {}
    }
}

fn verify_report(report: &Report) -> Result<(), Box<dyn Error>> {
    let new_candidates = report
        .candidate_rlogs
        .iter()
        .filter(|row| row.new_candidate)
        .count();
    let known_candidates = report.candidate_rlogs.len() - new_candidates;
    let input_paths = report
        .inputs
        .rlogs
        .iter()
        .map(|row| &row.path)
        .collect::<BTreeSet<_>>();
    let expected_input_paths = report
        .candidate_rlogs
        .iter()
        .filter(|row| row.new_candidate)
        .map(|row| &row.receipt.path)
        .collect::<BTreeSet<_>>();
    let expected_qualifying_window = if report.selection.external_provider_only {
        "complete_gap_bounded_external_provider_window_with_selected_endpoint_damage"
    } else {
        "complete_gap_bounded_window_with_selected_endpoint_damage"
    };
    let candidate_counts_are_consistent = report.candidate_rlogs.iter().all(|row| {
        let selected_windows = if report.selection.external_provider_only {
            row.complete_external_provider_windows_with_damage
        } else {
            row.complete_windows_with_damage
        };
        let selected_damage = if report.selection.external_provider_only {
            row.external_provider_damage_events_while_active
        } else {
            row.damage_events_while_active
        };
        row.complete_self_provider_windows_with_damage
            .saturating_add(row.complete_external_provider_windows_with_damage)
            .saturating_add(row.complete_unresolved_provider_windows_with_damage)
            == row.complete_windows_with_damage
            && row
                .self_provider_damage_events_while_active
                .saturating_add(row.external_provider_damage_events_while_active)
                .saturating_add(row.unresolved_provider_damage_events_while_active)
                == row.damage_events_while_active
            && row.selected_complete_windows_with_damage == selected_windows
            && row.selected_damage_events_while_active == selected_damage
            && selected_windows > 0
            && selected_damage > 0
    });
    if report.schema_version != SCHEMA_VERSION
        || report.generated_by != GENERATED_BY
        || report.game_build.is_empty()
        || report.effect_id <= 0
        || !report.policy.recursive_directory_discovery_is_bounded
        || !report.policy.partial_rlog_names_are_excluded
        || !report.policy.exact_build_header_is_required
        || !report.policy.canonical_seal_replay_is_required
        || !report.policy.sealed_rlogs_are_streamed_one_event_at_a_time
        || !report
            .policy
            .data_gaps_pauses_and_run_boundaries_cut_effect_windows
        || !report
            .policy
            .known_candidates_are_deduplicated_by_sealed_content_sha256
        || !report
            .policy
            .status_source_and_target_define_provider_relationship
        || !report
            .policy
            .external_provider_requires_distinct_observed_entities
        || !report
            .policy
            .ambiguous_or_missing_status_sources_are_not_external_providers
        || report.policy.remote_player_cast_packets_are_required
        || report.policy.packet_absence_is_zero
        || report.policy.current_snapshots_may_rewrite_historical_runs
        || report.policy.candidate_manifest_is_controlled_pair_proof
        || report.policy.formula_authority
        || report.policy.runtime_authority
        || report.policy.ui_display_authority
        || report.policy.provider_rdps_credit_allowed
        || report.damage_relationship.endpoint_role().is_empty()
        || report.selection.qualifying_window != expected_qualifying_window
        || report.summary.discovered_sealed_name_candidates
            != report.discovery.discovered_sealed_name_candidates
        || report.summary.candidate_rlogs != report.candidate_rlogs.len()
        || report.summary.known_candidate_rlogs != known_candidates
        || report.summary.new_candidate_rlogs != new_candidates
        || report.summary.candidate_rlogs
            + report
                .summary
                .exact_build_effect_rlogs_without_selected_damage_window
            != report
                .summary
                .exact_build_sealed_rlogs
                .saturating_sub(report.summary.exact_build_rlogs_without_selected_effect)
        || report
            .summary
            .exact_build_effect_rlogs_without_complete_damage_window
            > report
                .summary
                .exact_build_effect_rlogs_without_selected_damage_window
        || (!report.selection.external_provider_only
            && report
                .summary
                .exact_build_effect_rlogs_without_complete_damage_window
                != report
                    .summary
                    .exact_build_effect_rlogs_without_selected_damage_window)
        || report
            .summary
            .observed_complete_self_provider_windows_with_damage
            < report
                .candidate_rlogs
                .iter()
                .map(|row| row.complete_self_provider_windows_with_damage)
                .sum()
        || report
            .summary
            .observed_self_provider_damage_events_while_active
            < report
                .candidate_rlogs
                .iter()
                .map(|row| row.self_provider_damage_events_while_active)
                .sum()
        || report
            .summary
            .observed_complete_external_provider_windows_with_damage
            < report
                .candidate_rlogs
                .iter()
                .map(|row| row.complete_external_provider_windows_with_damage)
                .sum()
        || report
            .summary
            .observed_external_provider_damage_events_while_active
            < report
                .candidate_rlogs
                .iter()
                .map(|row| row.external_provider_damage_events_while_active)
                .sum()
        || report
            .summary
            .observed_complete_unresolved_provider_windows_with_damage
            < report
                .candidate_rlogs
                .iter()
                .map(|row| row.complete_unresolved_provider_windows_with_damage)
                .sum()
        || report
            .summary
            .observed_unresolved_provider_damage_events_while_active
            < report
                .candidate_rlogs
                .iter()
                .map(|row| row.unresolved_provider_damage_events_while_active)
                .sum()
        || report.summary.formula_authority
        || report.summary.provider_rdps_credit_allowed
        || report.next_stage.refresh_required != (new_candidates > 0)
        || report.next_stage.source_manifest_json_pointer != "/inputs/rlogs"
        || input_paths != expected_input_paths
        || report.candidate_rlogs.iter().any(|row| {
            row.new_candidate == row.known_sealed_content || row.selected_effect_status_events == 0
        })
        || !candidate_counts_are_consistent
        || report.content_sha256 != report_digest(report)?
    {
        return Err("sealed RLOG candidate manifest is unsafe or inconsistent".into());
    }
    Ok(())
}

fn report_digest(report: &Report) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(report)?;
    value
        .as_object_mut()
        .expect("serialized report must be an object")
        .remove("content_sha256");
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value)?)
    ))
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn arguments() -> Result<Command, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let command = take_positional(&mut values)?;
    if command == "verify" {
        let input = take_value(&mut values, "--input")?;
        if !values.is_empty() {
            return Err(usage());
        }
        return Ok(Command::Verify {
            input: PathBuf::from(input),
        });
    }
    if command != "generate" && command != "generate-batch" {
        return Err(usage());
    }
    let build = take_value(&mut values, "--build")?
        .into_string()
        .map_err(|_| usage())?;
    let damage_relationship = DamageRelationship::parse(
        &take_value(&mut values, "--damage-relationship")?.to_string_lossy(),
    )?;
    let external_provider_only = take_flag(&mut values, "--external-provider-only")?;
    let maximum_files = take_optional_value(&mut values, "--maximum-files")?
        .map(|value| value.to_string_lossy().parse().map_err(|_| usage()))
        .transpose()?
        .unwrap_or(DEFAULT_MAXIMUM_FILES);
    let rlogs = take_repeatable(&mut values, "--rlog")
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let rlog_dirs = take_repeatable(&mut values, "--rlog-dir")
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let known_artifacts = take_repeatable(&mut values, "--known-artifact")
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if rlogs.is_empty() && rlog_dirs.is_empty() {
        return Err(usage());
    }
    if command == "generate-batch" {
        let effect_ids = take_repeatable(&mut values, "--effect-id")
            .into_iter()
            .map(|value| value.to_string_lossy().parse().map_err(|_| usage()))
            .collect::<Result<BTreeSet<i64>, _>>()?;
        let output_directory = PathBuf::from(take_value(&mut values, "--output-directory")?);
        if !values.is_empty() || effect_ids.is_empty() {
            return Err(usage());
        }
        return Ok(Command::GenerateBatch(BatchArguments {
            build,
            effect_ids,
            damage_relationship,
            external_provider_only,
            rlogs,
            rlog_dirs,
            known_artifacts,
            maximum_files,
            output_directory,
        }));
    }
    let effect_id = take_value(&mut values, "--effect-id")?
        .to_string_lossy()
        .parse()
        .map_err(|_| usage())?;
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(Command::Generate(Arguments {
        build,
        effect_id,
        damage_relationship,
        external_provider_only,
        rlogs,
        rlog_dirs,
        known_artifacts,
        maximum_files,
        output,
    }))
}

fn take_positional(values: &mut Vec<OsString>) -> Result<String, String> {
    if values.is_empty() {
        return Err(usage());
    }
    values.remove(0).into_string().map_err(|_| usage())
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag)?.ok_or_else(usage)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    values.remove(position);
    if position >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    Ok(Some(values.remove(position)))
}

fn take_repeatable(values: &mut Vec<OsString>, flag: &str) -> Vec<OsString> {
    let mut output = Vec::new();
    while let Some(position) = values.iter().position(|value| value == flag) {
        values.remove(position);
        if position < values.len() {
            output.push(values.remove(position));
        }
    }
    output
}

fn take_flag(values: &mut Vec<OsString>, flag: &str) -> Result<bool, String> {
    let positions = values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (value == flag).then_some(index))
        .collect::<Vec<_>>();
    if positions.len() > 1 {
        return Err(format!("{flag} may be specified only once"));
    }
    if let Some(position) = positions.first().copied() {
        values.remove(position);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn usage() -> String {
    "usage:\n  rlogs-bpsr-sealed-rlog-candidate-manifest generate --build <id> --effect-id <id> --damage-relationship <source|target> [--external-provider-only] (--rlog <sealed.rlog> | --rlog-dir <directory>)... [--known-artifact <json> ...] [--maximum-files <count>] --output <json>\n  rlogs-bpsr-sealed-rlog-candidate-manifest generate-batch --build <id> --effect-id <id> [--effect-id <id> ...] --damage-relationship <source|target> [--external-provider-only] (--rlog <sealed.rlog> | --rlog-dir <directory>)... [--known-artifact <json> ...] [--maximum-files <count>] --output-directory <directory>\n  rlogs-bpsr-sealed-rlog-candidate-manifest verify --input <json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_name_filter_rejects_partials() {
        assert!(is_sealed_name_candidate(Path::new("run.rlog")));
        assert!(!is_sealed_name_candidate(Path::new("run.partial.rlog")));
        assert!(!is_sealed_name_candidate(Path::new("run.jsonl")));
    }

    #[test]
    fn known_hash_discovery_is_key_scoped() {
        let value = serde_json::json!({
            "sessions": [{"sealed_content_sha256": "sha256:known"}],
            "unrelated": "sha256:not-known"
        });
        let mut hashes = BTreeSet::new();
        collect_named_hashes(&value, &mut hashes);
        assert_eq!(hashes, BTreeSet::from(["sha256:known".to_owned()]));
    }

    #[test]
    fn relationship_is_allegiance_neutral() {
        let source = EntityRef {
            actor_id: rlogs_events::ActorId(1),
            entity_uuid: rlogs_events::EntityUuid(10),
        };
        let target = EntityRef {
            actor_id: rlogs_events::ActorId(2),
            entity_uuid: rlogs_events::EntityUuid(20),
        };
        assert_eq!(DamageRelationship::Source.endpoint(source, target), source);
        assert_eq!(DamageRelationship::Target.endpoint(source, target), target);
        assert_eq!(DamageRelationship::Source.file_token(), "source");
        assert_eq!(DamageRelationship::Target.file_token(), "target");
    }

    #[test]
    fn batch_report_retains_legacy_per_effect_contract() {
        let arguments = BatchArguments {
            build: "24687926".to_owned(),
            effect_ids: BTreeSet::from([3_003_012, 3_003_014]),
            damage_relationship: DamageRelationship::Source,
            external_provider_only: false,
            rlogs: vec![PathBuf::from("fixture.rlog")],
            rlog_dirs: Vec::new(),
            known_artifacts: Vec::new(),
            maximum_files: 10,
            output_directory: PathBuf::from("unused"),
        };
        let receipt = RlogReceipt {
            path: "fixture.rlog".to_owned(),
            bytes: 10,
            sha256: "sha256:file".to_owned(),
            sealed_content_sha256: "sha256:sealed".to_owned(),
            event_count: 25,
        };
        let report = assemble_report(
            &arguments,
            3_003_012,
            vec![AuditCandidate {
                receipt,
                session_id: "session".to_owned(),
                protocol_pack_digest: "sha256:pack".to_owned(),
                selected_effect_status_events: 2,
                complete_gap_bounded_lifecycles: 1,
                complete_windows_with_damage: 1,
                damage_events_while_active: 7,
                complete_self_provider_windows_with_damage: 1,
                self_provider_damage_events_while_active: 7,
                complete_external_provider_windows_with_damage: 0,
                external_provider_damage_events_while_active: 0,
                complete_unresolved_provider_windows_with_damage: 0,
                unresolved_provider_damage_events_while_active: 0,
                data_quality_boundaries: 1,
            }],
            Vec::new(),
            &BTreeSet::new(),
            Vec::new(),
            1,
        )
        .expect("batch report should satisfy the legacy verifier");

        assert_eq!(report.effect_id, 3_003_012);
        assert_eq!(report.summary.candidate_rlogs, 1);
        assert_eq!(report.summary.new_candidate_rlogs, 1);
        assert_eq!(report.summary.new_candidate_canonical_events, 25);
        assert_eq!(report.summary.new_candidate_damage_events_while_active, 7);
        assert!(report.next_stage.refresh_required);
        verify_report(&report).expect("batch report must remain independently verifiable");
    }

    fn entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: rlogs_events::ActorId(actor_id),
            entity_uuid: rlogs_events::EntityUuid(entity_uuid),
        }
    }

    fn audited_candidate(self_windows: u64, external_windows: u64) -> AuditCandidate {
        AuditCandidate {
            receipt: RlogReceipt {
                path: "fixture.rlog".to_owned(),
                bytes: 10,
                sha256: "sha256:file".to_owned(),
                sealed_content_sha256: "sha256:sealed".to_owned(),
                event_count: 25,
            },
            session_id: "session".to_owned(),
            protocol_pack_digest: "sha256:pack".to_owned(),
            selected_effect_status_events: 2,
            complete_gap_bounded_lifecycles: self_windows.saturating_add(external_windows),
            complete_windows_with_damage: self_windows.saturating_add(external_windows),
            damage_events_while_active: self_windows
                .saturating_mul(7)
                .saturating_add(external_windows.saturating_mul(11)),
            complete_self_provider_windows_with_damage: self_windows,
            self_provider_damage_events_while_active: self_windows.saturating_mul(7),
            complete_external_provider_windows_with_damage: external_windows,
            external_provider_damage_events_while_active: external_windows.saturating_mul(11),
            complete_unresolved_provider_windows_with_damage: 0,
            unresolved_provider_damage_events_while_active: 0,
            data_quality_boundaries: 0,
        }
    }

    #[test]
    fn provider_relationship_requires_distinct_observed_entities() {
        let recipient = entity(1, 10);
        assert_eq!(
            ActiveWindow {
                provider: Some(recipient),
                recipient: Some(recipient),
                ..ActiveWindow::default()
            }
            .provider_relationship(),
            ProviderRelationship::SelfProvided
        );
        assert_eq!(
            ActiveWindow {
                provider: Some(entity(99, 10)),
                recipient: Some(recipient),
                ..ActiveWindow::default()
            }
            .provider_relationship(),
            ProviderRelationship::SelfProvided
        );
        assert_eq!(
            ActiveWindow {
                provider: Some(entity(2, 20)),
                recipient: Some(recipient),
                ..ActiveWindow::default()
            }
            .provider_relationship(),
            ProviderRelationship::ExternalProvider
        );
        assert_eq!(
            ActiveWindow {
                provider: None,
                recipient: Some(recipient),
                ..ActiveWindow::default()
            }
            .provider_relationship(),
            ProviderRelationship::Unresolved
        );
    }

    #[test]
    fn external_provider_selection_excludes_self_only_windows() {
        let self_only = audited_candidate(1, 0);
        assert!(candidate_qualifies(&self_only, false));
        assert!(!candidate_qualifies(&self_only, true));

        let external = audited_candidate(0, 1);
        assert!(candidate_qualifies(&external, false));
        assert!(candidate_qualifies(&external, true));
        assert_eq!(selected_window_counts(&external, true), (1, 11));
    }

    #[test]
    fn ambiguous_provider_observations_fail_closed() {
        let recipient = entity(1, 10);
        let mut window = ActiveWindow {
            damage_events: 5,
            provider: Some(entity(2, 20)),
            recipient: Some(recipient),
            provider_is_ambiguous: false,
        };
        window.provider_is_ambiguous = true;
        let mut audit = EffectAudit::default();
        audit.finish_window(window);
        assert_eq!(audit.complete_unresolved_with_damage, 1);
        assert_eq!(audit.unresolved_damage_while_active, 5);
        assert_eq!(audit.complete_external_with_damage, 0);
    }
}
