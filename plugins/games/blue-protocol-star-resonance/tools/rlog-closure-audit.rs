use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, DungeonEventKind, EncounterState, EventProvenance, EvidenceConfidence,
    EvidenceSource, RunState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 3;
const GENERATED_BY: &str = "rlogs-bpsr-rlog-closure-audit";

#[derive(Debug)]
struct Arguments {
    expected_build: String,
    expected_protocol_pack_digest: Option<String>,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_by: &'static str,
    expected_game_build: String,
    expected_protocol_pack_digest: Option<String>,
    policy: Policy,
    summary: Summary,
    inputs: Vec<InputReport>,
    content_sha256: String,
}

#[derive(Debug, Serialize)]
struct Policy {
    sealed_rlogs_are_streamed_one_event_at_a_time: bool,
    exact_build_is_required: bool,
    exact_protocol_pack_digest_is_required_when_supplied: bool,
    canonical_data_gaps_fail_closed_scope: bool,
    recorder_pauses_fail_closed_scope: bool,
    authoritative_completed_run_boundary_is_required: bool,
    authoritative_completed_dungeon_boundary_may_close_scope: bool,
    authoritative_cleared_encounter_boundary_may_close_scope: bool,
    gap_relevance_follows_observed_scope_connections: bool,
    canonical_integrity_seal_is_required: bool,
    protocol_journal_identity_is_inferred: bool,
    formula_authority: bool,
    runtime_promotion_allowed: bool,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    input_count: usize,
    exact_build_input_count: usize,
    exact_protocol_pack_input_count: usize,
    total_events: u64,
    total_damage_events: u64,
    total_observed_damage: String,
    total_data_gaps: u64,
    total_recorder_pauses: u64,
    closed_scope_candidate_count: usize,
    closed_scope_candidates: Vec<String>,
    encounter_scope_count: usize,
    encounter_scope_candidate_count: usize,
    encounter_scope_candidates: Vec<String>,
    runtime_promotion_allowed: bool,
}

#[derive(Debug, Serialize)]
struct InputReport {
    path: String,
    bytes: u64,
    session_id: String,
    game_build: String,
    protocol_pack_digest: String,
    exact_build: bool,
    exact_protocol_pack_digest: bool,
    integrity_seal_validated: bool,
    sealed_content_sha256: String,
    event_count: u64,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    damage_event_count: u64,
    observed_damage: String,
    data_gap_count: u64,
    recorder_pause_count: u64,
    dungeon_entered_count: u64,
    dungeon_started_count: u64,
    dungeon_completed_count: u64,
    dungeon_failed_count: u64,
    dungeon_exited_count: u64,
    run_boundary_completed_count: u64,
    closed_scope_candidate: bool,
    blockers: Vec<&'static str>,
    encounter_scopes: Vec<EncounterScopeReport>,
}

#[derive(Debug, Serialize)]
struct EncounterScopeReport {
    scope_id: String,
    scope_kind: &'static str,
    ordinal: u64,
    encounter_id: Option<String>,
    start_sequence: u64,
    end_sequence: Option<u64>,
    start_observed_micros: u64,
    end_observed_micros: Option<u64>,
    start_exact_wire: bool,
    end_exact_wire: bool,
    terminal_state: &'static str,
    relevant_connection_ids: Vec<u64>,
    damage_event_count: u64,
    observed_damage: String,
    relevant_data_gap_count: u64,
    recorder_pause_count: u64,
    closed_scope_candidate: bool,
    blockers: Vec<&'static str>,
}

#[derive(Debug)]
struct ActiveEncounterScope {
    scope_kind: &'static str,
    ordinal: u64,
    encounter_id: Option<String>,
    start_sequence: u64,
    start_observed_micros: u64,
    start_exact_wire: bool,
    relevant_connection_ids: BTreeSet<u64>,
    global_data_gap_count: u64,
    data_gaps_by_connection: BTreeMap<u64, u64>,
    recorder_pause_count: u64,
    damage_event_count: u64,
    observed_damage: i128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RLOG closure audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = arguments()?;
    if arguments.output.exists() {
        return Err(format!("refusing to overwrite {}", arguments.output.display()).into());
    }

    let mut inputs = arguments
        .rlogs
        .iter()
        .map(|path| scan_rlog(path, &arguments))
        .collect::<Result<Vec<_>, _>>()?;
    inputs.sort_by(|left, right| left.path.cmp(&right.path));
    let summary = summarize(&inputs);
    let mut report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY,
        expected_game_build: arguments.expected_build,
        expected_protocol_pack_digest: arguments.expected_protocol_pack_digest,
        policy: Policy {
            sealed_rlogs_are_streamed_one_event_at_a_time: true,
            exact_build_is_required: true,
            exact_protocol_pack_digest_is_required_when_supplied: true,
            canonical_data_gaps_fail_closed_scope: true,
            recorder_pauses_fail_closed_scope: true,
            authoritative_completed_run_boundary_is_required: true,
            authoritative_completed_dungeon_boundary_may_close_scope: true,
            authoritative_cleared_encounter_boundary_may_close_scope: true,
            gap_relevance_follows_observed_scope_connections: true,
            canonical_integrity_seal_is_required: true,
            protocol_journal_identity_is_inferred: false,
            formula_authority: false,
            runtime_promotion_allowed: false,
        },
        summary,
        inputs,
        content_sha256: String::new(),
    };
    report.content_sha256 = report_sha256(&report)?;
    write_json_new(&arguments.output, &report)?;
    println!(
        "RLOG closure audit: {} inputs, {} closed-scope candidates; runtime promotion remains disabled.",
        report.summary.input_count, report.summary.closed_scope_candidate_count
    );
    Ok(())
}

fn scan_rlog(path: &Path, arguments: &Arguments) -> Result<InputReport, Box<dyn Error>> {
    let bytes = fs::metadata(path)?.len();
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let session_id = reader.header().session_id.clone();
    let game_build = reader.header().region.client_build.clone();
    let protocol_pack_digest = reader.header().region.protocol_pack_digest.clone();
    let exact_build = game_build == arguments.expected_build;
    let exact_protocol_pack_digest = arguments
        .expected_protocol_pack_digest
        .as_ref()
        .is_none_or(|expected| protocol_pack_digest == *expected);
    let mut first_observed_micros = None;
    let mut last_observed_micros = None;
    let mut damage_event_count = 0_u64;
    let mut observed_damage = 0_i128;
    let mut data_gap_count = 0_u64;
    let mut recorder_pause_count = 0_u64;
    let mut dungeon_entered_count = 0_u64;
    let mut dungeon_started_count = 0_u64;
    let mut dungeon_completed_count = 0_u64;
    let mut dungeon_failed_count = 0_u64;
    let mut dungeon_exited_count = 0_u64;
    let mut run_boundary_completed_count = 0_u64;
    let mut active_encounter_scopes = Vec::<ActiveEncounterScope>::new();
    let mut encounter_scopes = Vec::<EncounterScopeReport>::new();
    let mut encounter_ordinals = BTreeMap::<&'static str, u64>::new();

    while let Some(envelope) = reader.next_event()? {
        first_observed_micros.get_or_insert(envelope.time.observed_micros);
        last_observed_micros = Some(envelope.time.observed_micros);
        let is_data_gap = matches!(
            &envelope.event,
            CanonicalEvent::Timeline(timeline)
                if matches!(&timeline.kind, TimelineEventKind::DataGap(_))
        );
        if !is_data_gap && let Some(connection_id) = wire_connection_id(&envelope.provenance) {
            for scope in &mut active_encounter_scopes {
                scope.relevant_connection_ids.insert(connection_id);
            }
        }
        match &envelope.event {
            CanonicalEvent::Dungeon(dungeon) => match dungeon.kind {
                DungeonEventKind::Entered => dungeon_entered_count += 1,
                DungeonEventKind::Started => {
                    dungeon_started_count += 1;
                    close_superseded_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_scopes,
                        "dungeon_run",
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_build,
                        exact_protocol_pack_digest,
                        &session_id,
                    );
                    start_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_ordinals,
                        "dungeon_run",
                        dungeon.instance_id.clone(),
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_wire(&envelope.provenance),
                        wire_connection_id(&envelope.provenance),
                    );
                }
                DungeonEventKind::Completed => {
                    dungeon_completed_count += 1;
                    close_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_scopes,
                        "dungeon_run",
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_wire(&envelope.provenance),
                        "cleared",
                        exact_build,
                        exact_protocol_pack_digest,
                        &session_id,
                    );
                }
                DungeonEventKind::Failed => {
                    dungeon_failed_count += 1;
                    close_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_scopes,
                        "dungeon_run",
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_wire(&envelope.provenance),
                        "failed",
                        exact_build,
                        exact_protocol_pack_digest,
                        &session_id,
                    );
                }
                DungeonEventKind::Ended => close_scope(
                    &mut active_encounter_scopes,
                    &mut encounter_scopes,
                    "dungeon_run",
                    envelope.sequence,
                    envelope.time.observed_micros,
                    exact_wire(&envelope.provenance),
                    "ended",
                    exact_build,
                    exact_protocol_pack_digest,
                    &session_id,
                ),
                DungeonEventKind::Exited => {
                    dungeon_exited_count += 1;
                    close_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_scopes,
                        "dungeon_run",
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_wire(&envelope.provenance),
                        "exited",
                        exact_build,
                        exact_protocol_pack_digest,
                        &session_id,
                    );
                }
                DungeonEventKind::BossEngaged => {
                    close_superseded_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_scopes,
                        "dungeon_boss",
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_build,
                        exact_protocol_pack_digest,
                        &session_id,
                    );
                    start_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_ordinals,
                        "dungeon_boss",
                        dungeon.instance_id.clone(),
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_wire(&envelope.provenance),
                        wire_connection_id(&envelope.provenance),
                    );
                }
                DungeonEventKind::BossDefeated => close_scope(
                    &mut active_encounter_scopes,
                    &mut encounter_scopes,
                    "dungeon_boss",
                    envelope.sequence,
                    envelope.time.observed_micros,
                    exact_wire(&envelope.provenance),
                    "cleared",
                    exact_build,
                    exact_protocol_pack_digest,
                    &session_id,
                ),
                _ => {}
            },
            CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Damage(damage) => {
                    damage_event_count += 1;
                    observed_damage = observed_damage.saturating_add(i128::from(damage.amount));
                    for scope in &mut active_encounter_scopes {
                        scope.damage_event_count = scope.damage_event_count.saturating_add(1);
                        scope.observed_damage = scope
                            .observed_damage
                            .saturating_add(i128::from(damage.amount));
                    }
                }
                TimelineEventKind::DataGap(gap) => {
                    data_gap_count += 1;
                    for scope in &mut active_encounter_scopes {
                        if let Some(connection_id) = gap.connection_id {
                            let count = scope
                                .data_gaps_by_connection
                                .entry(connection_id)
                                .or_default();
                            *count = count.saturating_add(1);
                        } else {
                            scope.global_data_gap_count =
                                scope.global_data_gap_count.saturating_add(1);
                        }
                    }
                }
                TimelineEventKind::RecorderPause(_) => {
                    recorder_pause_count += 1;
                    for scope in &mut active_encounter_scopes {
                        scope.recorder_pause_count = scope.recorder_pause_count.saturating_add(1);
                    }
                }
                TimelineEventKind::RunBoundary {
                    state: RunState::Completed,
                    ..
                } => run_boundary_completed_count += 1,
                TimelineEventKind::EncounterBoundary {
                    state,
                    encounter_id,
                    ..
                } => match state {
                    EncounterState::Started => {
                        close_superseded_scope(
                            &mut active_encounter_scopes,
                            &mut encounter_scopes,
                            "timeline_encounter",
                            envelope.sequence,
                            envelope.time.observed_micros,
                            exact_build,
                            exact_protocol_pack_digest,
                            &session_id,
                        );
                        start_scope(
                            &mut active_encounter_scopes,
                            &mut encounter_ordinals,
                            "timeline_encounter",
                            encounter_id.clone(),
                            envelope.sequence,
                            envelope.time.observed_micros,
                            exact_wire(&envelope.provenance),
                            wire_connection_id(&envelope.provenance),
                        );
                    }
                    EncounterState::Cleared => close_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_scopes,
                        "timeline_encounter",
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_wire(&envelope.provenance),
                        "cleared",
                        exact_build,
                        exact_protocol_pack_digest,
                        &session_id,
                    ),
                    EncounterState::Wiped => close_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_scopes,
                        "timeline_encounter",
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_wire(&envelope.provenance),
                        "wiped",
                        exact_build,
                        exact_protocol_pack_digest,
                        &session_id,
                    ),
                    EncounterState::Ended => close_scope(
                        &mut active_encounter_scopes,
                        &mut encounter_scopes,
                        "timeline_encounter",
                        envelope.sequence,
                        envelope.time.observed_micros,
                        exact_wire(&envelope.provenance),
                        "ended",
                        exact_build,
                        exact_protocol_pack_digest,
                        &session_id,
                    ),
                },
                _ => {}
            },
            _ => {}
        }
    }
    for active in active_encounter_scopes {
        encounter_scopes.push(finish_scope(
            active,
            None,
            None,
            false,
            "open",
            exact_build,
            exact_protocol_pack_digest,
            &session_id,
        ));
    }
    let seal = reader
        .summary()
        .ok_or("RLOG is missing its canonical integrity seal")?;
    let integrity_seal_validated = true;
    let mut blockers = Vec::new();
    if !exact_build {
        blockers.push("game_build_mismatch");
    }
    if !exact_protocol_pack_digest {
        blockers.push("protocol_pack_digest_mismatch");
    }
    if data_gap_count > 0 {
        blockers.push("canonical_data_gap_present");
    }
    if recorder_pause_count > 0 {
        blockers.push("recorder_pause_present");
    }
    if dungeon_completed_count == 0 || run_boundary_completed_count == 0 {
        blockers.push("authoritative_completed_run_boundary_missing");
    }
    if damage_event_count == 0 {
        blockers.push("ordinary_damage_missing");
    }
    let closed_scope_candidate = blockers.is_empty();

    Ok(InputReport {
        path: display_path(path),
        bytes,
        session_id,
        game_build,
        protocol_pack_digest,
        exact_build,
        exact_protocol_pack_digest,
        integrity_seal_validated,
        sealed_content_sha256: seal.content_sha256.clone(),
        event_count: seal.event_count,
        first_observed_micros,
        last_observed_micros,
        damage_event_count,
        observed_damage: observed_damage.to_string(),
        data_gap_count,
        recorder_pause_count,
        dungeon_entered_count,
        dungeon_started_count,
        dungeon_completed_count,
        dungeon_failed_count,
        dungeon_exited_count,
        run_boundary_completed_count,
        closed_scope_candidate,
        blockers,
        encounter_scopes,
    })
}

fn wire_connection_id(provenance: &EventProvenance) -> Option<u64> {
    match &provenance.source {
        EvidenceSource::Wire { connection_id, .. } => Some(*connection_id),
        _ => None,
    }
}

fn exact_wire(provenance: &EventProvenance) -> bool {
    provenance.confidence == EvidenceConfidence::Exact
        && matches!(&provenance.source, EvidenceSource::Wire { .. })
}

#[allow(clippy::too_many_arguments)]
fn start_scope(
    active_scopes: &mut Vec<ActiveEncounterScope>,
    ordinals: &mut BTreeMap<&'static str, u64>,
    scope_kind: &'static str,
    encounter_id: Option<String>,
    start_sequence: u64,
    start_observed_micros: u64,
    start_exact_wire: bool,
    start_connection_id: Option<u64>,
) {
    let ordinal = ordinals.entry(scope_kind).or_default();
    *ordinal = ordinal.saturating_add(1);
    let mut relevant_connection_ids = BTreeSet::new();
    if let Some(connection_id) = start_connection_id {
        relevant_connection_ids.insert(connection_id);
    }
    active_scopes.push(ActiveEncounterScope {
        scope_kind,
        ordinal: *ordinal,
        encounter_id,
        start_sequence,
        start_observed_micros,
        start_exact_wire,
        relevant_connection_ids,
        global_data_gap_count: 0,
        data_gaps_by_connection: BTreeMap::new(),
        recorder_pause_count: 0,
        damage_event_count: 0,
        observed_damage: 0,
    });
}

#[allow(clippy::too_many_arguments)]
fn close_scope(
    active_scopes: &mut Vec<ActiveEncounterScope>,
    reports: &mut Vec<EncounterScopeReport>,
    scope_kind: &'static str,
    end_sequence: u64,
    end_observed_micros: u64,
    end_exact_wire: bool,
    terminal_state: &'static str,
    exact_build: bool,
    exact_protocol_pack_digest: bool,
    session_id: &str,
) {
    let Some(position) = active_scopes
        .iter()
        .rposition(|scope| scope.scope_kind == scope_kind)
    else {
        return;
    };
    let active = active_scopes.remove(position);
    reports.push(finish_scope(
        active,
        Some(end_sequence),
        Some(end_observed_micros),
        end_exact_wire,
        terminal_state,
        exact_build,
        exact_protocol_pack_digest,
        session_id,
    ));
}

#[allow(clippy::too_many_arguments)]
fn close_superseded_scope(
    active_scopes: &mut Vec<ActiveEncounterScope>,
    reports: &mut Vec<EncounterScopeReport>,
    scope_kind: &'static str,
    end_sequence: u64,
    end_observed_micros: u64,
    exact_build: bool,
    exact_protocol_pack_digest: bool,
    session_id: &str,
) {
    close_scope(
        active_scopes,
        reports,
        scope_kind,
        end_sequence,
        end_observed_micros,
        false,
        "superseded",
        exact_build,
        exact_protocol_pack_digest,
        session_id,
    );
}

#[allow(clippy::too_many_arguments)]
fn finish_scope(
    active: ActiveEncounterScope,
    end_sequence: Option<u64>,
    end_observed_micros: Option<u64>,
    end_exact_wire: bool,
    terminal_state: &'static str,
    exact_build: bool,
    exact_protocol_pack_digest: bool,
    session_id: &str,
) -> EncounterScopeReport {
    let relevant_data_gap_count = active.relevant_connection_ids.iter().fold(
        active.global_data_gap_count,
        |count, connection_id| {
            count.saturating_add(
                active
                    .data_gaps_by_connection
                    .get(connection_id)
                    .copied()
                    .unwrap_or_default(),
            )
        },
    );
    let mut blockers = Vec::new();
    if !exact_build {
        blockers.push("game_build_mismatch");
    }
    if !exact_protocol_pack_digest {
        blockers.push("protocol_pack_digest_mismatch");
    }
    if !active.start_exact_wire {
        blockers.push("authoritative_start_boundary_missing");
    }
    if end_sequence.is_none() || !end_exact_wire {
        blockers.push("authoritative_end_boundary_missing");
    }
    if terminal_state != "cleared" {
        blockers.push("authoritative_cleared_boundary_missing");
    }
    if relevant_data_gap_count > 0 {
        blockers.push("canonical_data_gap_present");
    }
    if active.recorder_pause_count > 0 {
        blockers.push("recorder_pause_present");
    }
    if active.damage_event_count == 0 {
        blockers.push("ordinary_damage_missing");
    }
    let closed_scope_candidate = blockers.is_empty();

    EncounterScopeReport {
        scope_id: format!("{session_id}:{}:{:04}", active.scope_kind, active.ordinal),
        scope_kind: active.scope_kind,
        ordinal: active.ordinal,
        encounter_id: active.encounter_id,
        start_sequence: active.start_sequence,
        end_sequence,
        start_observed_micros: active.start_observed_micros,
        end_observed_micros,
        start_exact_wire: active.start_exact_wire,
        end_exact_wire,
        terminal_state,
        relevant_connection_ids: active.relevant_connection_ids.into_iter().collect(),
        damage_event_count: active.damage_event_count,
        observed_damage: active.observed_damage.to_string(),
        relevant_data_gap_count,
        recorder_pause_count: active.recorder_pause_count,
        closed_scope_candidate,
        blockers,
    }
}

fn summarize(inputs: &[InputReport]) -> Summary {
    let mut summary = Summary {
        input_count: inputs.len(),
        ..Summary::default()
    };
    let mut observed_damage = 0_i128;
    for input in inputs {
        summary.exact_build_input_count += usize::from(input.exact_build);
        summary.exact_protocol_pack_input_count += usize::from(input.exact_protocol_pack_digest);
        summary.total_events = summary.total_events.saturating_add(input.event_count);
        summary.total_damage_events = summary
            .total_damage_events
            .saturating_add(input.damage_event_count);
        observed_damage = observed_damage
            .saturating_add(input.observed_damage.parse::<i128>().unwrap_or_default());
        summary.total_data_gaps = summary.total_data_gaps.saturating_add(input.data_gap_count);
        summary.total_recorder_pauses = summary
            .total_recorder_pauses
            .saturating_add(input.recorder_pause_count);
        if input.closed_scope_candidate {
            summary.closed_scope_candidates.push(input.path.clone());
        }
        summary.encounter_scope_count = summary
            .encounter_scope_count
            .saturating_add(input.encounter_scopes.len());
        summary.encounter_scope_candidates.extend(
            input
                .encounter_scopes
                .iter()
                .filter(|scope| scope.closed_scope_candidate)
                .map(|scope| scope.scope_id.clone()),
        );
    }
    summary.closed_scope_candidate_count = summary.closed_scope_candidates.len();
    summary.encounter_scope_candidate_count = summary.encounter_scope_candidates.len();
    summary.total_observed_damage = observed_damage.to_string();
    summary
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let expected_build = take_value(&mut values, "--expected-build")?
        .into_string()
        .map_err(|_| "--expected-build must be UTF-8")?;
    if expected_build.is_empty() || !expected_build.bytes().all(|value| value.is_ascii_digit()) {
        return Err("--expected-build requires a numeric build ID".to_owned());
    }
    let expected_protocol_pack_digest =
        take_optional_value(&mut values, "--expected-protocol-pack-digest")
            .map(|value| value.into_string().map_err(|_| "digest must be UTF-8"))
            .transpose()?;
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let mut rlogs = Vec::new();
    while let Some(value) = take_optional_value(&mut values, "--rlog") {
        rlogs.push(PathBuf::from(value));
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        expected_build,
        expected_protocol_pack_digest,
        rlogs,
        output,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag).ok_or_else(usage)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return Some(OsString::new());
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn report_sha256(report: &Report) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(report)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn write_json_new(path: &Path, value: &impl Serialize) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn usage() -> String {
    "usage: rlogs-bpsr-rlog-closure-audit --expected-build <id> [--expected-protocol-pack-digest <sha256:...>] --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <audit.json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_scope() -> ActiveEncounterScope {
        ActiveEncounterScope {
            scope_kind: "dungeon_boss",
            ordinal: 1,
            encounter_id: Some("boss-1".to_owned()),
            start_sequence: 10,
            start_observed_micros: 100,
            start_exact_wire: true,
            relevant_connection_ids: BTreeSet::from([1]),
            global_data_gap_count: 0,
            data_gaps_by_connection: BTreeMap::new(),
            recorder_pause_count: 0,
            damage_event_count: 1,
            observed_damage: 123,
        }
    }

    #[test]
    fn unrelated_connection_gap_does_not_reject_scope() {
        let mut active = active_scope();
        active.data_gaps_by_connection.insert(2, 1);
        let report = finish_scope(
            active,
            Some(20),
            Some(200),
            true,
            "cleared",
            true,
            true,
            "session",
        );

        assert_eq!(report.relevant_data_gap_count, 0);
        assert!(report.closed_scope_candidate);
        assert!(report.blockers.is_empty());
    }

    #[test]
    fn relevant_connection_gap_rejects_scope() {
        let mut active = active_scope();
        active.data_gaps_by_connection.insert(1, 1);
        let report = finish_scope(
            active,
            Some(20),
            Some(200),
            true,
            "cleared",
            true,
            true,
            "session",
        );

        assert_eq!(report.relevant_data_gap_count, 1);
        assert!(!report.closed_scope_candidate);
        assert!(report.blockers.contains(&"canonical_data_gap_present"));
    }

    #[test]
    fn global_gap_rejects_scope() {
        let mut active = active_scope();
        active.global_data_gap_count = 1;
        let report = finish_scope(
            active,
            Some(20),
            Some(200),
            true,
            "cleared",
            true,
            true,
            "session",
        );

        assert_eq!(report.relevant_data_gap_count, 1);
        assert!(!report.closed_scope_candidate);
        assert!(report.blockers.contains(&"canonical_data_gap_present"));
    }

    #[test]
    fn non_cleared_or_non_exact_end_rejects_scope() {
        let report = finish_scope(
            active_scope(),
            Some(20),
            Some(200),
            false,
            "wiped",
            true,
            true,
            "session",
        );

        assert!(!report.closed_scope_candidate);
        assert!(
            report
                .blockers
                .contains(&"authoritative_end_boundary_missing")
        );
        assert!(
            report
                .blockers
                .contains(&"authoritative_cleared_boundary_missing")
        );
    }
}
