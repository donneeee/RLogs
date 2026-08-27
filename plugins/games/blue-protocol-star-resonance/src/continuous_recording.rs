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
    CaptureAdapter, CaptureRecord, CaptureRecordDraft, CaptureRecordKind, CaptureSession,
    GameBuild, JsonlJournalError, JsonlJournalWriter, ProtocolPack, ProtocolRuntime,
    ProtocolRuntimeConfig, ProtocolRuntimeError, ResearchPipeline, SealedDungeonRunLog,
    SegmentedDungeonLogWriter, SegmentedRecordingError,
};

#[derive(Debug, Clone)]
pub struct ContinuousResearchJournalConfig {
    pub path: PathBuf,
    /// Only packet records for these gameplay service IDs are retained. Gaps
    /// are always retained. Login/account services must never be added here.
    pub allowed_service_ids: BTreeSet<u64>,
    /// Retain an unrouted client FrameUp wrapper only after the same TCP
    /// connection has emitted a routed packet for an allowed gameplay service.
    /// This preserves acquisition evidence for nested client calls without
    /// broadening the journal to login/account connections.
    pub retain_opaque_client_frame_up: bool,
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
                    self.pipeline = Some(ResearchPipeline::new(filter));
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
    allowed_service_ids: BTreeSet<u64>,
    retain_opaque_client_frame_up: bool,
    gameplay_connection_ids: BTreeSet<u64>,
}

impl ResearchJournal {
    fn new(
        config: ContinuousResearchJournalConfig,
        session: CaptureSession,
    ) -> Result<Self, ContinuousRecordingError> {
        if config.allowed_service_ids.is_empty() {
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
            allowed_service_ids: config.allowed_service_ids,
            retain_opaque_client_frame_up: config.retain_opaque_client_frame_up,
            gameplay_connection_ids: BTreeSet::new(),
        })
    }

    fn retains(&mut self, record: &CaptureRecord) -> bool {
        retains_research_record(
            record,
            &self.allowed_service_ids,
            self.retain_opaque_client_frame_up,
            &mut self.gameplay_connection_ids,
        )
    }
}

fn retains_research_record(
    record: &CaptureRecord,
    allowed_service_ids: &BTreeSet<u64>,
    retain_opaque_client_frame_up: bool,
    gameplay_connection_ids: &mut BTreeSet<u64>,
) -> bool {
    let CaptureRecordKind::Packet(packet) = &record.kind else {
        return true;
    };
    if packet
        .route
        .is_some_and(|route| allowed_service_ids.contains(&route.key.service_id))
    {
        gameplay_connection_ids.insert(packet.connection_id);
        return true;
    }
    retain_opaque_client_frame_up
        && packet.direction == crate::PacketDirection::ClientToServer
        && packet.fragment == Some(crate::FragmentKind::FrameUp)
        && packet.route.is_none()
        && gameplay_connection_ids.contains(&packet.connection_id)
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

    #[error("research journal service allowlist cannot be empty")]
    EmptyResearchServiceAllowlist,

    #[error("could not create or write the research journal: {0}")]
    ResearchJournalIo(std::io::Error),

    #[error(transparent)]
    Connection(#[from] ConnectionFilterError),

    #[error(transparent)]
    Protocol(#[from] ProtocolRuntimeError),

    #[error(transparent)]
    Journal(#[from] JsonlJournalError),

    #[error(transparent)]
    Segmented(#[from] SegmentedRecordingError),
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        CanonicalEventDraft, CanonicalEventDraftKind, DungeonEvent, EventEnvelopeFactory,
        EventProvenance, EventSensitivity, EventTime, RegionContext,
    };

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

    #[test]
    fn research_journal_retains_opaque_client_frame_up_only_on_a_proven_gameplay_connection() {
        let allowed_service_ids = BTreeSet::from([10]);
        let mut gameplay_connection_ids = BTreeSet::new();

        let before_proof = packet_record(
            7,
            crate::PacketDirection::ClientToServer,
            crate::FragmentKind::FrameUp,
            None,
        );
        assert!(!retains_research_record(
            &before_proof,
            &allowed_service_ids,
            true,
            &mut gameplay_connection_ids,
        ));

        let gameplay_proof = packet_record(
            7,
            crate::PacketDirection::ServerToClient,
            crate::FragmentKind::Notify,
            Some(10),
        );
        assert!(retains_research_record(
            &gameplay_proof,
            &allowed_service_ids,
            true,
            &mut gameplay_connection_ids,
        ));
        assert!(retains_research_record(
            &before_proof,
            &allowed_service_ids,
            true,
            &mut gameplay_connection_ids,
        ));

        let other_connection = packet_record(
            8,
            crate::PacketDirection::ClientToServer,
            crate::FragmentKind::FrameUp,
            None,
        );
        assert!(!retains_research_record(
            &other_connection,
            &allowed_service_ids,
            true,
            &mut gameplay_connection_ids,
        ));

        let disallowed_direct_route = packet_record(
            7,
            crate::PacketDirection::ClientToServer,
            crate::FragmentKind::Call,
            Some(99),
        );
        assert!(!retains_research_record(
            &disallowed_direct_route,
            &allowed_service_ids,
            true,
            &mut gameplay_connection_ids,
        ));
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
