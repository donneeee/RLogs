use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, DataGapKind, EntityRef, StatusEffectInstanceId, StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 3;
const LEGACY_SCHEMA_VERSION: u16 = 2;
const GENERATED_BY: &str = "rlogs-bpsr-rlog-gap-window-audit";

#[derive(Debug)]
enum Command {
    Generate {
        build: String,
        manifest: PathBuf,
        effect_id: i64,
        damage_relationship: DamageRelationship,
        output: PathBuf,
    },
    Verify {
        input: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditReport {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    effect_id: i64,
    #[serde(default)]
    damage_relationship: DamageRelationship,
    policy: AuditPolicy,
    inputs: AuditInputs,
    summary: AuditSummary,
    sessions: Vec<SessionAudit>,
    blockers: Vec<String>,
    content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditPolicy {
    sealed_rlogs_are_streamed_one_event_at_a_time: bool,
    every_data_gap_and_recorder_pause_is_an_exclusion_boundary: bool,
    status_lifecycles_never_cross_exclusion_or_run_boundaries: bool,
    complete_gap_bounded_lifecycle_is_not_counterfactual_formula_proof: bool,
    packet_absence_is_not_zero: bool,
    structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: bool,
    current_snapshots_are_never_backfilled_into_historical_windows: bool,
    #[serde(default)]
    damage_relationship_is_explicit: bool,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DamageRelationship {
    Source,
    #[default]
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

    fn matches(self, endpoint: EntityRef, damage: &rlogs_events::DamageEvent) -> bool {
        match self {
            Self::Source => endpoint == damage.source,
            Self::Target => endpoint == damage.target,
        }
    }

    fn endpoint_role(self) -> &'static str {
        match self {
            Self::Source => "damage_actor",
            Self::Target => "damage_target",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditInputs {
    source_manifest: FileReceipt,
    source_manifest_kind: String,
    source_rlog_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileReceipt {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AuditSummary {
    source_rlog_count: usize,
    sealed_rlog_count: usize,
    source_rlog_bytes: u64,
    canonical_event_count: u64,
    data_gap_count: u64,
    recorder_pause_count: u64,
    run_boundary_count: u64,
    rlogs_with_data_gaps: usize,
    rlogs_without_data_gaps: usize,
    selected_effect_status_event_count: u64,
    selected_effect_applied_count: u64,
    selected_effect_terminal_count: u64,
    selected_effect_complete_gap_bounded_lifecycle_count: u64,
    selected_effect_complete_windows_with_damage_count: u64,
    selected_effect_damage_events_while_active: u64,
    selected_effect_lifecycles_cut_by_data_quality_boundary: u64,
    selected_effect_lifecycles_cut_by_run_boundary: u64,
    selected_effect_open_at_end_of_log: u64,
    selected_effect_events_without_instance_id: u64,
    selected_effect_unmatched_terminal_events: u64,
    selected_effect_duplicate_applications: u64,
    candidate_rlog_count: usize,
    gap_kind_counts: BTreeMap<String, u64>,
    exact_gap_bounded_lifecycle_windows_identified: bool,
    exact_damage_projection_proven: bool,
    exact_operation_order_proven: bool,
    exact_integer_rounding_proven: bool,
    packet_conservation_proven: bool,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionAudit {
    path: String,
    bytes: u64,
    session_id: String,
    sealed_content_sha256: String,
    event_count: u64,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    data_gap_count: u64,
    recorder_pause_count: u64,
    run_boundary_count: u64,
    selected_effect_status_event_count: u64,
    selected_effect_applied_count: u64,
    selected_effect_terminal_count: u64,
    selected_effect_complete_gap_bounded_lifecycle_count: u64,
    selected_effect_complete_windows_with_damage_count: u64,
    selected_effect_damage_events_while_active: u64,
    selected_effect_lifecycles_cut_by_data_quality_boundary: u64,
    selected_effect_lifecycles_cut_by_run_boundary: u64,
    selected_effect_open_at_end_of_log: u64,
    selected_effect_events_without_instance_id: u64,
    selected_effect_unmatched_terminal_events: u64,
    selected_effect_duplicate_applications: u64,
    gap_kind_counts: BTreeMap<String, u64>,
    exclusion_boundaries: Vec<ExclusionBoundary>,
    complete_gap_bounded_windows: Vec<EffectWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExclusionBoundary {
    envelope_sequence: u64,
    observed_micros: u64,
    boundary_kind: String,
    data_gap_kind: Option<DataGapKind>,
    detail: String,
    active_selected_effect_lifecycles_cut: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EffectWindow {
    segment_index: u64,
    instance_id: i64,
    target_actor_id: u64,
    target_entity_uuid: i64,
    source_actor_id: Option<u64>,
    source_entity_uuid: Option<i64>,
    applied_envelope_sequence: u64,
    applied_observed_micros: u64,
    terminal_envelope_sequence: u64,
    terminal_observed_micros: u64,
    terminal_state: StatusState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effect_endpoint_damage_role: Option<String>,
    damage_events_while_active: u64,
    gap_bounded: bool,
    controlled_counterfactual_pair_proven: bool,
    formula_authority: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ActiveKey {
    target: EntityRef,
    instance_id: StatusEffectInstanceId,
}

#[derive(Debug, Clone)]
struct ActiveEffect {
    segment_index: u64,
    target: EntityRef,
    source: Option<EntityRef>,
    applied_envelope_sequence: u64,
    applied_observed_micros: u64,
    damage_events_while_active: u64,
}

#[derive(Debug, Default)]
struct SessionAccumulator {
    data_gap_count: u64,
    recorder_pause_count: u64,
    run_boundary_count: u64,
    selected_effect_status_event_count: u64,
    selected_effect_applied_count: u64,
    selected_effect_terminal_count: u64,
    selected_effect_lifecycles_cut_by_data_quality_boundary: u64,
    selected_effect_lifecycles_cut_by_run_boundary: u64,
    selected_effect_events_without_instance_id: u64,
    selected_effect_unmatched_terminal_events: u64,
    selected_effect_duplicate_applications: u64,
    gap_kind_counts: BTreeMap<String, u64>,
    exclusion_boundaries: Vec<ExclusionBoundary>,
    complete_gap_bounded_windows: Vec<EffectWindow>,
    active: HashMap<ActiveKey, ActiveEffect>,
    segment_index: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RLOG gap-window audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match arguments()? {
        Command::Generate {
            build,
            manifest,
            effect_id,
            damage_relationship,
            output,
        } => generate(&build, &manifest, effect_id, damage_relationship, &output),
        Command::Verify { input } => {
            let report: AuditReport = serde_json::from_reader(BufReader::new(File::open(&input)?))?;
            verify_report(&report)?;
            println!(
                "RLOG gap-window audit verified for build {} effect {}: {} complete gap-bounded windows, formula authority=false.",
                report.game_build,
                report.effect_id,
                report
                    .summary
                    .selected_effect_complete_gap_bounded_lifecycle_count
            );
            Ok(())
        }
    }
}

fn generate(
    build: &str,
    manifest: &Path,
    effect_id: i64,
    damage_relationship: DamageRelationship,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if output.exists() {
        return Err(format!("refusing to overwrite {}", output.display()).into());
    }
    let manifest_bytes = fs::read(manifest)?;
    let manifest_json: Value = serde_json::from_slice(&manifest_bytes)?;
    if declared_game_build(&manifest_json).as_deref() != Some(build) {
        return Err("source manifest build does not match requested build".into());
    }
    let (source_paths, source_manifest_kind) = source_rlogs(&manifest_json)?;
    let mut sessions = Vec::with_capacity(source_paths.len());
    for source in &source_paths {
        sessions.push(audit_rlog(source, effect_id, damage_relationship)?);
    }
    sessions.sort_by(|left, right| left.path.cmp(&right.path));

    let mut report = AuditReport {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY.to_owned(),
        game_build: build.to_owned(),
        effect_id,
        damage_relationship,
        policy: AuditPolicy {
            sealed_rlogs_are_streamed_one_event_at_a_time: true,
            every_data_gap_and_recorder_pause_is_an_exclusion_boundary: true,
            status_lifecycles_never_cross_exclusion_or_run_boundaries: true,
            complete_gap_bounded_lifecycle_is_not_counterfactual_formula_proof: true,
            packet_absence_is_not_zero: true,
            structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
            current_snapshots_are_never_backfilled_into_historical_windows: true,
            damage_relationship_is_explicit: true,
            formula_authority: false,
            runtime_authority: false,
            provider_rdps_credit_allowed: false,
        },
        inputs: AuditInputs {
            source_manifest: FileReceipt {
                path: display_path(manifest),
                bytes: manifest_bytes.len() as u64,
                sha256: hex_digest(&manifest_bytes),
            },
            source_manifest_kind,
            source_rlog_count: source_paths.len(),
        },
        summary: summarize(&sessions),
        sessions,
        blockers: vec![
            "gap-bounded lifecycle windows do not provide an otherwise-identical effect-present/effect-absent damage pair".to_owned(),
            match damage_relationship {
                DamageRelationship::Source => "effect-recipient action rate and provider-removed damage opportunity remain unproven".to_owned(),
                DamageRelationship::Target => "target defense to damage projection remains unproven".to_owned(),
            },
            "damage-stage operation order and integer rounding remain unproven".to_owned(),
            "party-wide packet conservation remains unproven".to_owned(),
        ],
        content_sha256: String::new(),
    };
    report.content_sha256 = report_digest(&report)?;
    verify_report(&report)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "Audited {} sealed RLOGs and {} events: {} data gaps, {} complete gap-bounded effect {} lifecycles, {} with {}-side damage; formula authority=false.",
        report.summary.sealed_rlog_count,
        report.summary.canonical_event_count,
        report.summary.data_gap_count,
        report
            .summary
            .selected_effect_complete_gap_bounded_lifecycle_count,
        effect_id,
        report
            .summary
            .selected_effect_complete_windows_with_damage_count,
        damage_relationship.endpoint_role(),
    );
    println!("wrote {}", output.display());
    Ok(())
}

fn declared_game_build(manifest: &Value) -> Option<String> {
    manifest
        .get("game_build")
        .and_then(json_integer_or_string)
        .or_else(|| {
            manifest
                .get("expected_game_build")
                .and_then(json_integer_or_string)
        })
        .or_else(|| {
            manifest
                .pointer("/build_scope/expected_game_build")
                .and_then(json_integer_or_string)
        })
}

fn json_integer_or_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn source_rlogs(manifest: &Value) -> Result<(Vec<PathBuf>, String), Box<dyn Error>> {
    let (candidates, manifest_kind) = source_rlog_candidates(manifest)?;
    let mut paths = BTreeSet::new();
    for input in candidates {
        paths.insert(PathBuf::from(input));
    }
    if paths.is_empty() {
        return Err("source manifest contains no source RLOGs".into());
    }
    for path in &paths {
        if !path.is_file() {
            return Err(format!("source RLOG is missing: {}", path.display()).into());
        }
    }
    Ok((paths.into_iter().collect(), manifest_kind.to_owned()))
}

fn source_rlog_candidates(manifest: &Value) -> Result<(Vec<String>, &'static str), Box<dyn Error>> {
    if let Some(rlogs) = manifest.pointer("/inputs/rlogs").and_then(Value::as_array) {
        let paths = rlogs
            .iter()
            .map(|receipt| {
                receipt
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| "input RLOG receipt has a missing or non-string path".into())
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        return Ok((paths, "input_rlog_receipts"));
    }

    if let Some(sessions) = manifest.get("sessions").and_then(Value::as_array) {
        let paths = sessions
            .iter()
            .map(|session| {
                session
                    .get("rlog")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| "session manifest has a missing or non-string rlog path".into())
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        return Ok((paths, "session_rlog_manifest"));
    }

    if let Some(runs) = manifest.get("runs").and_then(Value::as_array) {
        let mut paths = Vec::new();
        for run in runs {
            let inputs = run
                .pointer("/cohort/source_inputs")
                .and_then(Value::as_array)
                .ok_or("target-mitigation rollup run lacks cohort source inputs")?;
            for input in inputs {
                paths.push(
                    input
                        .as_str()
                        .ok_or("source RLOG path is not a string")?
                        .to_owned(),
                );
            }
        }
        return Ok((paths, "target_mitigation_rollup"));
    }

    Err("unsupported source manifest shape".into())
}

fn audit_rlog(
    path: &Path,
    effect_id: i64,
    damage_relationship: DamageRelationship,
) -> Result<SessionAudit, Box<dyn Error>> {
    let bytes = fs::metadata(path)?.len();
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let session_id = reader.header().session_id.clone();
    let mut accumulator = SessionAccumulator::default();
    while let Some(envelope) = reader.next_event()? {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::DataGap(gap) => {
                accumulator.data_gap_count = accumulator.data_gap_count.saturating_add(1);
                *accumulator
                    .gap_kind_counts
                    .entry(data_gap_kind_name(gap.kind).to_owned())
                    .or_default() += 1;
                let cut = accumulator.active.len();
                accumulator.selected_effect_lifecycles_cut_by_data_quality_boundary = accumulator
                    .selected_effect_lifecycles_cut_by_data_quality_boundary
                    .saturating_add(cut as u64);
                accumulator.active.clear();
                accumulator.segment_index = accumulator.segment_index.saturating_add(1);
                accumulator.exclusion_boundaries.push(ExclusionBoundary {
                    envelope_sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    boundary_kind: "data_gap".to_owned(),
                    data_gap_kind: Some(gap.kind),
                    detail: gap.detail.clone(),
                    active_selected_effect_lifecycles_cut: cut,
                });
            }
            TimelineEventKind::RecorderPause(pause) => {
                accumulator.recorder_pause_count =
                    accumulator.recorder_pause_count.saturating_add(1);
                let cut = accumulator.active.len();
                accumulator.selected_effect_lifecycles_cut_by_data_quality_boundary = accumulator
                    .selected_effect_lifecycles_cut_by_data_quality_boundary
                    .saturating_add(cut as u64);
                accumulator.active.clear();
                accumulator.segment_index = accumulator.segment_index.saturating_add(1);
                accumulator.exclusion_boundaries.push(ExclusionBoundary {
                    envelope_sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    boundary_kind: "recorder_pause".to_owned(),
                    data_gap_kind: None,
                    detail: format!(
                        "recorder paused from {}us through {}us",
                        pause.started_micros, pause.resumed_micros
                    ),
                    active_selected_effect_lifecycles_cut: cut,
                });
            }
            TimelineEventKind::RunBoundary { .. } => {
                accumulator.run_boundary_count = accumulator.run_boundary_count.saturating_add(1);
                let cut = accumulator.active.len();
                accumulator.selected_effect_lifecycles_cut_by_run_boundary = accumulator
                    .selected_effect_lifecycles_cut_by_run_boundary
                    .saturating_add(cut as u64);
                accumulator.active.clear();
                accumulator.segment_index = accumulator.segment_index.saturating_add(1);
            }
            TimelineEventKind::Status(status) if status.effect.0 == effect_id => {
                accumulator.selected_effect_status_event_count = accumulator
                    .selected_effect_status_event_count
                    .saturating_add(1);
                let Some(instance_id) = status.instance_id else {
                    accumulator.selected_effect_events_without_instance_id = accumulator
                        .selected_effect_events_without_instance_id
                        .saturating_add(1);
                    continue;
                };
                let key = ActiveKey {
                    target: status.target,
                    instance_id,
                };
                match status.state {
                    StatusState::Applied => {
                        accumulator.selected_effect_applied_count =
                            accumulator.selected_effect_applied_count.saturating_add(1);
                        let active = ActiveEffect {
                            segment_index: accumulator.segment_index,
                            target: status.target,
                            source: status.source,
                            applied_envelope_sequence: envelope.sequence,
                            applied_observed_micros: envelope.time.observed_micros,
                            damage_events_while_active: 0,
                        };
                        if accumulator.active.insert(key, active).is_some() {
                            accumulator.selected_effect_duplicate_applications = accumulator
                                .selected_effect_duplicate_applications
                                .saturating_add(1);
                        }
                    }
                    StatusState::Consumed | StatusState::Removed => {
                        accumulator.selected_effect_terminal_count =
                            accumulator.selected_effect_terminal_count.saturating_add(1);
                        if let Some(active) = accumulator.active.remove(&key) {
                            accumulator.complete_gap_bounded_windows.push(EffectWindow {
                                segment_index: active.segment_index,
                                instance_id: instance_id.0,
                                target_actor_id: active.target.actor_id.0,
                                target_entity_uuid: active.target.entity_uuid.0,
                                source_actor_id: active.source.map(|source| source.actor_id.0),
                                source_entity_uuid: active
                                    .source
                                    .map(|source| source.entity_uuid.0),
                                applied_envelope_sequence: active.applied_envelope_sequence,
                                applied_observed_micros: active.applied_observed_micros,
                                terminal_envelope_sequence: envelope.sequence,
                                terminal_observed_micros: envelope.time.observed_micros,
                                terminal_state: status.state,
                                effect_endpoint_damage_role: Some(
                                    damage_relationship.endpoint_role().to_owned(),
                                ),
                                damage_events_while_active: active.damage_events_while_active,
                                gap_bounded: true,
                                controlled_counterfactual_pair_proven: false,
                                formula_authority: false,
                            });
                        } else {
                            accumulator.selected_effect_unmatched_terminal_events = accumulator
                                .selected_effect_unmatched_terminal_events
                                .saturating_add(1);
                        }
                    }
                    StatusState::Refreshed | StatusState::Stacked => {}
                }
            }
            TimelineEventKind::Damage(damage) => {
                for active in accumulator.active.values_mut() {
                    if damage_relationship.matches(active.target, damage) {
                        active.damage_events_while_active =
                            active.damage_events_while_active.saturating_add(1);
                    }
                }
            }
            _ => {}
        }
    }
    let replay = reader
        .summary()
        .ok_or("sealed RLOG replay summary is missing")?;
    let selected_effect_open_at_end_of_log = accumulator.active.len() as u64;
    let complete_count = accumulator.complete_gap_bounded_windows.len() as u64;
    let complete_with_damage = accumulator
        .complete_gap_bounded_windows
        .iter()
        .filter(|window| window.damage_events_while_active > 0)
        .count() as u64;
    let damage_while_active = accumulator
        .complete_gap_bounded_windows
        .iter()
        .map(|window| window.damage_events_while_active)
        .sum();
    Ok(SessionAudit {
        path: display_path(path),
        bytes,
        session_id,
        sealed_content_sha256: replay.content_sha256.clone(),
        event_count: replay.event_count,
        first_observed_micros: replay.first_observed_micros,
        last_observed_micros: replay.last_observed_micros,
        data_gap_count: accumulator.data_gap_count,
        recorder_pause_count: accumulator.recorder_pause_count,
        run_boundary_count: accumulator.run_boundary_count,
        selected_effect_status_event_count: accumulator.selected_effect_status_event_count,
        selected_effect_applied_count: accumulator.selected_effect_applied_count,
        selected_effect_terminal_count: accumulator.selected_effect_terminal_count,
        selected_effect_complete_gap_bounded_lifecycle_count: complete_count,
        selected_effect_complete_windows_with_damage_count: complete_with_damage,
        selected_effect_damage_events_while_active: damage_while_active,
        selected_effect_lifecycles_cut_by_data_quality_boundary: accumulator
            .selected_effect_lifecycles_cut_by_data_quality_boundary,
        selected_effect_lifecycles_cut_by_run_boundary: accumulator
            .selected_effect_lifecycles_cut_by_run_boundary,
        selected_effect_open_at_end_of_log,
        selected_effect_events_without_instance_id: accumulator
            .selected_effect_events_without_instance_id,
        selected_effect_unmatched_terminal_events: accumulator
            .selected_effect_unmatched_terminal_events,
        selected_effect_duplicate_applications: accumulator.selected_effect_duplicate_applications,
        gap_kind_counts: accumulator.gap_kind_counts,
        exclusion_boundaries: accumulator.exclusion_boundaries,
        complete_gap_bounded_windows: accumulator.complete_gap_bounded_windows,
    })
}

fn summarize(sessions: &[SessionAudit]) -> AuditSummary {
    let mut summary = AuditSummary {
        source_rlog_count: sessions.len(),
        sealed_rlog_count: sessions.len(),
        source_rlog_bytes: sessions.iter().map(|session| session.bytes).sum(),
        canonical_event_count: sessions.iter().map(|session| session.event_count).sum(),
        data_gap_count: sessions.iter().map(|session| session.data_gap_count).sum(),
        recorder_pause_count: sessions
            .iter()
            .map(|session| session.recorder_pause_count)
            .sum(),
        run_boundary_count: sessions
            .iter()
            .map(|session| session.run_boundary_count)
            .sum(),
        rlogs_with_data_gaps: sessions
            .iter()
            .filter(|session| session.data_gap_count > 0)
            .count(),
        rlogs_without_data_gaps: sessions
            .iter()
            .filter(|session| session.data_gap_count == 0)
            .count(),
        selected_effect_status_event_count: sum_session(sessions, |session| {
            session.selected_effect_status_event_count
        }),
        selected_effect_applied_count: sum_session(sessions, |session| {
            session.selected_effect_applied_count
        }),
        selected_effect_terminal_count: sum_session(sessions, |session| {
            session.selected_effect_terminal_count
        }),
        selected_effect_complete_gap_bounded_lifecycle_count: sum_session(sessions, |session| {
            session.selected_effect_complete_gap_bounded_lifecycle_count
        }),
        selected_effect_complete_windows_with_damage_count: sum_session(sessions, |session| {
            session.selected_effect_complete_windows_with_damage_count
        }),
        selected_effect_damage_events_while_active: sum_session(sessions, |session| {
            session.selected_effect_damage_events_while_active
        }),
        selected_effect_lifecycles_cut_by_data_quality_boundary: sum_session(sessions, |session| {
            session.selected_effect_lifecycles_cut_by_data_quality_boundary
        }),
        selected_effect_lifecycles_cut_by_run_boundary: sum_session(sessions, |session| {
            session.selected_effect_lifecycles_cut_by_run_boundary
        }),
        selected_effect_open_at_end_of_log: sum_session(sessions, |session| {
            session.selected_effect_open_at_end_of_log
        }),
        selected_effect_events_without_instance_id: sum_session(sessions, |session| {
            session.selected_effect_events_without_instance_id
        }),
        selected_effect_unmatched_terminal_events: sum_session(sessions, |session| {
            session.selected_effect_unmatched_terminal_events
        }),
        selected_effect_duplicate_applications: sum_session(sessions, |session| {
            session.selected_effect_duplicate_applications
        }),
        candidate_rlog_count: sessions
            .iter()
            .filter(|session| session.selected_effect_complete_windows_with_damage_count > 0)
            .count(),
        exact_gap_bounded_lifecycle_windows_identified: sessions
            .iter()
            .any(|session| session.selected_effect_complete_gap_bounded_lifecycle_count > 0),
        exact_damage_projection_proven: false,
        exact_operation_order_proven: false,
        exact_integer_rounding_proven: false,
        packet_conservation_proven: false,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
        ..AuditSummary::default()
    };
    for session in sessions {
        for (kind, count) in &session.gap_kind_counts {
            *summary.gap_kind_counts.entry(kind.clone()).or_default() += count;
        }
    }
    summary
}

fn verify_report(report: &AuditReport) -> Result<(), Box<dyn Error>> {
    if !matches!(
        report.schema_version,
        LEGACY_SCHEMA_VERSION | SCHEMA_VERSION
    ) || report.generated_by != GENERATED_BY
        || report.effect_id <= 0
        || report.policy.sealed_rlogs_are_streamed_one_event_at_a_time != true
        || report
            .policy
            .every_data_gap_and_recorder_pause_is_an_exclusion_boundary
            != true
        || report
            .policy
            .structurally_unobservable_remote_player_packets_are_not_acquisition_requirements
            != true
        || report.policy.packet_absence_is_not_zero != true
        || report.policy.formula_authority
        || report.policy.runtime_authority
        || report.policy.provider_rdps_credit_allowed
    {
        return Err("RLOG gap-window audit policy is unsafe".into());
    }
    if (report.schema_version == LEGACY_SCHEMA_VERSION
        && (report.damage_relationship != DamageRelationship::Target
            || report.policy.damage_relationship_is_explicit))
        || (report.schema_version == SCHEMA_VERSION
            && !report.policy.damage_relationship_is_explicit)
    {
        return Err("RLOG gap-window audit damage relationship is inconsistent".into());
    }
    if report.content_sha256 != report_digest(report)? {
        return Err("RLOG gap-window audit content digest mismatch".into());
    }
    let expected = summarize(&report.sessions);
    if serde_json::to_value(&expected)? != serde_json::to_value(&report.summary)?
        || report.inputs.source_rlog_count != report.sessions.len()
        || report.summary.sealed_rlog_count != report.summary.source_rlog_count
        || report.summary.formula_authority
        || report.summary.runtime_authority
        || report.summary.provider_rdps_credit_allowed
        || report.sessions.iter().any(|session| {
            session.complete_gap_bounded_windows.iter().any(|window| {
                !window.gap_bounded
                    || window.controlled_counterfactual_pair_proven
                    || window.formula_authority
                    || window.terminal_envelope_sequence <= window.applied_envelope_sequence
                    || window.terminal_observed_micros < window.applied_observed_micros
            })
        })
    {
        return Err("RLOG gap-window audit totals or authority flags are inconsistent".into());
    }
    Ok(())
}

fn report_digest(report: &AuditReport) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(report)?;
    let object = value
        .as_object_mut()
        .expect("serialized report must be an object");
    object.remove("content_sha256");
    if report.schema_version == LEGACY_SCHEMA_VERSION {
        object.remove("damage_relationship");
        object
            .get_mut("policy")
            .and_then(Value::as_object_mut)
            .map(|policy| policy.remove("damage_relationship_is_explicit"));
        if let Some(sessions) = object.get_mut("sessions").and_then(Value::as_array_mut) {
            for session in sessions {
                if let Some(windows) = session
                    .get_mut("complete_gap_bounded_windows")
                    .and_then(Value::as_array_mut)
                {
                    for window in windows {
                        window
                            .as_object_mut()
                            .map(|window| window.remove("effect_endpoint_damage_role"));
                    }
                }
            }
        }
    }
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value)?)
    ))
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn data_gap_kind_name(kind: DataGapKind) -> &'static str {
    match kind {
        DataGapKind::CaptureDrop => "capture_drop",
        DataGapKind::TcpGap => "tcp_gap",
        DataGapKind::UnknownRoute => "unknown_route",
        DataGapKind::DecodeFailure => "decode_failure",
        DataGapKind::UnsupportedFragment => "unsupported_fragment",
    }
}

fn sum_session(sessions: &[SessionAudit], select: impl Fn(&SessionAudit) -> u64) -> u64 {
    sessions.iter().map(select).sum()
}

fn arguments() -> Result<Command, String> {
    let mut values = env::args_os().skip(1);
    let command = values
        .next()
        .ok_or_else(usage)?
        .to_string_lossy()
        .into_owned();
    let mut options = BTreeMap::new();
    while let Some(flag) = values.next() {
        let flag = flag.to_string_lossy().into_owned();
        let value = values
            .next()
            .ok_or_else(|| format!("{flag} requires a value"))?;
        options.insert(flag, value);
    }
    match command.as_str() {
        "generate" => Ok(Command::Generate {
            build: take_required(&mut options, "--build")?
                .to_string_lossy()
                .into_owned(),
            manifest: PathBuf::from(take_manifest(&mut options)?),
            effect_id: take_required(&mut options, "--effect-id")?
                .to_string_lossy()
                .parse()
                .map_err(|_| "--effect-id requires an integer".to_owned())?,
            damage_relationship: DamageRelationship::parse(
                &take_required(&mut options, "--damage-relationship")?.to_string_lossy(),
            )?,
            output: PathBuf::from(take_required(&mut options, "--output")?),
        }),
        "verify" => Ok(Command::Verify {
            input: PathBuf::from(take_required(&mut options, "--input")?),
        }),
        _ => Err(usage()),
    }
}

fn take_required(
    values: &mut BTreeMap<String, std::ffi::OsString>,
    flag: &str,
) -> Result<std::ffi::OsString, String> {
    values
        .remove(flag)
        .ok_or_else(|| format!("{flag} is required"))
}

fn take_manifest(
    values: &mut BTreeMap<String, std::ffi::OsString>,
) -> Result<std::ffi::OsString, String> {
    match (values.remove("--manifest"), values.remove("--rollup")) {
        (Some(manifest), None) | (None, Some(manifest)) => Ok(manifest),
        (Some(_), Some(_)) => Err("use only one of --manifest or --rollup".to_owned()),
        (None, None) => Err("--manifest is required".to_owned()),
    }
}

fn usage() -> String {
    "usage:\n  rlogs-bpsr-rlog-gap-window-audit generate --build <id> --manifest <json> --effect-id <id> --damage-relationship <source|target> --output <json>\n  rlogs-bpsr-rlog-gap-window-audit verify --input <json>".to_owned()
}

#[cfg(test)]
mod tests {
    use rlogs_events::{ActorId, DamageEvent, DamageFlags, DamagePacketDetail, EntityUuid};

    use super::*;

    fn entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(entity_uuid),
        }
    }

    #[test]
    fn damage_relationship_selects_the_effect_endpoint_side_explicitly() {
        let recipient = entity(1, 100);
        let endpoint = entity(2, 200);
        let damage = DamageEvent {
            source: recipient,
            direct_source: None,
            target: endpoint,
            ability: None,
            amount: 1,
            actual_amount: Some(1),
            hp_loss: Some(1),
            shield_loss: Some(0),
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail::default(),
        };

        assert!(DamageRelationship::Source.matches(recipient, &damage));
        assert!(!DamageRelationship::Source.matches(endpoint, &damage));
        assert!(DamageRelationship::Target.matches(endpoint, &damage));
        assert!(!DamageRelationship::Target.matches(recipient, &damage));
        assert_eq!(DamageRelationship::Source.endpoint_role(), "damage_actor");
        assert_eq!(DamageRelationship::Target.endpoint_role(), "damage_target");
    }

    #[test]
    fn accepts_audited_input_rlog_receipts_without_rewriting_paths() {
        let manifest = serde_json::json!({
            "inputs": {
                "rlogs": [
                    { "path": "runtime-data/logs/a.rlog", "bytes": 10, "sha256": "a" },
                    { "path": "runtime-data/logs/b.rlog", "bytes": 20, "sha256": "b" }
                ]
            }
        });
        let (paths, kind) = source_rlog_candidates(&manifest).unwrap();
        assert_eq!(kind, "input_rlog_receipts");
        assert_eq!(
            paths,
            [
                "runtime-data/logs/a.rlog".to_owned(),
                "runtime-data/logs/b.rlog".to_owned()
            ]
        );
    }

    #[test]
    fn verifier_rejects_formula_authority_on_gap_bounded_windows() {
        let mut report = AuditReport {
            schema_version: SCHEMA_VERSION,
            generated_by: GENERATED_BY.to_owned(),
            game_build: "24687926".to_owned(),
            effect_id: 2_110_092,
            damage_relationship: DamageRelationship::Target,
            policy: AuditPolicy {
                sealed_rlogs_are_streamed_one_event_at_a_time: true,
                every_data_gap_and_recorder_pause_is_an_exclusion_boundary: true,
                status_lifecycles_never_cross_exclusion_or_run_boundaries: true,
                complete_gap_bounded_lifecycle_is_not_counterfactual_formula_proof: true,
                packet_absence_is_not_zero: true,
                structurally_unobservable_remote_player_packets_are_not_acquisition_requirements:
                    true,
                current_snapshots_are_never_backfilled_into_historical_windows: true,
                damage_relationship_is_explicit: true,
                formula_authority: false,
                runtime_authority: false,
                provider_rdps_credit_allowed: false,
            },
            inputs: AuditInputs {
                source_manifest: FileReceipt {
                    path: "fixture.json".to_owned(),
                    bytes: 1,
                    sha256: "fixture".to_owned(),
                },
                source_manifest_kind: "session_rlog_manifest".to_owned(),
                source_rlog_count: 0,
            },
            summary: AuditSummary::default(),
            sessions: Vec::new(),
            blockers: vec!["not formula proof".to_owned()],
            content_sha256: String::new(),
        };
        report.policy.formula_authority = true;
        report.content_sha256 = report_digest(&report).unwrap();
        assert!(verify_report(&report).is_err());
    }

    #[test]
    fn current_build_session_manifest_is_accepted_without_snapshot_backfill() {
        let manifest = serde_json::json!({
            "build_scope": {
                "expected_game_build": "24687926",
                "recording_build_identity_authority": false
            },
            "sessions": [
                { "rlog": "b.rlog" },
                { "rlog": "a.rlog" }
            ]
        });

        assert_eq!(declared_game_build(&manifest).as_deref(), Some("24687926"));
        let (paths, kind) = source_rlog_candidates(&manifest).unwrap();
        assert_eq!(paths, ["b.rlog", "a.rlog"]);
        assert_eq!(kind, "session_rlog_manifest");
    }

    #[test]
    fn expected_game_build_report_identity_is_accepted() {
        let manifest = serde_json::json!({
            "expected_game_build": "24687926",
            "sessions": [{ "rlog": "a.rlog" }]
        });

        assert_eq!(declared_game_build(&manifest).as_deref(), Some("24687926"));
    }

    #[test]
    fn legacy_target_mitigation_rollup_remains_supported() {
        let manifest = serde_json::json!({
            "game_build": 24687926,
            "runs": [{ "cohort": { "source_inputs": ["a.rlog"] } }]
        });

        assert_eq!(declared_game_build(&manifest).as_deref(), Some("24687926"));
        let (paths, kind) = source_rlog_candidates(&manifest).unwrap();
        assert_eq!(paths, ["a.rlog"]);
        assert_eq!(kind, "target_mitigation_rollup");
    }
}
