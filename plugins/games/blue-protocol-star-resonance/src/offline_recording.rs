use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::Arc;

use rlogs_capture::{
    CaptureError, CaptureFileFormat, CaptureSource, CaptureSourceKind, CaptureSourceMetadata,
    CapturedFrame, ValidatedCapture,
};
use rlogs_core::GameConnectionFilter;
use rlogs_events::{EventTopic, RegionEvidence, RegionIdentity};
use rlogs_log_format::{RlogError, RlogHeader, RlogSeal, RlogWriter};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CaptureGap, CaptureGapKind, CaptureRecord, CaptureRecordDraft, CaptureRecordKind,
    CoverageReport, DecoderKind, GameBuild, JsonlJournalError, JsonlJournalReader,
    ObjectiveCatalogResolver, ProtocolDecodeBatch, ProtocolDecodeStatus, ProtocolFeature,
    ProtocolPack, ProtocolPackRouteDisposition, ProtocolRuntime, ProtocolRuntimeConfig,
    ProtocolRuntimeError, ResearchPipeline, RouteKey,
};

pub const OFFLINE_RECORDING_REPORT_SCHEMA_VERSION: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineRecordingLimits {
    pub maximum_frames: u64,
    pub maximum_records: u64,
    pub maximum_unique_routes: usize,
}

impl Default for OfflineRecordingLimits {
    fn default() -> Self {
        Self {
            maximum_frames: 10_000_000,
            maximum_records: 20_000_000,
            maximum_unique_routes: 65_536,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OfflineRecordingConfig {
    pub session_id: String,
    pub producer: String,
    pub build: GameBuild,
    pub region: RegionIdentity,
    pub region_evidence: Vec<RegionEvidence>,
    pub limits: OfflineRecordingLimits,
    pub decoder: ProtocolRuntimeConfig,
    pub objective_catalog: Option<Arc<dyn ObjectiveCatalogResolver>>,
}

pub struct OfflineRecordingResult<W> {
    pub output: W,
    pub report: OfflineRecordingReport,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JournalTailPolicy {
    #[default]
    Strict,
    /// Preserve the valid prefix and append an explicit malformed-frame gap
    /// only when the final, unterminated JSON line ends unexpectedly.
    RecoverTruncatedFinalLine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineRecordingReport {
    pub schema_version: u16,
    pub session_id: String,
    pub source: CaptureSourceMetadata,
    pub protocol_pack_id: String,
    pub protocol_pack_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_pack_transition: Option<ProtocolPackTransitionRecording>,
    pub frame_count: u64,
    pub record_count: u64,
    pub rlog: RlogSeal,
    pub capture: CaptureCoverageSummary,
    pub decoder: DecodeCoverageSummary,
    pub event_topics: Vec<EventTopicCoverage>,
    pub routes: Vec<RouteRecordingCoverage>,
    pub features: Vec<FeatureRecordingCoverage>,
    pub gaps: Vec<GapRecordingCoverage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolPackTransitionRecording {
    pub policy: String,
    pub source_protocol_pack_id: String,
    pub source_protocol_pack_digest: String,
    pub destination_protocol_pack_id: String,
    pub destination_protocol_pack_digest: String,
    pub demoted_route_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureCoverageSummary {
    pub packet_count: u64,
    pub gap_count: u64,
    pub wire_bytes: u64,
    pub application_bytes: u64,
    pub unrouted_packet_count: u64,
    pub unclassified_fragment_packet_count: u64,
    pub known_route_count: u64,
    pub unknown_route_count: u64,
    pub known_packet_count: u64,
    pub unknown_packet_count: u64,
    pub allowed_packet_count: u64,
    pub opaque_packet_count: u64,
    pub prohibited_packet_count: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodeCoverageSummary {
    pub decoded_records: u64,
    /// Count only; the announced hostname and IP are intentionally omitted.
    pub announced_server_records: u64,
    pub server_clock_records: u64,
    pub capture_gap_records: u64,
    pub unrouted_records: u64,
    pub opaque_local_only_records: u64,
    pub prohibited_records: u64,
    pub missing_application_payload_records: u64,
    pub decode_failed_records: u64,
    pub canonical_event_count: u64,
}

impl DecodeCoverageSummary {
    fn observe(
        &mut self,
        status: ProtocolDecodeStatus,
        event_count: usize,
        announced_server: bool,
        server_clock: bool,
    ) {
        match status {
            ProtocolDecodeStatus::Decoded => {
                self.decoded_records = self.decoded_records.saturating_add(1);
            }
            ProtocolDecodeStatus::CaptureGap => {
                self.capture_gap_records = self.capture_gap_records.saturating_add(1);
            }
            ProtocolDecodeStatus::Unrouted => {
                self.unrouted_records = self.unrouted_records.saturating_add(1);
            }
            ProtocolDecodeStatus::OpaqueLocalOnly => {
                self.opaque_local_only_records = self.opaque_local_only_records.saturating_add(1);
            }
            ProtocolDecodeStatus::Prohibited(_) => {
                self.prohibited_records = self.prohibited_records.saturating_add(1);
            }
            ProtocolDecodeStatus::MissingApplicationPayload => {
                self.missing_application_payload_records =
                    self.missing_application_payload_records.saturating_add(1);
            }
            ProtocolDecodeStatus::DecodeFailed => {
                self.decode_failed_records = self.decode_failed_records.saturating_add(1);
            }
        }
        self.canonical_event_count = self
            .canonical_event_count
            .saturating_add(event_count as u64);
        if announced_server {
            self.announced_server_records = self.announced_server_records.saturating_add(1);
        }
        if server_clock {
            self.server_clock_records = self.server_clock_records.saturating_add(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTopicCoverage {
    pub topic: EventTopic,
    pub event_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteRecordingDisposition {
    Allowed,
    Opaque,
    Prohibited,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRecordingCoverage {
    pub route: RouteKey,
    pub service_name: Option<String>,
    pub method_name: Option<String>,
    pub disposition: RouteRecordingDisposition,
    pub decoder: Option<DecoderKind>,
    pub packet_count: u64,
    pub wire_bytes: u64,
    pub application_bytes: u64,
    pub first_record_sequence: u64,
    pub last_record_sequence: u64,
    pub decode: DecodeCoverageSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureRecordingCoverage {
    pub feature: ProtocolFeature,
    pub route_count: u64,
    pub packet_count: u64,
    pub allowed_packet_count: u64,
    pub opaque_packet_count: u64,
    pub prohibited_packet_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapRecordingCoverage {
    pub kind: CaptureGapKind,
    pub count: u64,
}

#[derive(Debug, Error)]
pub enum OfflineRecordingError {
    #[error(transparent)]
    Capture(#[from] CaptureError),

    #[error(transparent)]
    Protocol(#[from] ProtocolRuntimeError),

    #[error(transparent)]
    Rlog(#[from] RlogError),

    #[error(transparent)]
    Journal(#[from] JsonlJournalError),

    #[error("offline capture contains no packet frames")]
    EmptyCapture,

    #[error("protocol journal contains no capture records")]
    EmptyJournal,

    #[error("protocol journal build does not match the requested decoder build")]
    JournalBuildMismatch,

    #[error("protocol journal was recorded with another protocol-pack digest")]
    JournalProtocolPackMismatch,

    #[error("protocol journal does not declare its source protocol-pack digest")]
    JournalProtocolPackDigestMissing,

    #[error("unsafe protocol-pack journal transition: {0}")]
    UnsafeJournalProtocolPackTransition(String),

    #[error("offline recording limits must be greater than zero")]
    InvalidLimits,

    #[error("offline capture exceeds the {maximum}-frame recording limit")]
    FrameLimitExceeded { maximum: u64 },

    #[error("BPSR framing exceeds the {maximum}-record recording limit")]
    RecordLimitExceeded { maximum: u64 },

    #[error("capture contains more than {maximum} distinct routed messages")]
    UniqueRouteLimitExceeded { maximum: usize },

    #[error("capture record sequence space is exhausted")]
    RecordSequenceExhausted,

    #[error("capture record time moved backward from {previous}us to {next}us")]
    RecordTimeMovedBackward { previous: u64, next: u64 },
}

pub fn record_offline_capture<S, W>(
    source: S,
    connections: GameConnectionFilter,
    pack: &ProtocolPack,
    config: OfflineRecordingConfig,
    output: W,
) -> Result<OfflineRecordingResult<W>, OfflineRecordingError>
where
    S: CaptureSource,
    W: Write,
{
    validate_limits(config.limits)?;
    let mut capture = ValidatedCapture::new(source);
    let first = capture
        .next_frame()?
        .ok_or(OfflineRecordingError::EmptyCapture)?;
    let objective_catalog = config.objective_catalog.clone();
    let mut runtime = ProtocolRuntime::new(
        pack,
        config.session_id.clone(),
        &config.build,
        config.region,
        config.region_evidence,
        config.decoder,
    )?;
    if let Some(objective_catalog) = objective_catalog {
        runtime = runtime.with_objective_catalog(objective_catalog);
    }
    let header = RlogHeader::new(
        config.session_id.clone(),
        runtime.region_context().clone(),
        config.producer,
    );
    let mut writer = RlogWriter::new(output, header)?;
    let mut pipeline = ResearchPipeline::new(connections);
    let mut state = RecordingState::default();

    process_frame(
        &mut pipeline,
        &mut runtime,
        &mut writer,
        &mut state,
        config.limits,
        first,
    )?;
    while let Some(frame) = capture.next_frame()? {
        process_frame(
            &mut pipeline,
            &mut runtime,
            &mut writer,
            &mut state,
            config.limits,
            frame,
        )?;
    }
    append_pipeline_records(
        &mut runtime,
        &mut writer,
        &mut state,
        config.limits,
        |emit| pipeline.finish(emit),
    )?;

    let source = capture.metadata().clone();
    let (output, seal) = writer.finish_with_seal()?;
    let report = build_report(config.session_id, source, pack, None, state, seal);
    Ok(OfflineRecordingResult { output, report })
}

/// Replays an already framed research journal through the same protocol runtime and
/// coverage accounting used by offline packet captures.
///
/// This avoids a second packet-framing implementation while still producing the
/// exact per-route decode report required for protocol-pack promotion. The report
/// intentionally records zero source frames because a protocol journal starts
/// after packet capture, TCP reassembly, and BPSR framing.
pub fn record_offline_journal<R, W>(
    reader: R,
    pack: &ProtocolPack,
    config: OfflineRecordingConfig,
    output: W,
) -> Result<OfflineRecordingResult<W>, OfflineRecordingError>
where
    R: BufRead,
    W: Write,
{
    record_offline_journal_with_tail_policy(reader, pack, config, output, JournalTailPolicy::Strict)
}

pub fn record_offline_journal_with_tail_policy<R, W>(
    reader: R,
    pack: &ProtocolPack,
    config: OfflineRecordingConfig,
    output: W,
    tail_policy: JournalTailPolicy,
) -> Result<OfflineRecordingResult<W>, OfflineRecordingError>
where
    R: BufRead,
    W: Write,
{
    record_offline_journal_with_digest_policy(
        reader,
        pack,
        JournalDigestPolicy::AllowMissing(pack.digest()),
        None,
        config,
        output,
        tail_policy,
    )
}

/// Replays a journal recorded under `source_pack` with `destination_pack` only
/// when the destination is a fail-closed, monotonic demotion of the source.
///
/// This path requires the journal header to contain the exact source-pack
/// digest. It permits unchanged routes and `allowed` to `opaque` demotions;
/// route additions, removals, decoder activations, and semantic changes fail.
pub fn record_offline_journal_transition_with_tail_policy<R, W>(
    reader: R,
    source_pack: &ProtocolPack,
    destination_pack: &ProtocolPack,
    config: OfflineRecordingConfig,
    output: W,
    tail_policy: JournalTailPolicy,
) -> Result<OfflineRecordingResult<W>, OfflineRecordingError>
where
    R: BufRead,
    W: Write,
{
    let validation = validate_monotonic_pack_transition(source_pack, destination_pack)?;
    let transition = ProtocolPackTransitionRecording {
        policy: "monotonic_allowed_to_opaque_only".to_owned(),
        source_protocol_pack_id: source_pack.definition().pack_id.clone(),
        source_protocol_pack_digest: source_pack.digest().to_owned(),
        destination_protocol_pack_id: destination_pack.definition().pack_id.clone(),
        destination_protocol_pack_digest: destination_pack.digest().to_owned(),
        demoted_route_count: validation.demoted_route_count,
    };
    record_offline_journal_with_digest_policy(
        reader,
        destination_pack,
        JournalDigestPolicy::Require(source_pack.digest()),
        Some(transition),
        config,
        output,
        tail_policy,
    )
}

#[derive(Debug, Clone, Copy)]
enum JournalDigestPolicy<'a> {
    AllowMissing(&'a str),
    Require(&'a str),
}

fn record_offline_journal_with_digest_policy<R, W>(
    reader: R,
    runtime_pack: &ProtocolPack,
    journal_digest_policy: JournalDigestPolicy<'_>,
    transition: Option<ProtocolPackTransitionRecording>,
    config: OfflineRecordingConfig,
    output: W,
    tail_policy: JournalTailPolicy,
) -> Result<OfflineRecordingResult<W>, OfflineRecordingError>
where
    R: BufRead,
    W: Write,
{
    validate_limits(config.limits)?;
    let mut stream = JsonlJournalReader::new(reader).into_record_stream()?;
    let capture_session = stream.session().clone();
    if capture_session.game_build != config.build {
        return Err(OfflineRecordingError::JournalBuildMismatch);
    }
    match (
        journal_digest_policy,
        capture_session.protocol_pack_digest.as_deref(),
    ) {
        (JournalDigestPolicy::AllowMissing(expected), Some(actual))
        | (JournalDigestPolicy::Require(expected), Some(actual))
            if actual != expected =>
        {
            return Err(OfflineRecordingError::JournalProtocolPackMismatch);
        }
        (JournalDigestPolicy::Require(_), None) => {
            return Err(OfflineRecordingError::JournalProtocolPackDigestMissing);
        }
        _ => {}
    }

    let objective_catalog = config.objective_catalog.clone();
    let mut runtime = ProtocolRuntime::new(
        runtime_pack,
        config.session_id.clone(),
        &config.build,
        config.region,
        config.region_evidence,
        config.decoder,
    )?;
    if let Some(objective_catalog) = objective_catalog {
        runtime = runtime.with_objective_catalog(objective_catalog);
    }
    let header = RlogHeader::new(
        config.session_id.clone(),
        runtime.region_context().clone(),
        config.producer,
    );
    let mut writer = RlogWriter::new(output, header)?;
    let mut state = RecordingState::default();

    loop {
        let record = match stream.next_record() {
            Ok(Some(record)) => record,
            Ok(None) => break,
            Err(JsonlJournalError::InvalidJson { line, source })
                if tail_policy == JournalTailPolicy::RecoverTruncatedFinalLine
                    && source.is_eof()
                    && stream
                        .truncated_tail()
                        .is_some_and(|(tail_line, _, _)| tail_line == line) =>
            {
                let (_, truncated_bytes, observed_micros) =
                    stream.truncated_tail().expect("tail was checked above");
                append_record(
                    &mut runtime,
                    &mut writer,
                    &mut state,
                    config.limits,
                    CaptureRecordDraft {
                        observed_micros,
                        wall_clock_unix_micros: None,
                        kind: CaptureRecordKind::Gap(CaptureGap {
                            kind: CaptureGapKind::MalformedFrame,
                            connection_id: None,
                            stream_id: None,
                            lost_bytes: Some(truncated_bytes as u64),
                            detail: format!(
                                "protocol journal ended during JSONL record {line}; valid prefix retained"
                            ),
                        }),
                    },
                )?;
                break;
            }
            Err(error) => return Err(error.into()),
        };
        append_record(
            &mut runtime,
            &mut writer,
            &mut state,
            config.limits,
            CaptureRecordDraft {
                observed_micros: record.observed_micros,
                wall_clock_unix_micros: record.wall_clock_unix_micros,
                kind: record.kind,
            },
        )?;
    }
    if state.record_count == 0 {
        return Err(OfflineRecordingError::EmptyJournal);
    }

    let source = CaptureSourceMetadata {
        source_id: capture_session.capture_id,
        display_name: "BPSR protocol journal replay".to_owned(),
        kind: CaptureSourceKind::Replay,
        link_types: Vec::new(),
        file_format: Some(CaptureFileFormat::RlogsEvidence),
    };
    let (output, seal) = writer.finish_with_seal()?;
    let report = build_report(
        config.session_id,
        source,
        runtime_pack,
        transition,
        state,
        seal,
    );
    Ok(OfflineRecordingResult { output, report })
}

#[derive(Debug, Clone, Copy)]
struct PackTransitionValidation {
    demoted_route_count: usize,
}

fn validate_monotonic_pack_transition(
    source: &ProtocolPack,
    destination: &ProtocolPack,
) -> Result<PackTransitionValidation, OfflineRecordingError> {
    let source_definition = source.definition();
    let destination_definition = destination.definition();
    let unsafe_transition =
        |reason: String| OfflineRecordingError::UnsafeJournalProtocolPackTransition(reason);

    if source_definition.schema_version != destination_definition.schema_version {
        return Err(unsafe_transition("schema version changed".to_owned()));
    }
    if source_definition.target != destination_definition.target {
        return Err(unsafe_transition("exact build target changed".to_owned()));
    }
    if !equivalent_provenance(
        &source_definition.provenance,
        &destination_definition.provenance,
    ) {
        return Err(unsafe_transition(
            "pack provenance changed beyond path-separator normalization".to_owned(),
        ));
    }
    if source_definition.routes.len() != destination_definition.routes.len() {
        return Err(unsafe_transition("route count changed".to_owned()));
    }

    let mut demoted_route_count = 0usize;
    for source_route in &source_definition.routes {
        let Some(destination_route) = destination.route(&source_route.route) else {
            return Err(unsafe_transition(format!(
                "route removed: {:?}",
                source_route.route
            )));
        };
        if source_route.service_name != destination_route.service_name
            || source_route.method_name != destination_route.method_name
            || source_route.message_name != destination_route.message_name
            || source_route.confidence != destination_route.confidence
            || source_route.features != destination_route.features
        {
            return Err(unsafe_transition(format!(
                "route semantics changed: {:?}",
                source_route.route
            )));
        }
        if !equivalent_provenance(&source_route.provenance, &destination_route.provenance) {
            return Err(unsafe_transition(format!(
                "route provenance changed beyond path-separator normalization: {:?}",
                source_route.route
            )));
        }
        match (source_route.disposition, destination_route.disposition) {
            (source_disposition, destination_disposition)
                if source_disposition == destination_disposition => {}
            (
                ProtocolPackRouteDisposition::Allowed { .. },
                ProtocolPackRouteDisposition::Opaque,
            ) => {
                demoted_route_count = demoted_route_count.saturating_add(1);
            }
            (source_disposition, destination_disposition) => {
                return Err(unsafe_transition(format!(
                    "route disposition changed non-monotonically for {:?}: {source_disposition:?} -> {destination_disposition:?}",
                    source_route.route
                )));
            }
        }
    }

    Ok(PackTransitionValidation {
        demoted_route_count,
    })
}

fn equivalent_provenance(
    source: &[crate::MappingProvenance],
    destination: &[crate::MappingProvenance],
) -> bool {
    source.len() == destination.len()
        && source.iter().zip(destination).all(|(source, destination)| {
            source.source == destination.source
                && source.reference.replace('\\', "/") == destination.reference.replace('\\', "/")
        })
}

fn validate_limits(limits: OfflineRecordingLimits) -> Result<(), OfflineRecordingError> {
    if limits.maximum_frames == 0
        || limits.maximum_records == 0
        || limits.maximum_unique_routes == 0
    {
        return Err(OfflineRecordingError::InvalidLimits);
    }
    Ok(())
}

fn process_frame<W: Write>(
    pipeline: &mut ResearchPipeline,
    runtime: &mut ProtocolRuntime<'_>,
    writer: &mut RlogWriter<W>,
    state: &mut RecordingState,
    limits: OfflineRecordingLimits,
    frame: CapturedFrame,
) -> Result<(), OfflineRecordingError> {
    if state.frame_count >= limits.maximum_frames {
        return Err(OfflineRecordingError::FrameLimitExceeded {
            maximum: limits.maximum_frames,
        });
    }
    state.frame_count = state.frame_count.saturating_add(1);
    append_pipeline_records(runtime, writer, state, limits, |emit| {
        pipeline.process_frame(&frame, emit);
    })
}

fn append_pipeline_records<W: Write>(
    runtime: &mut ProtocolRuntime<'_>,
    writer: &mut RlogWriter<W>,
    state: &mut RecordingState,
    limits: OfflineRecordingLimits,
    process: impl FnOnce(&mut dyn FnMut(CaptureRecordDraft)),
) -> Result<(), OfflineRecordingError> {
    let mut error = None;
    let mut emit = |draft| {
        if error.is_some() {
            return;
        }
        if let Err(source) = append_record(runtime, writer, state, limits, draft) {
            error = Some(source);
        }
    };
    process(&mut emit);
    error.map_or(Ok(()), Err)
}

fn append_record<W: Write>(
    runtime: &mut ProtocolRuntime<'_>,
    writer: &mut RlogWriter<W>,
    state: &mut RecordingState,
    limits: OfflineRecordingLimits,
    draft: CaptureRecordDraft,
) -> Result<(), OfflineRecordingError> {
    if state.record_count >= limits.maximum_records {
        return Err(OfflineRecordingError::RecordLimitExceeded {
            maximum: limits.maximum_records,
        });
    }
    if let Some(previous) = state.previous_record_micros
        && draft.observed_micros < previous
    {
        return Err(OfflineRecordingError::RecordTimeMovedBackward {
            previous,
            next: draft.observed_micros,
        });
    }
    let sequence = state
        .record_count
        .checked_add(1)
        .ok_or(OfflineRecordingError::RecordSequenceExhausted)?;
    let record = CaptureRecord {
        sequence,
        observed_micros: draft.observed_micros,
        wall_clock_unix_micros: draft.wall_clock_unix_micros,
        kind: draft.kind,
    };
    if let CaptureRecordKind::Packet(packet) = &record.kind
        && let Some(route) = packet.route.map(|routed| routed.key)
        && !state.capture_coverage.routes().contains_key(&route)
        && state.capture_coverage.routes().len() >= limits.maximum_unique_routes
    {
        return Err(OfflineRecordingError::UniqueRouteLimitExceeded {
            maximum: limits.maximum_unique_routes,
        });
    }

    state.capture_coverage.observe(&record);
    let route = record_route(&record);
    let batch = runtime.process(&record)?;
    observe_batch(state, route, &batch);
    for event in batch.events {
        *state.event_topics.entry(event.event.topic()).or_default() = state
            .event_topics
            .get(&event.event.topic())
            .copied()
            .unwrap_or_default()
            .saturating_add(1);
        writer.push(&event)?;
    }
    state.record_count = sequence;
    state.previous_record_micros = Some(record.observed_micros);
    Ok(())
}

fn record_route(record: &CaptureRecord) -> Option<RouteKey> {
    let CaptureRecordKind::Packet(packet) = &record.kind else {
        return None;
    };
    packet.route.map(|routed| routed.key)
}

fn observe_batch(state: &mut RecordingState, route: Option<RouteKey>, batch: &ProtocolDecodeBatch) {
    let announced_server = batch.announced_server.is_some();
    let server_clock = batch.server_clock.is_some();
    state.decode.observe(
        batch.status,
        batch.events.len(),
        announced_server,
        server_clock,
    );
    if let Some(route) = route {
        state.route_decode.entry(route).or_default().observe(
            batch.status,
            batch.events.len(),
            announced_server,
            server_clock,
        );
    }
}

#[derive(Default)]
struct RecordingState {
    frame_count: u64,
    record_count: u64,
    previous_record_micros: Option<u64>,
    capture_coverage: CoverageReport,
    decode: DecodeCoverageSummary,
    route_decode: BTreeMap<RouteKey, DecodeCoverageSummary>,
    event_topics: BTreeMap<EventTopic, u64>,
}

fn build_report(
    session_id: String,
    source: CaptureSourceMetadata,
    pack: &ProtocolPack,
    protocol_pack_transition: Option<ProtocolPackTransitionRecording>,
    state: RecordingState,
    seal: RlogSeal,
) -> OfflineRecordingReport {
    let pack_summary = state.capture_coverage.summarize_pack(pack);
    let capture = CaptureCoverageSummary {
        packet_count: state.capture_coverage.packet_count,
        gap_count: state.capture_coverage.gap_count,
        wire_bytes: state.capture_coverage.wire_bytes,
        application_bytes: state.capture_coverage.application_bytes,
        unrouted_packet_count: state.capture_coverage.unrouted_packet_count,
        unclassified_fragment_packet_count: state
            .capture_coverage
            .unclassified_fragment_packet_count,
        known_route_count: pack_summary.routes.known_routes,
        unknown_route_count: pack_summary.routes.unknown_routes,
        known_packet_count: pack_summary.routes.known_packets,
        unknown_packet_count: pack_summary.routes.unknown_packets,
        allowed_packet_count: pack_summary.allowed_packets,
        opaque_packet_count: pack_summary.opaque_packets,
        prohibited_packet_count: pack_summary.prohibited_packets,
    };
    let routes = state
        .capture_coverage
        .routes()
        .iter()
        .map(|(route, coverage)| {
            let mapping = pack.route(route);
            let (disposition, decoder) = mapping.map_or(
                (RouteRecordingDisposition::Unknown, None),
                |mapping| match mapping.disposition {
                    ProtocolPackRouteDisposition::Allowed { decoder, .. } => {
                        (RouteRecordingDisposition::Allowed, Some(decoder))
                    }
                    ProtocolPackRouteDisposition::Opaque => {
                        (RouteRecordingDisposition::Opaque, None)
                    }
                    ProtocolPackRouteDisposition::Prohibited { .. } => {
                        (RouteRecordingDisposition::Prohibited, None)
                    }
                },
            );
            RouteRecordingCoverage {
                route: *route,
                service_name: mapping.map(|mapping| mapping.service_name.clone()),
                method_name: mapping.map(|mapping| mapping.method_name.clone()),
                disposition,
                decoder,
                packet_count: coverage.packet_count,
                wire_bytes: coverage.wire_bytes,
                application_bytes: coverage.application_bytes,
                first_record_sequence: coverage.first_sequence,
                last_record_sequence: coverage.last_sequence,
                decode: state.route_decode.get(route).copied().unwrap_or_default(),
            }
        })
        .collect();
    let features = pack_summary
        .features
        .into_iter()
        .map(|(feature, coverage)| FeatureRecordingCoverage {
            feature,
            route_count: coverage.route_count,
            packet_count: coverage.packet_count,
            allowed_packet_count: coverage.allowed_packets,
            opaque_packet_count: coverage.opaque_packets,
            prohibited_packet_count: coverage.prohibited_packets,
        })
        .collect();
    let gaps = state
        .capture_coverage
        .gaps()
        .iter()
        .map(|(kind, count)| GapRecordingCoverage {
            kind: *kind,
            count: *count,
        })
        .collect();
    let event_topics = state
        .event_topics
        .into_iter()
        .map(|(topic, event_count)| EventTopicCoverage { topic, event_count })
        .collect();

    OfflineRecordingReport {
        schema_version: OFFLINE_RECORDING_REPORT_SCHEMA_VERSION,
        session_id,
        source,
        protocol_pack_id: pack.definition().pack_id.clone(),
        protocol_pack_digest: pack.digest().to_owned(),
        protocol_pack_transition,
        frame_count: state.frame_count,
        record_count: state.record_count,
        rlog: seal,
        capture,
        decoder: state.decode,
        event_topics,
        routes,
        features,
        gaps,
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    use crate::{
        AllowedDataDomain, DecoderKind, FragmentKind, MappingConfidence, MappingProvenance,
        PacketDirection, ProtocolPackDefinition, ProtocolPackRoute, ProtocolPackTarget,
    };

    fn pack(pack_id: &str, disposition: ProtocolPackRouteDisposition) -> ProtocolPack {
        ProtocolPack::build(ProtocolPackDefinition {
            schema_version: 1,
            pack_id: pack_id.to_owned(),
            target: ProtocolPackTarget {
                deployment_id: "global".to_owned(),
                region_id: None,
                channel: "steam".to_owned(),
                build_id: "24687926".to_owned(),
                executable_version: None,
            },
            acquisition: Default::default(),
            provenance: vec![MappingProvenance {
                source: "test".to_owned(),
                reference: if pack_id.ends_with("v2") {
                    "research\\route.json".to_owned()
                } else {
                    "research/route.json".to_owned()
                },
            }],
            routes: vec![ProtocolPackRoute {
                route: RouteKey::new(
                    PacketDirection::ClientToServer,
                    FragmentKind::Call,
                    103_198_054,
                    0x3D002,
                ),
                service_name: "World".to_owned(),
                method_name: "UseSlot".to_owned(),
                message_name: Some("UseSlotReq".to_owned()),
                confidence: MappingConfidence::Candidate,
                provenance: vec![MappingProvenance {
                    source: "test".to_owned(),
                    reference: if pack_id.ends_with("v2") {
                        "research\\use-slot.json".to_owned()
                    } else {
                        "research/use-slot.json".to_owned()
                    },
                }],
                features: vec![ProtocolFeature::Skill],
                disposition,
            }],
        })
        .expect("test pack must be valid")
    }

    #[test]
    fn monotonic_transition_allows_only_allowed_to_opaque_demotion() {
        let source = pack(
            "candidate-v2",
            ProtocolPackRouteDisposition::Allowed {
                domain: AllowedDataDomain::Combat,
                decoder: DecoderKind::WorldUseSlotV1,
            },
        );
        let destination = pack("candidate-v3", ProtocolPackRouteDisposition::Opaque);

        let validation = validate_monotonic_pack_transition(&source, &destination)
            .expect("fail-closed demotion must be replay-safe");
        assert_eq!(validation.demoted_route_count, 1);
    }

    #[test]
    fn monotonic_transition_rejects_decoder_activation() {
        let source = pack("candidate-v2", ProtocolPackRouteDisposition::Opaque);
        let destination = pack(
            "candidate-v3",
            ProtocolPackRouteDisposition::Allowed {
                domain: AllowedDataDomain::Combat,
                decoder: DecoderKind::WorldUseSlotV1,
            },
        );

        assert!(matches!(
            validate_monotonic_pack_transition(&source, &destination),
            Err(OfflineRecordingError::UnsafeJournalProtocolPackTransition(
                _
            ))
        ));
    }
}
