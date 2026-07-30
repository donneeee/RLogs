use std::collections::BTreeMap;
use std::io::Write;

use rlogs_capture::{
    CaptureError, CaptureSource, CaptureSourceMetadata, CapturedFrame, ValidatedCapture,
};
use rlogs_core::GameConnectionFilter;
use rlogs_events::{EventTopic, RegionEvidence, RegionIdentity};
use rlogs_log_format::{RlogError, RlogHeader, RlogSeal, RlogWriter};
use serde::Serialize;
use thiserror::Error;

use crate::{
    CaptureGapKind, CaptureRecord, CaptureRecordDraft, CaptureRecordKind, CoverageReport,
    DecoderKind, GameBuild, ProtocolDecodeBatch, ProtocolDecodeStatus, ProtocolFeature,
    ProtocolPack, ProtocolPackRouteDisposition, ProtocolRuntime, ProtocolRuntimeConfig,
    ProtocolRuntimeError, ResearchPipeline, RouteKey,
};

pub const OFFLINE_RECORDING_REPORT_SCHEMA_VERSION: u16 = 3;

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
}

pub struct OfflineRecordingResult<W> {
    pub output: W,
    pub report: OfflineRecordingReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfflineRecordingReport {
    pub schema_version: u16,
    pub session_id: String,
    pub source: CaptureSourceMetadata,
    pub protocol_pack_id: String,
    pub protocol_pack_digest: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EventTopicCoverage {
    pub topic: EventTopic,
    pub event_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteRecordingDisposition {
    Allowed,
    Opaque,
    Prohibited,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FeatureRecordingCoverage {
    pub feature: ProtocolFeature,
    pub route_count: u64,
    pub packet_count: u64,
    pub allowed_packet_count: u64,
    pub opaque_packet_count: u64,
    pub prohibited_packet_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

    #[error("offline capture contains no packet frames")]
    EmptyCapture,

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
    let mut runtime = ProtocolRuntime::new(
        pack,
        config.session_id.clone(),
        &config.build,
        config.region,
        config.region_evidence,
        config.decoder,
    )?;
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
    let report = build_report(config.session_id, source, pack, state, seal);
    Ok(OfflineRecordingResult { output, report })
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
