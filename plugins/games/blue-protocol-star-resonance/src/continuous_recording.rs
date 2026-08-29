//! Continuous, process-owned BPSR decoding with selective run persistence.

use std::collections::{BTreeSet, HashSet};
use std::fs::{File, OpenOptions};
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};

use rlogs_capture::CapturedFrame;
use rlogs_core::{ConnectionFilterError, GameConnection, GameConnectionFilter};
use rlogs_events::{
    CanonicalEvent, DungeonEventKind, EventEnvelope, RegionEvidence, RegionIdentity,
    TimelineEventKind,
};
use thiserror::Error;

use crate::{
    BpsrFramerSetConfig, BpsrFramerSetConfigError, CaptureAdapter, CaptureRecord,
    CaptureRecordDraft, CaptureRecordKind, CaptureSession, GameBuild, JsonlJournalError,
    JsonlJournalWriter, ProtocolPack, ProtocolRuntime, ProtocolRuntimeConfig, ProtocolRuntimeError,
    ResearchPipeline, RouteKey, SealedDungeonRunLog, SegmentedDungeonLogWriter,
    SegmentedRecordingError,
};

#[derive(Debug, Clone)]
pub struct ContinuousResearchJournalConfig {
    pub path: PathBuf,
    /// Only these exact gameplay routes are retained. Gaps are always retained;
    /// unknown and prohibited login/account routes must never be added here.
    pub retained_routes: BTreeSet<RouteKey>,
}

#[derive(Debug, Clone)]
pub struct ContinuousRecordingConfig {
    pub base_session_id: String,
    pub producer: String,
    pub build: GameBuild,
    pub region: RegionIdentity,
    pub region_evidence: Vec<RegionEvidence>,
    pub decoder: ProtocolRuntimeConfig,
    pub output_directory: PathBuf,
    pub persist_dungeon_logs: bool,
    pub research_journal: Option<ContinuousResearchJournalConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContinuousRecordingMetrics {
    pub frame_count: u64,
    pub record_count: u64,
    pub decoded_event_count: u64,
    pub saved_event_count: u64,
    pub completed_run_count: u64,
    pub incomplete_run_count: u64,
    pub connection_count: usize,
    pub research_record_count: u64,
}

/// Keeps BPSR protocol state hot for the lifetime of one game process.
///
/// All process-owned frames are decoded in memory. The segmented writer opens
/// only for authoritative dungeon entry/start events, so pre-world and
/// between-run events are never written into dungeon logs.
pub struct ContinuousBpsrRecorder<'a> {
    runtime: ProtocolRuntime<'a>,
    pipeline: Option<ResearchPipeline>,
    framing: BpsrFramerSetConfig,
    connections: HashSet<GameConnection>,
    segments: Option<AsyncSegmentedDungeonLogWriter>,
    research_journal: Option<ResearchJournal>,
    next_record_sequence: u64,
    previous_record_micros: Option<u64>,
    metrics: ContinuousRecordingMetrics,
}

impl<'a> ContinuousBpsrRecorder<'a> {
    pub fn new(
        pack: &'a ProtocolPack,
        config: ContinuousRecordingConfig,
    ) -> Result<Self, ContinuousRecordingError> {
        let mut framing = BpsrFramerSetConfig::default();
        framing.stream.frame_up_layout = pack.definition().acquisition.frame_up_layout;
        let runtime = ProtocolRuntime::new(
            pack,
            config.base_session_id.clone(),
            &config.build,
            config.region,
            config.region_evidence,
            config.decoder,
        )?;
        let segments = config
            .persist_dungeon_logs
            .then(|| {
                SegmentedDungeonLogWriter::new(
                    &config.output_directory,
                    &config.base_session_id,
                    &config.producer,
                )
            })
            .transpose()?
            .map(AsyncSegmentedDungeonLogWriter::new)
            .transpose()?;
        let research_journal = config
            .research_journal
            .map(|journal| {
                ResearchJournal::new(
                    journal,
                    CaptureSession {
                        format_version: 1,
                        capture_id: config.base_session_id,
                        started_unix_micros: unix_micros(),
                        game_build: config.build,
                        adapter: CaptureAdapter {
                            name: "process-owned-dumpcap".into(),
                            version: None,
                        },
                        protocol_pack_digest: Some(pack.digest().to_owned()),
                    },
                )
            })
            .transpose()?;
        Ok(Self {
            runtime,
            pipeline: None,
            framing,
            connections: HashSet::new(),
            segments,
            research_journal,
            next_record_sequence: 1,
            previous_record_micros: None,
            metrics: ContinuousRecordingMetrics::default(),
        })
    }

    pub fn metrics(&self) -> &ContinuousRecordingMetrics {
        &self.metrics
    }

    pub fn is_saving_run(&self) -> bool {
        self.segments
            .as_ref()
            .is_some_and(AsyncSegmentedDungeonLogWriter::is_recording)
    }

    /// Adds exact sockets already attributed to the monitored game process.
    /// Existing TCP and protocol state is preserved.
    pub fn add_connections(
        &mut self,
        connections: impl IntoIterator<Item = GameConnection>,
    ) -> Result<usize, ContinuousRecordingError> {
        let mut added = 0;
        for connection in connections {
            if self.connections.contains(&connection) {
                continue;
            }
            match &mut self.pipeline {
                Some(pipeline) => {
                    pipeline.try_add_connection(connection)?;
                }
                None => {
                    let filter = GameConnectionFilter::try_new(vec![connection])?;
                    self.pipeline = Some(ResearchPipeline::try_with_framing_config(
                        filter,
                        self.framing,
                    )?);
                }
            }
            self.connections.insert(connection);
            added += 1;
        }
        self.metrics.connection_count = self.connections.len();
        Ok(added)
    }

    pub fn process_frame(
        &mut self,
        frame: CapturedFrame,
    ) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        self.process_frame_with_events(frame, |_| {})
    }

    /// Decodes one frame and exposes canonical events before archival work.
    ///
    /// Live projections can consume borrowed events here without cloning them
    /// or waiting for the current dungeon segment to seal.
    pub fn process_frame_with_events(
        &mut self,
        frame: CapturedFrame,
        mut observe: impl FnMut(&rlogs_events::EventEnvelope),
    ) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        self.metrics.frame_count = self.metrics.frame_count.saturating_add(1);
        let pipeline = self
            .pipeline
            .as_mut()
            .ok_or(ContinuousRecordingError::NoAttributedConnection)?;
        let mut drafts = Vec::new();
        pipeline.process_frame(&frame, |draft| drafts.push(draft));
        self.process_drafts(drafts, &mut observe)
    }

    /// Drains transport/framing state and seals an active run as incomplete.
    /// Normal dungeon completion does not call this; monitoring continues.
    pub fn finish(&mut self) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        let mut sealed = Vec::new();
        if let Some(pipeline) = &mut self.pipeline {
            let mut drafts = Vec::new();
            pipeline.finish(|draft| drafts.push(draft));
            sealed.extend(self.process_drafts(drafts, &mut |_| {})?);
        }
        if let Some(segments) = &mut self.segments {
            let final_segment = segments.finish()?;
            self.observe_sealed(&final_segment);
            sealed.extend(final_segment);
        }
        if let Some(journal) = &mut self.research_journal {
            journal.writer.flush()?;
        }
        Ok(sealed)
    }

    fn process_drafts(
        &mut self,
        drafts: Vec<CaptureRecordDraft>,
        observe: &mut impl FnMut(&rlogs_events::EventEnvelope),
    ) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        let mut sealed = Vec::new();
        for draft in drafts {
            if let Some(previous) = self.previous_record_micros
                && draft.observed_micros < previous
            {
                return Err(ContinuousRecordingError::RecordTimeMovedBackward {
                    previous,
                    next: draft.observed_micros,
                });
            }
            let sequence = self.next_record_sequence;
            self.next_record_sequence = self
                .next_record_sequence
                .checked_add(1)
                .ok_or(ContinuousRecordingError::RecordSequenceExhausted)?;
            let record = CaptureRecord {
                sequence,
                observed_micros: draft.observed_micros,
                wall_clock_unix_micros: draft.wall_clock_unix_micros,
                kind: draft.kind,
            };
            if let Some(journal) = &mut self.research_journal
                && journal.retains(&record)
            {
                journal.writer.append_record(&record)?;
                self.metrics.research_record_count =
                    self.metrics.research_record_count.saturating_add(1);
            }
            let batch = self.runtime.process(&record)?;
            self.metrics.record_count = self.metrics.record_count.saturating_add(1);
            self.metrics.decoded_event_count = self
                .metrics
                .decoded_event_count
                .saturating_add(batch.events.len() as u64);
            for event in &batch.events {
                observe(event);
            }
            let newly_sealed = match &mut self.segments {
                Some(segments) => segments.consume_batch(batch.events)?,
                None => Vec::new(),
            };
            self.observe_sealed(&newly_sealed);
            sealed.extend(newly_sealed);
            self.previous_record_micros = Some(record.observed_micros);
        }
        Ok(sealed)
    }

    fn observe_sealed(&mut self, logs: &[SealedDungeonRunLog]) {
        for log in logs {
            self.metrics.saved_event_count = self
                .metrics
                .saved_event_count
                .saturating_add(log.seal.event_count);
            if log.is_completed() {
                self.metrics.completed_run_count =
                    self.metrics.completed_run_count.saturating_add(1);
            } else {
                self.metrics.incomplete_run_count =
                    self.metrics.incomplete_run_count.saturating_add(1);
            }
        }
    }
}

struct ResearchJournal {
    writer: JsonlJournalWriter<BufWriter<File>>,
    retained_routes: BTreeSet<RouteKey>,
}

impl ResearchJournal {
    fn new(
        config: ContinuousResearchJournalConfig,
        session: CaptureSession,
    ) -> Result<Self, ContinuousRecordingError> {
        if config.retained_routes.is_empty() {
            return Err(ContinuousRecordingError::EmptyResearchServiceAllowlist);
        }
        if let Some(parent) = config.path.parent() {
            std::fs::create_dir_all(parent).map_err(ContinuousRecordingError::ResearchJournalIo)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config.path)
            .map_err(ContinuousRecordingError::ResearchJournalIo)?;
        Ok(Self {
            writer: JsonlJournalWriter::new(BufWriter::new(file), session)?,
            retained_routes: config.retained_routes,
        })
    }

    fn retains(&mut self, record: &CaptureRecord) -> bool {
        retains_research_record(record, &self.retained_routes)
    }
}

fn retains_research_record(record: &CaptureRecord, retained_routes: &BTreeSet<RouteKey>) -> bool {
    let CaptureRecordKind::Packet(packet) = &record.kind else {
        return true;
    };
    packet
        .route
        .is_some_and(|route| retained_routes.contains(&route.key))
}

fn unix_micros() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_micros()).ok())
}

const ARCHIVE_QUEUE_BATCH_CAPACITY: usize = 1_024;

struct AsyncSegmentedDungeonLogWriter {
    sender: Option<SyncSender<ArchiveCommand>>,
    results: Receiver<Result<Vec<SealedDungeonRunLog>, SegmentedRecordingError>>,
    saving_run: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

enum ArchiveCommand {
    Batch {
        events: Vec<EventEnvelope>,
        reply: Option<SyncSender<Result<Vec<SealedDungeonRunLog>, SegmentedRecordingError>>>,
    },
    Finish {
        reply: SyncSender<Result<Vec<SealedDungeonRunLog>, SegmentedRecordingError>>,
    },
    Shutdown,
}

impl AsyncSegmentedDungeonLogWriter {
    fn new(writer: SegmentedDungeonLogWriter) -> Result<Self, ContinuousRecordingError> {
        let (sender, commands) = mpsc::sync_channel(ARCHIVE_QUEUE_BATCH_CAPACITY);
        let (result_sender, results) = mpsc::channel();
        let saving_run = Arc::new(AtomicBool::new(false));
        let worker_saving_run = Arc::clone(&saving_run);
        let worker = thread::Builder::new()
            .name("rlogs-bpsr-archive".into())
            .spawn(move || {
                run_archive_worker(writer, commands, result_sender, worker_saving_run);
            })
            .map_err(ContinuousRecordingError::ArchiveWorkerSpawn)?;
        Ok(Self {
            sender: Some(sender),
            results,
            saving_run,
            worker: Some(worker),
        })
    }

    fn is_recording(&self) -> bool {
        self.saving_run.load(Ordering::Acquire)
    }

    fn consume_batch(
        &mut self,
        events: Vec<EventEnvelope>,
    ) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        let mut sealed = self.drain_ready()?;
        let requires_sync = batch_requires_archive_sync(&events);
        if requires_sync {
            let (reply, response) = mpsc::sync_channel(1);
            self.send(ArchiveCommand::Batch {
                events,
                reply: Some(reply),
            })?;
            sealed.extend(
                response
                    .recv()
                    .map_err(|_| ContinuousRecordingError::ArchiveWorkerDisconnected)??,
            );
        } else {
            self.send(ArchiveCommand::Batch {
                events,
                reply: None,
            })?;
        }
        sealed.extend(self.drain_ready()?);
        Ok(sealed)
    }

    fn finish(&mut self) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        let mut sealed = self.drain_ready()?;
        let (reply, response) = mpsc::sync_channel(1);
        self.send(ArchiveCommand::Finish { reply })?;
        self.sender.take();
        sealed.extend(
            response
                .recv()
                .map_err(|_| ContinuousRecordingError::ArchiveWorkerDisconnected)??,
        );
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            return Err(ContinuousRecordingError::ArchiveWorkerPanicked);
        }
        sealed.extend(self.drain_ready()?);
        Ok(sealed)
    }

    fn send(&self, command: ArchiveCommand) -> Result<(), ContinuousRecordingError> {
        self.sender
            .as_ref()
            .ok_or(ContinuousRecordingError::ArchiveWorkerDisconnected)?
            .send(command)
            .map_err(|_| ContinuousRecordingError::ArchiveWorkerDisconnected)
    }

    fn drain_ready(&mut self) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        let mut sealed = Vec::new();
        loop {
            match self.results.try_recv() {
                Ok(result) => sealed.extend(result?),
                Err(TryRecvError::Empty) => return Ok(sealed),
                Err(TryRecvError::Disconnected) if self.sender.is_none() => return Ok(sealed),
                Err(TryRecvError::Disconnected) => {
                    return Err(ContinuousRecordingError::ArchiveWorkerDisconnected);
                }
            }
        }
    }
}

impl Drop for AsyncSegmentedDungeonLogWriter {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(ArchiveCommand::Shutdown);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_archive_worker(
    mut writer: SegmentedDungeonLogWriter,
    commands: Receiver<ArchiveCommand>,
    results: mpsc::Sender<Result<Vec<SealedDungeonRunLog>, SegmentedRecordingError>>,
    saving_run: Arc<AtomicBool>,
) {
    while let Ok(command) = commands.recv() {
        match command {
            ArchiveCommand::Batch { events, reply } => {
                let result = writer.consume_batch(events);
                saving_run.store(writer.is_recording(), Ordering::Release);
                let failed = result.is_err();
                match reply {
                    Some(reply) => {
                        if reply.send(result).is_err() {
                            break;
                        }
                    }
                    None => match result {
                        Ok(logs) if logs.is_empty() => {}
                        result => {
                            if results.send(result).is_err() {
                                break;
                            }
                        }
                    },
                }
                if failed {
                    break;
                }
            }
            ArchiveCommand::Finish { reply } => {
                let result = writer.finish();
                saving_run.store(false, Ordering::Release);
                let _ = reply.send(result);
                break;
            }
            ArchiveCommand::Shutdown => break,
        }
    }
    saving_run.store(false, Ordering::Release);
}

fn batch_requires_archive_sync(events: &[EventEnvelope]) -> bool {
    events.iter().any(|event| match &event.event {
        CanonicalEvent::Dungeon(dungeon) => matches!(
            dungeon.kind,
            DungeonEventKind::Entered
                | DungeonEventKind::Started
                | DungeonEventKind::Completed
                | DungeonEventKind::Failed
                | DungeonEventKind::Exited
        ),
        CanonicalEvent::Timeline(timeline) => {
            matches!(timeline.kind, TimelineEventKind::RunBoundary { .. })
        }
        _ => false,
    })
}

#[derive(Debug, Error)]
pub enum ContinuousRecordingError {
    #[error("a process-owned frame arrived before its exact game connection was registered")]
    NoAttributedConnection,

    #[error("continuous capture record sequence space is exhausted")]
    RecordSequenceExhausted,

    #[error("continuous capture time moved backward from {previous}us to {next}us")]
    RecordTimeMovedBackward { previous: u64, next: u64 },

    #[error("could not start the bounded BPSR archive worker: {0}")]
    ArchiveWorkerSpawn(std::io::Error),

    #[error("the bounded BPSR archive worker disconnected")]
    ArchiveWorkerDisconnected,

    #[error("the bounded BPSR archive worker panicked")]
    ArchiveWorkerPanicked,

    #[error("research journal route allowlist cannot be empty")]
    EmptyResearchServiceAllowlist,

    #[error("could not create or write the research journal: {0}")]
    ResearchJournalIo(std::io::Error),

    #[error(transparent)]
    Connection(#[from] ConnectionFilterError),

    #[error(transparent)]
    Framing(#[from] BpsrFramerSetConfigError),

    #[error(transparent)]
    Protocol(#[from] ProtocolRuntimeError),

    #[error(transparent)]
    Journal(#[from] JsonlJournalError),

    #[error(transparent)]
    Segmented(#[from] SegmentedRecordingError),
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::net::{IpAddr, Ipv4Addr};

    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use prost::Message;
    use rlogs_capture::{CaptureLinkType, TimestampNormalization};
    use rlogs_events::{
        CanonicalEventDraft, CanonicalEventDraftKind, DungeonEvent, EventEnvelopeFactory,
        EventProvenance, EventSensitivity, EventTime, RegionContext, TimelineEventKind,
    };
    use rlogs_network::IpEndpoint;

    use crate::game_schema_v1 as schema;

    use super::*;

    fn packet_record(
        connection_id: u64,
        direction: crate::PacketDirection,
        fragment: crate::FragmentKind,
        service_id: Option<u64>,
    ) -> CaptureRecord {
        CaptureRecord {
            sequence: 1,
            observed_micros: 1,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(crate::PacketEnvelope {
                connection_id,
                stream_id: 1,
                source: None,
                destination: None,
                direction,
                fragment: Some(fragment),
                route: service_id.map(|service_id| crate::RoutedMessage {
                    key: crate::RouteKey::new(direction, fragment, service_id, 1),
                    stub_id: 0,
                    call_id: None,
                }),
                compression: crate::CompressionState::NotCompressed,
                payload: crate::PacketPayload {
                    wire_bytes: vec![1, 2, 3],
                    application_bytes: Some(vec![2, 3]),
                },
            }),
        }
    }

    fn endpoint(last: u8, port: u16) -> IpEndpoint {
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    fn bpsr_frame(fragment: u16, payload: &[u8]) -> Vec<u8> {
        let length = 6 + payload.len();
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(&(length as u32).to_be_bytes());
        bytes.extend_from_slice(&fragment.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn routed_payload(
        service_id: u64,
        method_id: u32,
        call_id: Option<u32>,
        body: &[u8],
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&service_id.to_be_bytes());
        payload.extend_from_slice(&0_u32.to_be_bytes());
        if let Some(call_id) = call_id {
            payload.extend_from_slice(&call_id.to_be_bytes());
        }
        payload.extend_from_slice(&method_id.to_be_bytes());
        payload.extend_from_slice(body);
        payload
    }

    fn nested_frame_up(service_id: u64, method_id: u32, call_id: u32, body: &[u8]) -> Vec<u8> {
        let nested = bpsr_frame(
            1,
            &routed_payload(service_id, method_id, Some(call_id), body),
        );
        let mut payload = Vec::with_capacity(4 + nested.len());
        payload.extend_from_slice(&1_u32.to_be_bytes());
        payload.extend_from_slice(&nested);
        bpsr_frame(5, &payload)
    }

    fn captured_frame(
        sequence: u64,
        observed_micros: u64,
        source: IpEndpoint,
        destination: IpEndpoint,
        tcp_sequence: u32,
        payload: &[u8],
    ) -> CapturedFrame {
        let tcp = TcpHeader::new(source.port, destination.port, tcp_sequence, 16_384);
        let builder = PacketBuilder::ipv4(
            match source.address {
                IpAddr::V4(address) => address.octets(),
                IpAddr::V6(_) => unreachable!(),
            },
            match destination.address {
                IpAddr::V4(address) => address.octets(),
                IpAddr::V6(_) => unreachable!(),
            },
            64,
        )
        .tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, payload).unwrap();
        CapturedFrame {
            sequence,
            observed_micros,
            source_timestamp_nanos: Some(1_000_000 + observed_micros as i64 * 1_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: Some(0),
            link_type: CaptureLinkType::RawIpv4,
            original_length: packet.len() as u32,
            bytes: Bytes::from(packet),
        }
    }

    fn route(
        key: RouteKey,
        service_name: &str,
        method_name: &str,
        disposition: crate::ProtocolPackRouteDisposition,
    ) -> crate::ProtocolPackRoute {
        crate::ProtocolPackRoute {
            route: key,
            service_name: service_name.into(),
            method_name: method_name.into(),
            message_name: None,
            confidence: crate::MappingConfidence::Verified,
            provenance: Vec::new(),
            features: vec![crate::ProtocolFeature::Skill],
            disposition,
        }
    }

    #[test]
    fn research_journal_retains_only_exact_routes_and_gaps() {
        let retained_route = crate::RouteKey::new(
            crate::PacketDirection::ServerToClient,
            crate::FragmentKind::Notify,
            10,
            1,
        );
        let retained_routes = BTreeSet::from([retained_route]);
        let exact_gameplay = packet_record(
            7,
            crate::PacketDirection::ServerToClient,
            crate::FragmentKind::Notify,
            Some(10),
        );
        assert!(retains_research_record(&exact_gameplay, &retained_routes));

        let opaque_frame_up = packet_record(
            7,
            crate::PacketDirection::ClientToServer,
            crate::FragmentKind::FrameUp,
            None,
        );
        assert!(!retains_research_record(&opaque_frame_up, &retained_routes));

        let same_service_unknown_route = packet_record(
            7,
            crate::PacketDirection::ClientToServer,
            crate::FragmentKind::Call,
            Some(10),
        );
        assert!(!retains_research_record(
            &same_service_unknown_route,
            &retained_routes,
        ));

        let gap = CaptureRecord {
            sequence: 2,
            observed_micros: 2,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Gap(crate::CaptureGap {
                kind: crate::CaptureGapKind::TcpGap,
                connection_id: Some(7),
                stream_id: Some(1),
                lost_bytes: Some(1),
                detail: "fixture".into(),
            }),
        };
        assert!(retains_research_record(&gap, &retained_routes));
    }

    #[test]
    fn nested_use_slot_is_journaled_and_replays_to_exact_canonical_cast() {
        const WORLD: u64 = 103_198_054;
        const WORLD_NTF: u64 = 1_664_308_034;
        const USE_SLOT: u32 = 249_858;
        const PROHIBITED: u32 = 4_098;
        const UNKNOWN: u32 = 999_999;
        let use_slot_route = RouteKey::new(
            crate::PacketDirection::ClientToServer,
            crate::FragmentKind::Call,
            WORLD,
            USE_SLOT,
        );
        let prohibited_route = RouteKey::new(
            crate::PacketDirection::ClientToServer,
            crate::FragmentKind::Call,
            WORLD,
            PROHIBITED,
        );
        let self_delta_route = RouteKey::new(
            crate::PacketDirection::ServerToClient,
            crate::FragmentKind::Notify,
            WORLD_NTF,
            46,
        );
        let pack = ProtocolPack::build(crate::ProtocolPackDefinition {
            schema_version: crate::PROTOCOL_PACK_SCHEMA_VERSION,
            pack_id: "nested-use-slot-acquisition-fixture".into(),
            target: crate::ProtocolPackTarget {
                deployment_id: "global".into(),
                region_id: None,
                channel: "steam".into(),
                build_id: crate::BPSR_USE_SKILL_ATTR_BUILD.into(),
                executable_version: None,
            },
            acquisition: crate::ProtocolPackAcquisition {
                frame_up_layout: crate::BpsrFrameUpLayout::NestedAfterFourBytes,
            },
            provenance: Vec::new(),
            routes: vec![
                route(
                    self_delta_route,
                    "WorldNtf",
                    "SyncToMeDeltaInfo",
                    crate::ProtocolPackRouteDisposition::Allowed {
                        domain: crate::DecoderKind::SyncToMeDeltaV1.domain(),
                        decoder: crate::DecoderKind::SyncToMeDeltaV1,
                    },
                ),
                route(
                    use_slot_route,
                    "World",
                    "UseSlot",
                    crate::ProtocolPackRouteDisposition::Allowed {
                        domain: crate::DecoderKind::WorldUseSlotV1.domain(),
                        decoder: crate::DecoderKind::WorldUseSlotV1,
                    },
                ),
                route(
                    prohibited_route,
                    "World",
                    "Authenticate",
                    crate::ProtocolPackRouteDisposition::Prohibited {
                        class: crate::ProhibitedDataClass::AuthenticationToken,
                    },
                ),
            ],
        })
        .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "rlogs-nested-use-slot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let journal_path = directory.join("capture.protocol.jsonl");
        let region = RegionIdentity {
            deployment_id: "global".into(),
            region_id: "global".into(),
            realm_id: None,
            world_id: None,
        };
        let build = GameBuild {
            deployment_id: "global".into(),
            region_id: None,
            channel: "steam".into(),
            build_id: crate::BPSR_USE_SKILL_ATTR_BUILD.into(),
            executable_version: None,
        };
        let mut recorder = ContinuousBpsrRecorder::new(
            &pack,
            ContinuousRecordingConfig {
                base_session_id: "nested-use-slot".into(),
                producer: "test".into(),
                build: build.clone(),
                region: region.clone(),
                region_evidence: Vec::new(),
                decoder: ProtocolRuntimeConfig::default(),
                output_directory: directory.clone(),
                persist_dungeon_logs: false,
                research_journal: Some(ContinuousResearchJournalConfig {
                    path: journal_path.clone(),
                    retained_routes: BTreeSet::from([self_delta_route, use_slot_route]),
                }),
            },
        )
        .unwrap();
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        recorder
            .add_connections([GameConnection { client, server }])
            .unwrap();

        let self_delta_body = schema::SyncToMeDeltaInfo {
            delta: Some(schema::AoiSyncToMeDelta {
                base_delta: None,
                hate_ids: Vec::new(),
                cooldowns: Vec::new(),
                fight_resource_cooldowns: Vec::new(),
                uuid: Some(216_009_015_936),
            }),
        }
        .encode_to_vec();
        let self_delta_wire = bpsr_frame(2, &routed_payload(WORLD_NTF, 46, None, &self_delta_body));
        recorder
            .process_frame(captured_frame(
                1,
                100,
                server,
                client,
                100,
                &self_delta_wire,
            ))
            .unwrap();

        let use_slot_wire = nested_frame_up(
            WORLD,
            USE_SLOT,
            1,
            &crate::use_skill_attr::tests::world_skill_use_payload(),
        );
        recorder
            .process_frame(captured_frame(2, 200, client, server, 100, &use_slot_wire))
            .unwrap();
        let prohibited_wire = nested_frame_up(WORLD, PROHIBITED, 2, &[1, 2, 3]);
        recorder
            .process_frame(captured_frame(
                3,
                300,
                client,
                server,
                100 + use_slot_wire.len() as u32,
                &prohibited_wire,
            ))
            .unwrap();
        let unknown_wire = nested_frame_up(WORLD, UNKNOWN, 3, &[4, 5, 6]);
        recorder
            .process_frame(captured_frame(
                4,
                400,
                client,
                server,
                100 + use_slot_wire.len() as u32 + prohibited_wire.len() as u32,
                &unknown_wire,
            ))
            .unwrap();
        recorder.finish().unwrap();
        drop(recorder);

        let journal =
            crate::JsonlJournalReader::new(BufReader::new(File::open(&journal_path).unwrap()))
                .read()
                .unwrap();
        let recorded_routes = journal
            .records()
            .iter()
            .filter_map(|record| match &record.kind {
                CaptureRecordKind::Packet(packet) => packet.route.map(|route| route.key),
                CaptureRecordKind::Gap(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(recorded_routes, vec![self_delta_route, use_slot_route]);
        assert!(!recorded_routes.contains(&prohibited_route));
        assert!(
            !recorded_routes
                .iter()
                .any(|route| route.method_id == UNKNOWN)
        );

        let mut runtime = ProtocolRuntime::new(
            &pack,
            "nested-use-slot-replay",
            &build,
            region,
            Vec::new(),
            ProtocolRuntimeConfig::default(),
        )
        .unwrap();
        let mut cast = None;
        for record in journal.records() {
            for event in runtime.process(record).unwrap().events {
                if let rlogs_events::CanonicalEvent::Timeline(timeline) = event.event
                    && let TimelineEventKind::Cast(observed) = timeline.kind
                {
                    cast = Some(observed);
                }
            }
        }
        let cast = cast.expect("nested World.UseSlot must replay to a canonical cast");
        let timing = cast.action_timing.expect("exact action timing");
        assert_eq!(timing.action_instance_id, 9_001);
        assert_eq!(timing.base_ability.0, 2_233);
        assert_eq!(timing.slot_id, 21);

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bounded_archive_worker_seals_boundary_batches_without_blocking_normal_batches() {
        let directory = std::env::temp_dir().join(format!(
            "rlogs-async-segment-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let writer = SegmentedDungeonLogWriter::new(&directory, "async-test", "unit-test").unwrap();
        let mut writer = AsyncSegmentedDungeonLogWriter::new(writer).unwrap();
        let region = RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "global".into(),
                realm_id: None,
                world_id: None,
            },
            client_build: "fixture".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: Vec::new(),
        };
        let mut envelopes = EventEnvelopeFactory::new("continuous", region);
        let event = |kind| CanonicalEventDraft {
            time: EventTime {
                observed_micros: match kind {
                    DungeonEventKind::Entered => 1_000,
                    _ => 2_000,
                },
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(1, 1, 1),
            sensitivity: EventSensitivity::PublicGameplay,
            kind: CanonicalEventDraftKind::Dungeon(DungeonEvent {
                kind,
                dungeon_id: None,
                instance_id: Some("instance-1".into()),
                difficulty_id: None,
                objective_map_key: None,
                objective_id: None,
                objective_value: None,
                objective_complete: None,
                objective_catalog: None,
                flow: None,
            }),
        };

        assert!(
            writer
                .consume_batch(vec![
                    envelopes.emit(event(DungeonEventKind::Entered)).unwrap()
                ])
                .unwrap()
                .is_empty()
        );
        assert!(writer.is_recording());
        let sealed = writer
            .consume_batch(vec![
                envelopes.emit(event(DungeonEventKind::Completed)).unwrap(),
            ])
            .unwrap();
        assert_eq!(sealed.len(), 1);
        assert!(sealed[0].is_completed());
        assert!(!writer.is_recording());
        assert!(writer.finish().unwrap().is_empty());

        std::fs::remove_file(&sealed[0].path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
}
