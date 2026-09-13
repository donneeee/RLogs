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
    BoundaryReason, CanonicalEvent, DungeonEventKind, EventEnvelope, EventProvenance,
    EventSensitivity, EventTime, LocalSkillObservationReceipt, RegionContext, RegionEvidence,
    RegionIdentity, RunState, TimelineEvent, TimelineEventKind,
};
use thiserror::Error;

use crate::{
    BpsrFramerSetConfig, BpsrFramerSetConfigError, CaptureAdapter, CaptureRecord,
    CaptureRecordDraft, CaptureRecordKind, CaptureSession, GameBuild, JsonlJournalError,
    JsonlJournalWriter, LocalPhotoAssetReference, ObjectiveCatalogResolver, ProtocolPack,
    ProtocolRuntime, ProtocolRuntimeConfig, ProtocolRuntimeError, ResearchPipeline, RouteKey,
    SealedDungeonRunLog, SegmentedDungeonLogWriter, SegmentedRecordingError,
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
    pub objective_catalog: Option<Arc<dyn ObjectiveCatalogResolver>>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LocalSkillRouteCounters {
    requests: u64,
    decoded: u64,
    failures: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveLocalSkillObservation {
    source: rlogs_events::EntityRef,
    started_micros: u64,
    authoritative_start: bool,
    completion_observed: bool,
    completion_micros: Option<u64>,
    completion_sequence: Option<u64>,
    completion_source_matches: bool,
    counters_at_completion: Option<LocalSkillRouteCounters>,
    queue_saturations_at_completion: Option<u64>,
    data_gaps_at_completion: Option<u64>,
    counters_at_start: LocalSkillRouteCounters,
    queue_saturations_at_start: u64,
    data_gaps: u64,
    skill_event_sequences: Vec<u64>,
}

#[derive(Debug, Default)]
struct LocalSkillObservationTracker {
    counters: LocalSkillRouteCounters,
    queue_saturations: u64,
    active: Option<ActiveLocalSkillObservation>,
}

impl LocalSkillObservationTracker {
    fn observe_protocol(&mut self, local_skill_route: bool, status: crate::ProtocolDecodeStatus) {
        if !local_skill_route {
            return;
        }
        self.counters.requests = self.counters.requests.saturating_add(1);
        match status {
            crate::ProtocolDecodeStatus::Decoded => {
                self.counters.decoded = self.counters.decoded.saturating_add(1);
            }
            crate::ProtocolDecodeStatus::DecodeFailed
            | crate::ProtocolDecodeStatus::MissingApplicationPayload => {
                self.counters.failures = self.counters.failures.saturating_add(1);
            }
            _ => {}
        }
    }

    fn observe_event(
        &mut self,
        event: &EventEnvelope,
        source_before_record: Option<rlogs_events::EntityRef>,
        local_skill_route: bool,
    ) {
        if let CanonicalEvent::Dungeon(dungeon) = &event.event
            && matches!(
                dungeon.kind,
                DungeonEventKind::Entered | DungeonEventKind::Started
            )
            && self.active.is_none()
        {
            if let Some(source) = source_before_record {
                self.active = Some(ActiveLocalSkillObservation {
                    source,
                    started_micros: event.time.observed_micros,
                    authoritative_start: dungeon.kind == DungeonEventKind::Started,
                    completion_observed: false,
                    completion_micros: None,
                    completion_sequence: None,
                    completion_source_matches: false,
                    counters_at_completion: None,
                    queue_saturations_at_completion: None,
                    data_gaps_at_completion: None,
                    counters_at_start: self.counters,
                    queue_saturations_at_start: self.queue_saturations,
                    data_gaps: 0,
                    skill_event_sequences: Vec::new(),
                });
            }
        }
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if !active.completion_observed
            && local_skill_route
            && matches!(
                &event.event,
                CanonicalEvent::Timeline(timeline)
                    if matches!(&timeline.kind, TimelineEventKind::Cast(cast)
                        if cast.state == rlogs_events::CastState::Started
                            && cast.source == active.source)
            )
        {
            active.skill_event_sequences.push(event.sequence);
        }
        if matches!(
            &event.event,
            CanonicalEvent::Dungeon(dungeon) if dungeon.kind == DungeonEventKind::Started
        ) || matches!(
            &event.event,
            CanonicalEvent::Timeline(timeline)
                if matches!(timeline.kind, TimelineEventKind::RunBoundary {
                    state: RunState::Started,
                    reason: BoundaryReason::AuthoritativePacket,
                    ..
                })
        ) {
            active.started_micros = event.time.observed_micros;
            active.authoritative_start = true;
            active.counters_at_start = self.counters;
            active.queue_saturations_at_start = self.queue_saturations;
            active.data_gaps = 0;
            active.skill_event_sequences.clear();
        }
        if matches!(
            &event.event,
            CanonicalEvent::Timeline(timeline)
                if matches!(timeline.kind, TimelineEventKind::DataGap(_))
        ) {
            active.data_gaps = active.data_gaps.saturating_add(1);
        }
        if !active.completion_observed
            && (matches!(
                &event.event,
                CanonicalEvent::Dungeon(dungeon) if dungeon.kind == DungeonEventKind::Completed
            ) || matches!(
                &event.event,
                CanonicalEvent::Timeline(timeline)
                    if matches!(timeline.kind, TimelineEventKind::RunBoundary { state: RunState::Completed, .. })
            ))
        {
            active.completion_observed = true;
            active.completion_micros = Some(event.time.observed_micros);
            active.completion_sequence = Some(event.sequence);
            active.completion_source_matches = source_before_record == Some(active.source);
            active.counters_at_completion = Some(self.counters);
            active.queue_saturations_at_completion = Some(self.queue_saturations);
            active.data_gaps_at_completion = Some(active.data_gaps);
        }
    }

    fn finish(
        &mut self,
        ended_micros: u64,
        region: &RegionContext,
    ) -> Option<(LocalSkillObservationReceipt, u64, Vec<u64>)> {
        let active = self.active.take()?;
        let completion_counters = active.counters_at_completion?;
        let requests = completion_counters
            .requests
            .saturating_sub(active.counters_at_start.requests);
        let decoded = completion_counters
            .decoded
            .saturating_sub(active.counters_at_start.decoded);
        let failures = completion_counters
            .failures
            .saturating_sub(active.counters_at_start.failures);
        let saturations = active
            .queue_saturations_at_completion?
            .saturating_sub(active.queue_saturations_at_start);
        let data_gaps = active.data_gaps_at_completion?;
        if !active.authoritative_start
            || !active.completion_observed
            || !active.completion_source_matches
            || failures != 0
            || requests != decoded
            || usize::try_from(decoded).ok() != Some(active.skill_event_sequences.len())
            || saturations != 0
            || data_gaps != 0
        {
            return None;
        }
        Some((
            LocalSkillObservationReceipt {
                source: active.source,
                route: "world_use_slot_v1".into(),
                deployment_id: region.identity.deployment_id.clone(),
                client_build: region.client_build.clone(),
                protocol_pack_digest: region.protocol_pack_digest.clone(),
                run_started_micros: active.started_micros,
                run_ended_micros: active.completion_micros.unwrap_or(ended_micros),
                request_count: requests,
                decoded_count: decoded,
                decode_failure_count: failures,
                capture_queue_saturation_count: saturations,
                data_gap_count: data_gaps,
                authoritative_start: true,
                authoritative_completion: true,
            },
            active.completion_sequence?,
            active.skill_event_sequences,
        ))
    }
}

#[derive(Debug, Clone)]
struct ManualEventContext {
    schema_version: u16,
    session_id: String,
    sequence: u64,
    region: RegionContext,
    time: EventTime,
    timeline_sequence: u64,
}

#[derive(Debug, Clone)]
pub struct ContinuousForcedReset {
    /// Manual boundary supplied to live reducers before their presentation
    /// state is cleared. `None` means no canonical event had been decoded yet.
    pub boundary: Option<EventEnvelope>,
    pub sealed_logs: Vec<SealedDungeonRunLog>,
    pub invalidated_active_run: bool,
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
    last_event_context: Option<ManualEventContext>,
    metrics: ContinuousRecordingMetrics,
    local_skill_observation: LocalSkillObservationTracker,
}

impl<'a> ContinuousBpsrRecorder<'a> {
    pub fn new(
        pack: &'a ProtocolPack,
        config: ContinuousRecordingConfig,
    ) -> Result<Self, ContinuousRecordingError> {
        let mut framing = BpsrFramerSetConfig::default();
        framing.stream.frame_up_layout = pack.definition().acquisition.frame_up_layout;
        let mut runtime = ProtocolRuntime::new(
            pack,
            config.base_session_id.clone(),
            &config.build,
            config.region,
            config.region_evidence,
            config.decoder,
        )?;
        if let Some(objective_catalog) = config.objective_catalog.clone() {
            runtime = runtime.with_objective_catalog(objective_catalog);
        }
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
                        protocol_pack_authority: None,
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
            last_event_context: None,
            metrics: ContinuousRecordingMetrics::default(),
            local_skill_observation: LocalSkillObservationTracker::default(),
        })
    }

    pub fn metrics(&self) -> &ContinuousRecordingMetrics {
        &self.metrics
    }

    /// Latest packet-resolved session identity. This can advance before a
    /// dungeon segment exists (for example on NotifyEnterWorld.scene_ip), so
    /// live consumers can prepare the eventual log header immediately.
    pub fn region_context(&self) -> &RegionContext {
        self.runtime.region_context()
    }

    pub fn is_saving_run(&self) -> bool {
        self.segments
            .as_ref()
            .is_some_and(AsyncSegmentedDungeonLogWriter::is_recording)
    }

    /// Supplies the monotonic capture-ingress saturation counter. The recorder
    /// snapshots it at run entry and refuses a completeness receipt if it
    /// advances before the terminal boundary.
    pub fn observe_capture_queue_saturations(&mut self, count: u64) {
        self.local_skill_observation.queue_saturations =
            self.local_skill_observation.queue_saturations.max(count);
    }

    /// Seals the active persisted run with an explicit manual boundary. The
    /// resulting incomplete log remains available locally and its reducer
    /// carries `manual_boundary`, which makes server submission ineligible.
    pub fn force_reset(
        &mut self,
        observed_micros: u64,
    ) -> Result<ContinuousForcedReset, ContinuousRecordingError> {
        let invalidated_active_run = self.is_saving_run();
        let Some(context) = self.last_event_context.as_ref() else {
            return Ok(ContinuousForcedReset {
                boundary: None,
                sealed_logs: Vec::new(),
                invalidated_active_run,
            });
        };
        let time = EventTime {
            observed_micros: observed_micros.max(context.time.observed_micros),
            game_time_millis: None,
        };
        let provenance = EventProvenance::manual("user forced the live overlay to reset");
        let timeline = TimelineEvent {
            sequence: context.timeline_sequence.saturating_add(1),
            time,
            provenance: provenance.clone(),
            kind: TimelineEventKind::RunBoundary {
                state: RunState::Exited,
                scene_id: None,
                reason: BoundaryReason::Manual,
            },
        };
        let boundary = EventEnvelope {
            schema_version: context.schema_version,
            session_id: context.session_id.clone(),
            sequence: context.sequence.saturating_add(1),
            region: context.region.clone(),
            time,
            provenance,
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(timeline),
        };
        let sealed_logs = match self.segments.as_mut() {
            Some(segments) => segments.consume_batch(vec![boundary.clone()])?,
            None => Vec::new(),
        };
        self.observe_sealed(&sealed_logs);
        Ok(ContinuousForcedReset {
            boundary: Some(boundary),
            sealed_logs,
            invalidated_active_run,
        })
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
        observe: impl FnMut(&rlogs_events::EventEnvelope),
    ) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        self.process_frame_with_local_observations(frame, observe, |_| {})
    }

    pub fn process_frame_with_local_observations(
        &mut self,
        frame: CapturedFrame,
        observe: impl FnMut(&rlogs_events::EventEnvelope),
        observe_photo: impl FnMut(&LocalPhotoAssetReference),
    ) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        self.process_frame_with_inspection(frame, observe, observe_photo, |_, _| {})
    }

    /// Decodes one frame and exposes the reviewed capture record/status pair
    /// after protocol routing. The observer is intentionally borrowed and
    /// synchronous so callers can retain only explicitly bounded local detail.
    pub fn process_frame_with_inspection(
        &mut self,
        frame: CapturedFrame,
        mut observe: impl FnMut(&rlogs_events::EventEnvelope),
        mut observe_photo: impl FnMut(&LocalPhotoAssetReference),
        mut observe_protocol: impl FnMut(&CaptureRecord, crate::ProtocolDecodeStatus),
    ) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        self.metrics.frame_count = self.metrics.frame_count.saturating_add(1);
        let pipeline = self
            .pipeline
            .as_mut()
            .ok_or(ContinuousRecordingError::NoAttributedConnection)?;
        let mut drafts = Vec::new();
        pipeline.process_frame(&frame, |draft| drafts.push(draft));
        self.process_drafts(
            drafts,
            &mut observe,
            &mut observe_photo,
            &mut observe_protocol,
        )
    }

    /// Drains transport/framing state and seals an active run as incomplete.
    /// Normal dungeon completion does not call this; monitoring continues.
    pub fn finish(&mut self) -> Result<Vec<SealedDungeonRunLog>, ContinuousRecordingError> {
        let mut sealed = Vec::new();
        if let Some(pipeline) = &mut self.pipeline {
            let mut drafts = Vec::new();
            pipeline.finish(|draft| drafts.push(draft));
            sealed.extend(self.process_drafts(drafts, &mut |_| {}, &mut |_| {}, &mut |_, _| {})?);
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
        observe_photo: &mut impl FnMut(&LocalPhotoAssetReference),
        observe_protocol: &mut impl FnMut(&CaptureRecord, crate::ProtocolDecodeStatus),
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
            let source_before_record = self.runtime.authorized_local_skill_observation_source();
            let local_skill_route = self.runtime.is_authorized_local_skill_record(&record);
            let mut batch = self.runtime.process(&record)?;
            self.local_skill_observation
                .observe_protocol(local_skill_route, batch.status);
            for event in &batch.events {
                self.local_skill_observation.observe_event(
                    event,
                    source_before_record,
                    local_skill_route,
                );
            }
            let terminal = batch.events.iter().find(|event| terminal_run_event(event));
            if let Some(terminal) = terminal {
                let terminal_time = terminal.time;
                let terminal_sequence = terminal.sequence;
                if let Some((receipt, completion_sequence, mut skill_event_sequences)) = self
                    .local_skill_observation
                    .finish(terminal_time.observed_micros, self.runtime.region_context())
                {
                    skill_event_sequences.extend([completion_sequence, terminal_sequence]);
                    let receipt_event = self.runtime.emit_local_skill_observation_receipt(
                        terminal_time,
                        EventProvenance::derived(
                            "bpsr.complete-local-skill-observation.v1",
                            skill_event_sequences,
                        ),
                        receipt,
                    )?;
                    batch.events.push(receipt_event);
                }
            }
            observe_protocol(&record, batch.status);
            self.metrics.record_count = self.metrics.record_count.saturating_add(1);
            self.metrics.decoded_event_count = self
                .metrics
                .decoded_event_count
                .saturating_add(batch.events.len() as u64);
            for event in &batch.events {
                observe(event);
                let timeline_sequence = match &event.event {
                    CanonicalEvent::Timeline(timeline) => timeline.sequence,
                    _ => self
                        .last_event_context
                        .as_ref()
                        .map_or(0, |context| context.timeline_sequence),
                };
                self.last_event_context = Some(ManualEventContext {
                    schema_version: event.schema_version,
                    session_id: event.session_id.clone(),
                    sequence: event.sequence,
                    region: event.region.clone(),
                    time: event.time,
                    timeline_sequence,
                });
            }
            for photo in &batch.local_photo_assets {
                observe_photo(photo);
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

fn terminal_run_event(event: &EventEnvelope) -> bool {
    matches!(
        &event.event,
        CanonicalEvent::Dungeon(dungeon)
            if matches!(dungeon.kind, DungeonEventKind::Failed | DungeonEventKind::Exited)
    ) || matches!(
        &event.event,
        CanonicalEvent::Timeline(timeline)
            if matches!(timeline.kind, TimelineEventKind::RunBoundary {
                state: RunState::Failed | RunState::Exited,
                ..
            })
    )
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
        AbilityId, ActorId, CanonicalEventDraft, CanonicalEventDraftKind, CastEvent, CastState,
        DungeonEvent, EntityRef, EntityUuid, EventEnvelopeFactory, EventProvenance,
        EventSensitivity, EventTime, RegionContext, TimelineEventKind,
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
    fn local_skill_receipt_uses_authoritative_run_bounds_not_entry_or_exit_settlement() {
        let source = EntityRef {
            actor_id: ActorId(8),
            entity_uuid: EntityUuid(80),
        };
        let region = RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "global".into(),
                realm_id: None,
                world_id: None,
            },
            client_build: "25247556".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: Vec::new(),
        };
        let mut factory = EventEnvelopeFactory::new("receipt", region.clone());
        let (entered, started, completed) = {
            let mut dungeon = |kind, observed_micros| {
                factory
                    .emit(CanonicalEventDraft {
                        time: EventTime {
                            observed_micros,
                            game_time_millis: None,
                        },
                        provenance: EventProvenance::wire(observed_micros, 1, 1),
                        sensitivity: EventSensitivity::PublicGameplay,
                        kind: CanonicalEventDraftKind::Dungeon(DungeonEvent {
                            kind,
                            dungeon_id: None,
                            instance_id: Some("run-1".into()),
                            difficulty_id: None,
                            objective_map_key: None,
                            objective_id: None,
                            objective_value: None,
                            objective_complete: None,
                            objective_catalog: None,
                            flow: None,
                        }),
                    })
                    .unwrap()
            };
            (
                dungeon(DungeonEventKind::Entered, 100),
                dungeon(DungeonEventKind::Started, 200),
                dungeon(DungeonEventKind::Completed, 400),
            )
        };
        let cast = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 300,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(3, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Cast(CastEvent {
                    source,
                    ability: AbilityId(42),
                    target: None,
                    state: CastState::Started,
                    action_timing: None,
                })),
            })
            .unwrap();

        let mut tracker = LocalSkillObservationTracker::default();
        tracker.observe_event(&entered, Some(source), false);
        tracker.observe_protocol(true, crate::ProtocolDecodeStatus::Decoded);
        tracker.active.as_mut().unwrap().data_gaps = 1;
        tracker
            .active
            .as_mut()
            .unwrap()
            .skill_event_sequences
            .push(99);
        tracker.observe_event(&started, Some(source), false);
        tracker.observe_protocol(true, crate::ProtocolDecodeStatus::Decoded);
        tracker.observe_event(&cast, Some(source), true);
        tracker.observe_event(&completed, Some(source), false);

        // Settlement after authoritative completion must not enlarge or
        // invalidate the proof window, and teardown need not retain identity.
        tracker.observe_protocol(true, crate::ProtocolDecodeStatus::DecodeFailed);
        tracker.queue_saturations = 1;
        tracker.observe_event(&cast, None, true);
        let (receipt, completion_sequence, skill_sequences) =
            tracker.finish(500, &region).expect("complete receipt");

        assert_eq!(receipt.run_started_micros, 200);
        assert_eq!(receipt.run_ended_micros, 400);
        assert_eq!(receipt.request_count, 1);
        assert_eq!(receipt.decoded_count, 1);
        assert_eq!(receipt.decode_failure_count, 0);
        assert_eq!(receipt.capture_queue_saturation_count, 0);
        assert_eq!(receipt.data_gap_count, 0);
        assert_eq!(completion_sequence, completed.sequence);
        assert_eq!(skill_sequences, vec![cast.sequence]);
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
    fn correlated_photograph_return_reaches_only_the_local_photo_observer() {
        const PHOTOGRAPH: u64 = 904_190_988;
        const GET_ALBUM_PHOTOS: u32 = 4;
        let return_route = RouteKey::new(
            crate::PacketDirection::ServerToClient,
            crate::FragmentKind::Return,
            PHOTOGRAPH,
            GET_ALBUM_PHOTOS,
        );
        let pack = ProtocolPack::build(crate::ProtocolPackDefinition {
            schema_version: crate::PROTOCOL_PACK_SCHEMA_VERSION,
            pack_id: "photo-return-correlation-fixture".into(),
            target: crate::ProtocolPackTarget {
                deployment_id: "global".into(),
                region_id: None,
                channel: "steam".into(),
                build_id: "photo-fixture".into(),
                executable_version: None,
            },
            acquisition: crate::ProtocolPackAcquisition {
                frame_up_layout: crate::BpsrFrameUpLayout::NestedAfterFourBytes,
            },
            provenance: Vec::new(),
            routes: vec![route(
                return_route,
                "Photograph",
                "GetAlbumPhotos",
                crate::ProtocolPackRouteDisposition::Allowed {
                    domain: crate::DecoderKind::GetAlbumPhotosV1.domain(),
                    decoder: crate::DecoderKind::GetAlbumPhotosV1,
                },
            )],
        })
        .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "rlogs-photo-return-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let build = GameBuild {
            deployment_id: "global".into(),
            region_id: None,
            channel: "steam".into(),
            build_id: "photo-fixture".into(),
            executable_version: None,
        };
        let mut recorder = ContinuousBpsrRecorder::new(
            &pack,
            ContinuousRecordingConfig {
                base_session_id: "photo-return".into(),
                producer: "test".into(),
                build,
                region: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: None,
                },
                region_evidence: Vec::new(),
                decoder: ProtocolRuntimeConfig::default(),
                objective_catalog: None,
                output_directory: directory.clone(),
                persist_dungeon_logs: false,
                research_journal: None,
            },
        )
        .unwrap();
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        recorder
            .add_connections([GameConnection { client, server }])
            .unwrap();

        let call_id = 19;
        let call_wire = nested_frame_up(PHOTOGRAPH, GET_ALBUM_PHOTOS, call_id, &[]);
        recorder
            .process_frame(captured_frame(1, 100, client, server, 100, &call_wire))
            .unwrap();
        let body = schema::GetAlbumPhotosReturn {
            ret: Some(schema::GetAlbumPhotosReply {
                character_id: Some(3_296_036),
                photo_graphs: vec![schema::PhotoGraphShow {
                    photo_id: Some(42),
                    images: vec![schema::PhotoImageInfo {
                        picture_type: Some(2),
                        size: Some(900),
                        version: Some(3),
                        cos_url: Some("https://photo.playbpsr.com/xinghen-prod/render.webp".into()),
                    }],
                }],
            }),
        }
        .encode_to_vec();
        let mut return_payload = Vec::new();
        return_payload.extend_from_slice(&1_u32.to_be_bytes());
        return_payload.extend_from_slice(&call_id.to_be_bytes());
        return_payload.extend_from_slice(&0_u32.to_be_bytes());
        return_payload.extend_from_slice(&body);
        let return_wire = bpsr_frame(3, &return_payload);
        let mut events = Vec::new();
        let mut photos = Vec::new();
        let mut protocol = Vec::new();
        recorder
            .process_frame_with_inspection(
                captured_frame(2, 200, server, client, 200, &return_wire),
                |event| events.push(event.clone()),
                |photo| photos.push(photo.clone()),
                |record, status| protocol.push((record.sequence, status)),
            )
            .unwrap();

        assert!(events.is_empty());
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].character_id, 3_296_036);
        assert_eq!(photos[0].photo_id, 42);
        assert_eq!(photos[0].version, Some(3));
        assert_eq!(protocol, vec![(3, crate::ProtocolDecodeStatus::Decoded)]);
        std::fs::remove_dir_all(directory).unwrap();
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
                build_id: crate::BPSR_CURRENT_USE_SKILL_ATTR_BUILD.into(),
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
            build_id: crate::BPSR_CURRENT_USE_SKILL_ATTR_BUILD.into(),
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
                objective_catalog: None,
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
        assert!(sealed.is_empty());
        assert!(writer.is_recording());
        let sealed = writer
            .consume_batch(vec![
                envelopes.emit(event(DungeonEventKind::Exited)).unwrap(),
            ])
            .unwrap();
        assert_eq!(sealed.len(), 1);
        assert!(sealed[0].is_completed());
        assert!(!writer.is_recording());
        assert!(writer.finish().unwrap().is_empty());

        std::fs::remove_file(&sealed[0].path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn forced_reset_seals_an_active_run_as_incomplete_with_a_manual_boundary() {
        let directory = std::env::temp_dir().join(format!(
            "rlogs-forced-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let pack = ProtocolPack::build(crate::ProtocolPackDefinition {
            schema_version: crate::PROTOCOL_PACK_SCHEMA_VERSION,
            pack_id: "forced-reset-fixture".into(),
            target: crate::ProtocolPackTarget {
                deployment_id: "global".into(),
                region_id: None,
                channel: "steam".into(),
                build_id: "fixture".into(),
                executable_version: None,
            },
            acquisition: crate::ProtocolPackAcquisition {
                frame_up_layout: crate::BpsrFrameUpLayout::NestedAfterFourBytes,
            },
            provenance: Vec::new(),
            routes: Vec::new(),
        })
        .unwrap();
        let region = RegionIdentity {
            deployment_id: "global".into(),
            region_id: "global".into(),
            realm_id: None,
            world_id: None,
        };
        let mut recorder = ContinuousBpsrRecorder::new(
            &pack,
            ContinuousRecordingConfig {
                base_session_id: "forced-reset".into(),
                producer: "test".into(),
                build: GameBuild {
                    deployment_id: "global".into(),
                    region_id: Some("global".into()),
                    channel: "steam".into(),
                    build_id: "fixture".into(),
                    executable_version: None,
                },
                region: region.clone(),
                region_evidence: Vec::new(),
                decoder: ProtocolRuntimeConfig::default(),
                objective_catalog: None,
                output_directory: directory.clone(),
                persist_dungeon_logs: true,
                research_journal: None,
            },
        )
        .unwrap();
        let mut envelopes = EventEnvelopeFactory::new(
            "forced-reset",
            RegionContext {
                identity: region,
                client_build: "fixture".into(),
                protocol_pack_digest: pack.digest().into(),
                evidence: Vec::new(),
            },
        );
        let entered = envelopes
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 1_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Dungeon(DungeonEvent {
                    kind: DungeonEventKind::Entered,
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
            })
            .unwrap();
        recorder.last_event_context = Some(ManualEventContext {
            schema_version: entered.schema_version,
            session_id: entered.session_id.clone(),
            sequence: entered.sequence,
            region: entered.region.clone(),
            time: entered.time,
            timeline_sequence: 0,
        });
        assert!(
            recorder
                .segments
                .as_mut()
                .unwrap()
                .consume_batch(vec![entered])
                .unwrap()
                .is_empty()
        );
        assert!(recorder.is_saving_run());

        let reset = recorder.force_reset(2_000).unwrap();
        assert!(reset.invalidated_active_run);
        assert_eq!(reset.sealed_logs.len(), 1);
        assert!(!reset.sealed_logs[0].is_completed());
        assert!(!recorder.is_saving_run());
        let boundary = reset.boundary.expect("manual reset boundary");
        assert!(matches!(
            boundary.event,
            CanonicalEvent::Timeline(TimelineEvent {
                kind: TimelineEventKind::RunBoundary {
                    state: RunState::Exited,
                    reason: BoundaryReason::Manual,
                    ..
                },
                ..
            })
        ));

        std::fs::remove_file(&reset.sealed_logs[0].path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
}
