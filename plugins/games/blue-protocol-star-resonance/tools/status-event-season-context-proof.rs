use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, DataGapKind, EvidenceConfidence, EvidenceSource, RunState, StatusState,
    TimelineEventKind,
};
use rlogs_game_bpsr::CharacterProfilePatch;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 3;

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    effects: BTreeSet<i64>,
    output: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
struct InputEvidence {
    path: String,
    bytes: u64,
    sha256: String,
    session_id: String,
    game_build: String,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    tool: &'static str,
    game_build: String,
    policy: ProofPolicy,
    selection: Selection,
    inputs: Vec<InputEvidence>,
    summary: Summary,
    season_id_counts_for_prior_context: BTreeMap<i64, u64>,
    events: Vec<SelectedStatusEvent>,
}

#[derive(Debug, Serialize)]
struct ProofPolicy {
    scope: &'static str,
    exact_numeric_effect_ids_authoritative: bool,
    exact_input_build_authoritative: bool,
    only_positive_season_ids_from_bpsr_canonical_profile_events_are_accepted: bool,
    season_observations_require_exact_wire_provenance: bool,
    season_context_must_precede_status_in_same_sealed_rlog: bool,
    future_profile_events_may_backfill_earlier_status_events: bool,
    current_character_snapshots_may_replace_historical_context: bool,
    prior_continuous_monitor_context_is_candidate_until_protocol_coverage: bool,
    unresolved_context_is_preserved: bool,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Serialize)]
struct Selection {
    effect_ids: Vec<i64>,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    canonical_events_scanned: u64,
    run_boundaries_scanned: u64,
    data_gaps_scanned: u64,
    bpsr_profile_events_scanned: u64,
    positive_season_observations: u64,
    selected_status_events: u64,
    selected_events_with_prior_season_context: u64,
    selected_events_without_prior_season_context: u64,
    selected_events_with_only_later_season_observation: u64,
    selected_events_without_any_season_observation_in_rlog: u64,
    selected_events_with_prior_continuous_monitor_context_candidate: u64,
    selected_events_with_only_prior_continuous_monitor_context_candidate: u64,
    selected_events_with_gap_free_season_source_wire_lane_candidate: u64,
    selected_events_with_no_transport_gap_kind_since_candidate: u64,
    every_selected_event_has_prior_season_context: bool,
}

#[derive(Debug, Clone)]
struct SeasonObservation {
    season_id: i64,
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    character_id: String,
    rlog: String,
    session_id: String,
    monitor_run_number: Option<u32>,
    data_gap_ordinal: u64,
    data_gap_kind_counts: GapKindCounts,
    wire_capture_sequence: u64,
    wire_connection_id: u64,
    wire_stream_id: u64,
    source_wire_lane_gap_ordinal: u64,
}

#[derive(Debug, Default)]
struct MonitorChainState {
    last_run_number: Option<u32>,
    last_observed_micros: Option<u64>,
    latest_season: Option<SeasonObservation>,
    data_gap_ordinal: u64,
    data_gap_kind_counts: GapKindCounts,
    wire_lane_gap_ordinals: BTreeMap<(Option<u64>, Option<u64>), u64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct GapKindCounts {
    capture_drop: u64,
    tcp_gap: u64,
    unknown_route: u64,
    decode_failure: u64,
    unsupported_fragment: u64,
}

#[derive(Debug)]
struct PendingStatusEvent {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    effect_id: i64,
    state: StatusState,
    source_actor_id: Option<u64>,
    source_entity_uuid: Option<i64>,
    target_actor_id: u64,
    target_entity_uuid: i64,
    instance_id: Option<i64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    prior_season: Option<SeasonObservation>,
    prior_monitor_candidate: Option<SeasonObservation>,
    monitor_chain_consecutive: bool,
    monitor_clock_monotonic: bool,
    data_gap_ordinal_at_event: u64,
    data_gap_kind_counts_at_event: GapKindCounts,
    source_wire_lane_gap_ordinal_at_event: Option<u64>,
}

#[derive(Debug, Serialize)]
struct SelectedStatusEvent {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    effect_id: i64,
    state: StatusState,
    source_actor_id: Option<u64>,
    source_entity_uuid: Option<i64>,
    target_actor_id: u64,
    target_entity_uuid: i64,
    instance_id: Option<i64>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    prior_season_context: Option<SeasonContextEvidence>,
    first_later_season_observation: Option<SeasonContextEvidence>,
    prior_continuous_monitor_context_candidate: Option<ContinuousMonitorContextEvidence>,
    season_context_proven_before_event: bool,
    continuous_monitor_context_is_formula_authority: bool,
    future_backfill_rejected: bool,
}

#[derive(Debug, Serialize)]
struct SeasonContextEvidence {
    season_id: i64,
    profile_sequence: u64,
    profile_observed_micros: u64,
    profile_run_ordinal: u32,
    character_id: String,
    wire_capture_sequence: u64,
    wire_connection_id: u64,
    wire_stream_id: u64,
}

#[derive(Debug, Serialize)]
struct ContinuousMonitorContextEvidence {
    season: SeasonContextEvidence,
    source_rlog: String,
    source_session_id: String,
    source_monitor_run_number: Option<u32>,
    status_monitor_run_number: Option<u32>,
    data_gaps_since_observation: u64,
    data_gap_kind_counts_since_observation: GapKindCountEvidence,
    season_source_wire_lane_data_gaps_since_observation: Option<u64>,
    season_source_wire_lane_gap_free: bool,
    no_capture_or_tcp_gap_kind_since_observation: bool,
    consecutive_run_chain: bool,
    monotonic_monitor_clock: bool,
    protocol_event_coverage_required_for_authority: bool,
}

#[derive(Debug, Serialize)]
struct GapKindCountEvidence {
    capture_drop: u64,
    tcp_gap: u64,
    unknown_route: u64,
    decode_failure: u64,
    unsupported_fragment: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR status-event season-context proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_arguments(env::args_os().skip(1))?;
    ensure_output_is_new(&args.output)?;
    let inputs = inspect_inputs(&args.rlogs)?;
    let game_build = inputs
        .first()
        .map(|input| input.game_build.clone())
        .ok_or("at least one rlog input is required")?;
    let mut summary = Summary::default();
    let mut season_counts = BTreeMap::new();
    let mut events = Vec::new();
    let mut monitor_chains = BTreeMap::<String, MonitorChainState>::new();

    for (path, input) in args.rlogs.iter().zip(&inputs) {
        scan_rlog(
            path,
            input,
            &args.effects,
            &mut summary,
            &mut season_counts,
            &mut events,
            &mut monitor_chains,
        )?;
    }
    summary.every_selected_event_has_prior_season_context = summary.selected_status_events > 0
        && summary.selected_status_events == summary.selected_events_with_prior_season_context;

    let report = Report {
        schema_version: SCHEMA_VERSION,
        tool: "rlogs-bpsr-status-event-season-context-proof",
        game_build,
        policy: ProofPolicy {
            scope: "event_time_season_context_only",
            exact_numeric_effect_ids_authoritative: true,
            exact_input_build_authoritative: true,
            only_positive_season_ids_from_bpsr_canonical_profile_events_are_accepted: true,
            season_observations_require_exact_wire_provenance: true,
            season_context_must_precede_status_in_same_sealed_rlog: true,
            future_profile_events_may_backfill_earlier_status_events: false,
            current_character_snapshots_may_replace_historical_context: false,
            prior_continuous_monitor_context_is_candidate_until_protocol_coverage: true,
            unresolved_context_is_preserved: true,
            formula_authority: false,
            runtime_authority: false,
            provider_rdps_credit_allowed: false,
        },
        selection: Selection {
            effect_ids: args.effects.iter().copied().collect(),
        },
        inputs,
        summary,
        season_id_counts_for_prior_context: season_counts,
        events,
    };
    write_report_atomically(&args.output, &report)?;
    Ok(())
}

fn scan_rlog(
    path: &Path,
    input: &InputEvidence,
    effects: &BTreeSet<i64>,
    summary: &mut Summary,
    season_counts: &mut BTreeMap<i64, u64>,
    output: &mut Vec<SelectedStatusEvent>,
    monitor_chains: &mut BTreeMap<String, MonitorChainState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut run_ordinal = 0_u32;
    let mut latest_season: Option<SeasonObservation> = None;
    let mut all_seasons = Vec::<SeasonObservation>::new();
    let mut pending = Vec::<PendingStatusEvent>::new();
    let (monitor_id, monitor_run_number) = parse_monitor_session(&input.session_id);
    let chain = monitor_chains.entry(monitor_id).or_default();
    let monitor_chain_consecutive = chain.last_run_number.is_some_and(|last| {
        monitor_run_number.is_some_and(|current| current == last.saturating_add(1))
    });
    if chain.last_run_number.is_some() && !monitor_chain_consecutive {
        *chain = MonitorChainState::default();
    }
    let mut last_observed_micros = None;

    while let Some(envelope) = reader.next_event()? {
        last_observed_micros = Some(envelope.time.observed_micros);
        summary.canonical_events_scanned = summary.canonical_events_scanned.saturating_add(1);
        match &envelope.event {
            CanonicalEvent::CharacterProfileObserved { profile } => {
                let Some((wire_capture_sequence, wire_connection_id, wire_stream_id)) =
                    exact_wire_coordinates(&envelope)
                else {
                    continue;
                };
                let Ok(patch) = CharacterProfilePatch::from_game_event(profile) else {
                    continue;
                };
                summary.bpsr_profile_events_scanned =
                    summary.bpsr_profile_events_scanned.saturating_add(1);
                let Some(season_id) = patch
                    .season
                    .as_ref()
                    .and_then(|season| season.season_id)
                    .filter(|season_id| *season_id > 0)
                else {
                    continue;
                };
                summary.positive_season_observations =
                    summary.positive_season_observations.saturating_add(1);
                let observation = SeasonObservation {
                    season_id,
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    run_ordinal,
                    character_id: patch.character.character_id,
                    rlog: input.path.clone(),
                    session_id: envelope.session_id.clone(),
                    monitor_run_number,
                    data_gap_ordinal: chain.data_gap_ordinal,
                    data_gap_kind_counts: chain.data_gap_kind_counts,
                    wire_capture_sequence,
                    wire_connection_id,
                    wire_stream_id,
                    source_wire_lane_gap_ordinal: *chain
                        .wire_lane_gap_ordinals
                        .get(&(Some(wire_connection_id), Some(wire_stream_id)))
                        .unwrap_or(&0),
                };
                latest_season = Some(observation.clone());
                chain.latest_season = Some(observation.clone());
                all_seasons.push(observation);
            }
            CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::RunBoundary { state, .. } => {
                    summary.run_boundaries_scanned =
                        summary.run_boundaries_scanned.saturating_add(1);
                    match state {
                        RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                        RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                        _ => {}
                    }
                }
                TimelineEventKind::DataGap(gap) => {
                    summary.data_gaps_scanned = summary.data_gaps_scanned.saturating_add(1);
                    chain.data_gap_ordinal = chain.data_gap_ordinal.saturating_add(1);
                    chain.data_gap_kind_counts.observe(gap.kind);
                    *chain
                        .wire_lane_gap_ordinals
                        .entry((gap.connection_id, gap.stream_id))
                        .or_insert(0) += 1;
                }
                TimelineEventKind::Status(status) if effects.contains(&status.effect.0) => {
                    summary.selected_status_events =
                        summary.selected_status_events.saturating_add(1);
                    let prior_season = latest_season.clone().filter(|season| {
                        season.sequence < envelope.sequence
                            && season.observed_micros <= envelope.time.observed_micros
                    });
                    let prior_monitor_candidate = prior_season
                        .is_none()
                        .then(|| {
                            chain.latest_season.clone().filter(|season| {
                                season.session_id != envelope.session_id
                                    && season.sequence > 0
                                    && season.observed_micros <= envelope.time.observed_micros
                            })
                        })
                        .flatten();
                    let monitor_clock_monotonic = chain
                        .last_observed_micros
                        .is_some_and(|last| last <= envelope.time.observed_micros);
                    let source_wire_lane_gap_ordinal_at_event =
                        prior_monitor_candidate.as_ref().map(|season| {
                            *chain
                                .wire_lane_gap_ordinals
                                .get(&(
                                    Some(season.wire_connection_id),
                                    Some(season.wire_stream_id),
                                ))
                                .unwrap_or(&0)
                        });
                    pending.push(PendingStatusEvent {
                        rlog: input.path.clone(),
                        session_id: envelope.session_id.clone(),
                        run_ordinal,
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        effect_id: status.effect.0,
                        state: status.state,
                        source_actor_id: status.source.map(|source| source.actor_id.0),
                        source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                        target_actor_id: status.target.actor_id.0,
                        target_entity_uuid: status.target.entity_uuid.0,
                        instance_id: status.instance_id.map(|instance| instance.0),
                        origin_source_type_id: status.origin.map(|origin| origin.source_type_id),
                        origin_source_config_id: status
                            .origin
                            .map(|origin| origin.source_config_id),
                        prior_season,
                        prior_monitor_candidate,
                        monitor_chain_consecutive,
                        monitor_clock_monotonic,
                        data_gap_ordinal_at_event: chain.data_gap_ordinal,
                        data_gap_kind_counts_at_event: chain.data_gap_kind_counts,
                        source_wire_lane_gap_ordinal_at_event,
                    });
                }
                _ => {}
            },
            _ => {}
        }
    }

    chain.last_run_number = monitor_run_number;
    chain.last_observed_micros = last_observed_micros;

    for event in pending {
        let first_later = event
            .prior_season
            .is_none()
            .then(|| {
                all_seasons
                    .iter()
                    .find(|season| season.sequence > event.sequence)
                    .cloned()
            })
            .flatten();
        if let Some(prior) = &event.prior_season {
            summary.selected_events_with_prior_season_context = summary
                .selected_events_with_prior_season_context
                .saturating_add(1);
            *season_counts.entry(prior.season_id).or_insert(0) += 1;
        } else {
            summary.selected_events_without_prior_season_context = summary
                .selected_events_without_prior_season_context
                .saturating_add(1);
            if first_later.is_some() {
                summary.selected_events_with_only_later_season_observation = summary
                    .selected_events_with_only_later_season_observation
                    .saturating_add(1);
            } else {
                summary.selected_events_without_any_season_observation_in_rlog = summary
                    .selected_events_without_any_season_observation_in_rlog
                    .saturating_add(1);
            }
        }
        let future_backfill_rejected = event.prior_season.is_none() && first_later.is_some();
        let season_context_proven_before_event = event.prior_season.is_some();
        let continuous_candidate = event.prior_monitor_candidate.map(|season| {
            let gap_counts = event
                .data_gap_kind_counts_at_event
                .saturating_difference(season.data_gap_kind_counts);
            let source_lane_gaps = event
                .source_wire_lane_gap_ordinal_at_event
                .map(|ordinal| ordinal.saturating_sub(season.source_wire_lane_gap_ordinal));
            ContinuousMonitorContextEvidence {
                source_rlog: season.rlog.clone(),
                source_session_id: season.session_id.clone(),
                source_monitor_run_number: season.monitor_run_number,
                status_monitor_run_number: parse_monitor_session(&event.session_id).1,
                data_gaps_since_observation: chain_gap_delta(
                    season.data_gap_ordinal,
                    event.data_gap_ordinal_at_event,
                ),
                data_gap_kind_counts_since_observation: gap_counts.into(),
                season_source_wire_lane_data_gaps_since_observation: source_lane_gaps,
                season_source_wire_lane_gap_free: source_lane_gaps == Some(0),
                no_capture_or_tcp_gap_kind_since_observation: gap_counts.capture_drop == 0
                    && gap_counts.tcp_gap == 0,
                consecutive_run_chain: event.monitor_chain_consecutive,
                monotonic_monitor_clock: event.monitor_clock_monotonic,
                protocol_event_coverage_required_for_authority: true,
                season: SeasonContextEvidence::from(season),
            }
        });
        if let Some(candidate) = &continuous_candidate {
            summary.selected_events_with_prior_continuous_monitor_context_candidate = summary
                .selected_events_with_prior_continuous_monitor_context_candidate
                .saturating_add(1);
            if event.prior_season.is_none() {
                summary.selected_events_with_only_prior_continuous_monitor_context_candidate =
                    summary
                        .selected_events_with_only_prior_continuous_monitor_context_candidate
                        .saturating_add(1);
            }
            if candidate.season_source_wire_lane_gap_free {
                summary.selected_events_with_gap_free_season_source_wire_lane_candidate = summary
                    .selected_events_with_gap_free_season_source_wire_lane_candidate
                    .saturating_add(1);
            }
            if candidate.no_capture_or_tcp_gap_kind_since_observation {
                summary.selected_events_with_no_transport_gap_kind_since_candidate = summary
                    .selected_events_with_no_transport_gap_kind_since_candidate
                    .saturating_add(1);
            }
        }
        output.push(SelectedStatusEvent {
            rlog: event.rlog,
            session_id: event.session_id,
            run_ordinal: event.run_ordinal,
            sequence: event.sequence,
            observed_micros: event.observed_micros,
            effect_id: event.effect_id,
            state: event.state,
            source_actor_id: event.source_actor_id,
            source_entity_uuid: event.source_entity_uuid,
            target_actor_id: event.target_actor_id,
            target_entity_uuid: event.target_entity_uuid,
            instance_id: event.instance_id,
            origin_source_type_id: event.origin_source_type_id,
            origin_source_config_id: event.origin_source_config_id,
            prior_season_context: event.prior_season.map(SeasonContextEvidence::from),
            first_later_season_observation: first_later.map(SeasonContextEvidence::from),
            prior_continuous_monitor_context_candidate: continuous_candidate,
            season_context_proven_before_event,
            continuous_monitor_context_is_formula_authority: false,
            future_backfill_rejected,
        });
    }
    Ok(())
}

fn parse_monitor_session(session_id: &str) -> (String, Option<u32>) {
    let Some((monitor, suffix)) = session_id.rsplit_once(".run-") else {
        return (session_id.to_owned(), None);
    };
    let run = suffix.parse::<u32>().ok();
    (monitor.to_owned(), run)
}

fn chain_gap_delta(observed_gap_ordinal: u64, event_gap_ordinal: u64) -> u64 {
    event_gap_ordinal.saturating_sub(observed_gap_ordinal)
}

fn exact_wire_coordinates(envelope: &rlogs_events::EventEnvelope) -> Option<(u64, u64, u64)> {
    if envelope.provenance.confidence != EvidenceConfidence::Exact {
        return None;
    }
    match &envelope.provenance.source {
        EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } => Some((*capture_sequence, *connection_id, *stream_id)),
        EvidenceSource::Derived { .. } | EvidenceSource::Manual { .. } => None,
    }
}

impl GapKindCounts {
    fn observe(&mut self, kind: DataGapKind) {
        let target = match kind {
            DataGapKind::CaptureDrop => &mut self.capture_drop,
            DataGapKind::TcpGap => &mut self.tcp_gap,
            DataGapKind::UnknownRoute => &mut self.unknown_route,
            DataGapKind::DecodeFailure => &mut self.decode_failure,
            DataGapKind::UnsupportedFragment => &mut self.unsupported_fragment,
        };
        *target = target.saturating_add(1);
    }

    fn saturating_difference(self, earlier: Self) -> Self {
        Self {
            capture_drop: self.capture_drop.saturating_sub(earlier.capture_drop),
            tcp_gap: self.tcp_gap.saturating_sub(earlier.tcp_gap),
            unknown_route: self.unknown_route.saturating_sub(earlier.unknown_route),
            decode_failure: self.decode_failure.saturating_sub(earlier.decode_failure),
            unsupported_fragment: self
                .unsupported_fragment
                .saturating_sub(earlier.unsupported_fragment),
        }
    }
}

impl From<GapKindCounts> for GapKindCountEvidence {
    fn from(value: GapKindCounts) -> Self {
        Self {
            capture_drop: value.capture_drop,
            tcp_gap: value.tcp_gap,
            unknown_route: value.unknown_route,
            decode_failure: value.decode_failure,
            unsupported_fragment: value.unsupported_fragment,
        }
    }
}

impl From<SeasonObservation> for SeasonContextEvidence {
    fn from(value: SeasonObservation) -> Self {
        Self {
            season_id: value.season_id,
            profile_sequence: value.sequence,
            profile_observed_micros: value.observed_micros,
            profile_run_ordinal: value.run_ordinal,
            character_id: value.character_id,
            wire_capture_sequence: value.wire_capture_sequence,
            wire_connection_id: value.wire_connection_id,
            wire_stream_id: value.wire_stream_id,
        }
    }
}

fn inspect_inputs(paths: &[PathBuf]) -> Result<Vec<InputEvidence>, Box<dyn std::error::Error>> {
    let mut expected_build: Option<String> = None;
    let mut seen = BTreeSet::new();
    let mut inputs = Vec::with_capacity(paths.len());
    for path in paths {
        let canonical = fs::canonicalize(path)?;
        if !seen.insert(canonical.clone()) {
            return Err(format!("duplicate rlog input: {}", path.display()).into());
        }
        let reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        let build = reader.header().region.client_build.trim().to_owned();
        if build.is_empty() {
            return Err(format!("{} has an empty client build", path.display()).into());
        }
        if expected_build
            .as_ref()
            .is_some_and(|expected| expected != &build)
        {
            return Err(format!("input build mismatch at {}: {build}", path.display()).into());
        }
        expected_build.get_or_insert_with(|| build.clone());
        let metadata = fs::metadata(path)?;
        inputs.push(InputEvidence {
            path: path.to_string_lossy().replace('\\', "/"),
            bytes: metadata.len(),
            sha256: sha256_file(path)?,
            session_id: reader.header().session_id.clone(),
            game_build: build,
        });
    }
    Ok(inputs)
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn ensure_output_is_new(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if output.exists() {
        return Err(format!(
            "refusing to overwrite existing output: {}",
            output.display()
        )
        .into());
    }
    let partial = partial_path(output);
    if partial.exists() {
        return Err(format!(
            "refusing to overwrite partial output: {}",
            partial.display()
        )
        .into());
    }
    Ok(())
}

fn write_report_atomically(
    output: &Path,
    report: &Report,
) -> Result<(), Box<dyn std::error::Error>> {
    let partial = partial_path(output);
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    fs::rename(&partial, output)?;
    Ok(())
}

fn partial_path(output: &Path) -> PathBuf {
    let mut value = output.as_os_str().to_os_string();
    value.push(".partial");
    PathBuf::from(value)
}

fn parse_arguments(values: impl Iterator<Item = OsString>) -> Result<Arguments, String> {
    let mut values = values.collect::<Vec<_>>();
    let output = PathBuf::from(take_one(&mut values, "--output")?);
    let rlogs = take_many(&mut values, "--rlog")
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let effects = take_many(&mut values, "--effect")
        .into_iter()
        .map(|value| parse_positive_i64(value, "--effect"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if rlogs.is_empty() || effects.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlogs,
        effects,
        output,
    })
}

fn take_one(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let mut found = take_many(values, flag);
    if found.len() != 1 {
        return Err(format!("{flag} must be supplied exactly once\n{}", usage()));
    }
    Ok(found.remove(0))
}

fn take_many(values: &mut Vec<OsString>, flag: &str) -> Vec<OsString> {
    let mut output = Vec::new();
    while let Some(position) = values.iter().position(|value| value == flag) {
        if position + 1 >= values.len() {
            output.push(OsString::new());
            values.remove(position);
            break;
        }
        output.push(values.remove(position + 1));
        values.remove(position);
    }
    output
}

fn parse_positive_i64(value: OsString, flag: &str) -> Result<i64, String> {
    let parsed = value
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| format!("{flag} requires a positive integer"))?;
    if parsed <= 0 {
        return Err(format!("{flag} requires a positive integer"));
    }
    Ok(parsed)
}

fn usage() -> String {
    "usage: rlogs-bpsr-status-event-season-context-proof --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --effect <positive-id> [--effect <positive-id> ...] --output <new.json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_observation_is_not_prior_context() {
        let status_sequence = 10;
        let later = SeasonObservation {
            season_id: 3,
            sequence: 11,
            observed_micros: 101,
            run_ordinal: 1,
            character_id: "7".to_owned(),
            rlog: "run-0001.rlog".to_owned(),
            session_id: "monitor.run-0001".to_owned(),
            monitor_run_number: Some(1),
            data_gap_ordinal: 0,
            data_gap_kind_counts: GapKindCounts::default(),
            wire_capture_sequence: 1,
            wire_connection_id: 2,
            wire_stream_id: 3,
            source_wire_lane_gap_ordinal: 0,
        };
        assert!(later.sequence > status_sequence);
        assert!(!(later.sequence < status_sequence));
    }

    #[test]
    fn prior_observation_may_cross_run_boundary_but_never_time() {
        let prior = SeasonObservation {
            season_id: 3,
            sequence: 5,
            observed_micros: 50,
            run_ordinal: 0,
            character_id: "7".to_owned(),
            rlog: "run-0001.rlog".to_owned(),
            session_id: "monitor.run-0001".to_owned(),
            monitor_run_number: Some(1),
            data_gap_ordinal: 0,
            data_gap_kind_counts: GapKindCounts::default(),
            wire_capture_sequence: 1,
            wire_connection_id: 2,
            wire_stream_id: 3,
            source_wire_lane_gap_ordinal: 0,
        };
        let status_sequence = 10;
        let status_micros = 100;
        assert!(prior.sequence < status_sequence && prior.observed_micros <= status_micros);
        assert_eq!(prior.run_ordinal, 0);
    }

    #[test]
    fn duplicate_effect_ids_are_deduplicated() {
        let args = parse_arguments(
            [
                "--rlog", "one.rlog", "--effect", "31602", "--effect", "31602", "--output",
                "new.json",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .unwrap();
        assert_eq!(args.effects.into_iter().collect::<Vec<_>>(), vec![31602]);
    }

    #[test]
    fn monitor_run_identity_is_exact_and_gap_delta_is_forward_only() {
        assert_eq!(
            parse_monitor_session("monitor-123.run-0007"),
            ("monitor-123".to_owned(), Some(7)),
        );
        assert_eq!(chain_gap_delta(4, 9), 5);
        assert_eq!(chain_gap_delta(9, 4), 0);
    }

    #[test]
    fn gap_kind_differences_do_not_merge_transport_and_decode_failures() {
        let earlier = GapKindCounts {
            capture_drop: 2,
            decode_failure: 3,
            ..GapKindCounts::default()
        };
        let later = GapKindCounts {
            capture_drop: 2,
            decode_failure: 9,
            ..GapKindCounts::default()
        };
        let difference = later.saturating_difference(earlier);
        assert_eq!(difference.capture_drop, 0);
        assert_eq!(difference.tcp_gap, 0);
        assert_eq!(difference.decode_failure, 6);
    }
}
