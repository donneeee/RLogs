//! Atomic `.rlog` output for packet-delimited BPSR dungeon runs.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use rlogs_events::{CanonicalEvent, EventEnvelope};
use rlogs_log_format::{RlogError, RlogHeader, RlogSeal, RlogWriter};
use thiserror::Error;

use crate::{
    DungeonRunSegmenter, DungeonSegmentAction, DungeonSegmentBoundary, DungeonSegmentEndReason,
    DungeonSegmentStartReason,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedDungeonRunLog {
    pub session_id: String,
    pub path: PathBuf,
    pub start_reason: DungeonSegmentStartReason,
    pub end_reason: DungeonSegmentEndReason,
    pub started: DungeonSegmentBoundary,
    pub ended: DungeonSegmentBoundary,
    pub seal: RlogSeal,
}

impl SealedDungeonRunLog {
    pub fn is_completed(&self) -> bool {
        self.end_reason == DungeonSegmentEndReason::Completed
    }
}

/// Persists only the canonical events admitted by [`DungeonRunSegmenter`].
///
/// Network and protocol state can remain live before entry, but no `.rlog`
/// exists until the game plug-in sees an authoritative dungeon boundary.
pub struct SegmentedDungeonLogWriter {
    output_directory: PathBuf,
    base_session_id: String,
    producer: String,
    next_run_index: u32,
    segmenter: DungeonRunSegmenter,
    active: Option<ActiveWriter>,
}

struct ActiveWriter {
    session_id: String,
    partial_path: PathBuf,
    final_path: PathBuf,
    start_reason: DungeonSegmentStartReason,
    started: DungeonSegmentBoundary,
    writer: RlogWriter<BufWriter<File>>,
    next_sequence: u64,
    next_timeline_sequence: u64,
}

impl SegmentedDungeonLogWriter {
    pub fn new(
        output_directory: impl Into<PathBuf>,
        base_session_id: impl Into<String>,
        producer: impl Into<String>,
    ) -> Result<Self, SegmentedRecordingError> {
        let output_directory = output_directory.into();
        let base_session_id = base_session_id.into();
        validate_id(&base_session_id)?;
        let producer = producer.into();
        if producer.trim().is_empty() {
            return Err(SegmentedRecordingError::EmptyProducer);
        }
        std::fs::create_dir_all(&output_directory)?;
        let output_directory = std::fs::canonicalize(output_directory)?;
        Ok(Self {
            output_directory,
            base_session_id,
            producer,
            next_run_index: 1,
            segmenter: DungeonRunSegmenter::default(),
            active: None,
        })
    }

    pub fn is_recording(&self) -> bool {
        self.segmenter.is_recording()
    }

    /// Consumes one decoder batch. Keeping the batch intact guarantees that
    /// all events emitted by a completion packet are written before sealing.
    pub fn consume_batch(
        &mut self,
        events: impl IntoIterator<Item = EventEnvelope>,
    ) -> Result<Vec<SealedDungeonRunLog>, SegmentedRecordingError> {
        let actions = self.segmenter.observe_batch(events);
        self.apply(actions)
    }

    pub fn finish(&mut self) -> Result<Vec<SealedDungeonRunLog>, SegmentedRecordingError> {
        let Some(action) = self.segmenter.finish() else {
            return Ok(Vec::new());
        };
        self.apply([action])
    }

    fn apply(
        &mut self,
        actions: impl IntoIterator<Item = DungeonSegmentAction>,
    ) -> Result<Vec<SealedDungeonRunLog>, SegmentedRecordingError> {
        let mut sealed = Vec::new();
        let mut pending_open = None;
        for action in actions {
            match action {
                DungeonSegmentAction::Open { reason, boundary } => {
                    if self.active.is_some() || pending_open.is_some() {
                        return Err(SegmentedRecordingError::InvalidActionOrder(
                            "open received while a segment is active",
                        ));
                    }
                    pending_open = Some((reason, boundary));
                }
                DungeonSegmentAction::Record(mut envelope) => {
                    if self.active.is_none() {
                        let (reason, boundary) = pending_open.take().ok_or(
                            SegmentedRecordingError::InvalidActionOrder(
                                "record received without an opening boundary",
                            ),
                        )?;
                        self.open(reason, boundary, &envelope)?;
                    }
                    let active =
                        self.active
                            .as_mut()
                            .ok_or(SegmentedRecordingError::InvalidActionOrder(
                                "segment writer was not opened",
                            ))?;
                    resequence(
                        &mut envelope,
                        &active.session_id,
                        &mut active.next_sequence,
                        &mut active.next_timeline_sequence,
                    )?;
                    active.writer.push(&envelope)?;
                }
                DungeonSegmentAction::Seal { reason, boundary } => {
                    if pending_open.is_some() {
                        return Err(SegmentedRecordingError::InvalidActionOrder(
                            "seal received before the opening event",
                        ));
                    }
                    sealed.push(self.seal(reason, boundary)?);
                }
            }
        }
        if pending_open.is_some() {
            return Err(SegmentedRecordingError::InvalidActionOrder(
                "opening boundary was not followed by its event",
            ));
        }
        Ok(sealed)
    }

    fn open(
        &mut self,
        start_reason: DungeonSegmentStartReason,
        started: DungeonSegmentBoundary,
        first_event: &EventEnvelope,
    ) -> Result<(), SegmentedRecordingError> {
        let index = self.next_run_index;
        self.next_run_index = self
            .next_run_index
            .checked_add(1)
            .ok_or(SegmentedRecordingError::RunIndexExhausted)?;
        let session_id = format!("{}.run-{index:04}", self.base_session_id);
        let final_path = self.output_directory.join(format!("{session_id}.rlog"));
        let partial_path = self
            .output_directory
            .join(format!("{session_id}.partial.rlog"));
        for path in [&final_path, &partial_path] {
            if path.exists() {
                return Err(SegmentedRecordingError::OutputExists(path.clone()));
            }
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial_path)?;
        let header = RlogHeader {
            schema_version: rlogs_log_format::RLOG_SCHEMA_VERSION,
            event_schema_version: first_event.schema_version,
            session_id: session_id.clone(),
            region: first_event.region.clone(),
            producer: self.producer.clone(),
        };
        let writer = match RlogWriter::new(BufWriter::new(file), header) {
            Ok(writer) => writer,
            Err(error) => {
                remove_partial(&partial_path);
                return Err(error.into());
            }
        };
        self.active = Some(ActiveWriter {
            session_id,
            partial_path,
            final_path,
            start_reason,
            started,
            writer,
            next_sequence: 1,
            next_timeline_sequence: 1,
        });
        Ok(())
    }

    fn seal(
        &mut self,
        end_reason: DungeonSegmentEndReason,
        ended: DungeonSegmentBoundary,
    ) -> Result<SealedDungeonRunLog, SegmentedRecordingError> {
        let active = self
            .active
            .take()
            .ok_or(SegmentedRecordingError::InvalidActionOrder(
                "seal received without an active segment",
            ))?;
        let result = (|| {
            let (mut output, seal) = active.writer.finish_with_seal()?;
            output.flush()?;
            output.get_ref().sync_all()?;
            drop(output);
            std::fs::rename(&active.partial_path, &active.final_path)?;
            Ok(SealedDungeonRunLog {
                session_id: active.session_id,
                path: active.final_path,
                start_reason: active.start_reason,
                end_reason,
                started: active.started,
                ended,
                seal,
            })
        })();
        if result.is_err() {
            remove_partial(&active.partial_path);
        }
        result
    }
}

fn resequence(
    envelope: &mut EventEnvelope,
    session_id: &str,
    next_sequence: &mut u64,
    next_timeline_sequence: &mut u64,
) -> Result<(), SegmentedRecordingError> {
    envelope.session_id = session_id.into();
    envelope.sequence = *next_sequence;
    *next_sequence = next_sequence
        .checked_add(1)
        .ok_or(SegmentedRecordingError::SequenceExhausted)?;
    if let CanonicalEvent::Timeline(timeline) = &mut envelope.event {
        timeline.sequence = *next_timeline_sequence;
        *next_timeline_sequence = next_timeline_sequence
            .checked_add(1)
            .ok_or(SegmentedRecordingError::SequenceExhausted)?;
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), SegmentedRecordingError> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(SegmentedRecordingError::InvalidSessionId);
    }
    Ok(())
}

fn remove_partial(path: &Path) {
    if path.is_file() {
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Debug, Error)]
pub enum SegmentedRecordingError {
    #[error("base session ID must use 1-96 ASCII letters, digits, '.', '_', or '-'")]
    InvalidSessionId,

    #[error("recording producer must not be empty")]
    EmptyProducer,

    #[error("refusing to overwrite segmented log {0}")]
    OutputExists(PathBuf),

    #[error("dungeon segment action order is invalid: {0}")]
    InvalidActionOrder(&'static str),

    #[error("dungeon run index space is exhausted")]
    RunIndexExhausted,

    #[error("segmented event sequence space is exhausted")]
    SequenceExhausted,

    #[error(transparent)]
    Rlog(#[from] RlogError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use rlogs_events::{
        BoundaryReason, CanonicalEventDraft, CanonicalEventDraftKind, CharacterIdentity,
        DungeonEvent, DungeonEventKind, EventEnvelopeFactory, EventProvenance, EventSensitivity,
        EventTime, GameProfileEvent, RegionContext, RegionIdentity, RunState, TimelineEventKind,
    };
    use rlogs_log_format::{RlogLimits, RlogReader};

    use super::*;

    fn region() -> RegionContext {
        RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "global".into(),
                realm_id: None,
                world_id: None,
            },
            client_build: "fixture".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: Vec::new(),
        }
    }

    fn dungeon_draft(sequence: u64, kind: DungeonEventKind) -> CanonicalEventDraft {
        CanonicalEventDraft {
            time: EventTime {
                observed_micros: sequence * 1_000,
                game_time_millis: Some(sequence as i64),
            },
            provenance: EventProvenance::wire(sequence, 1, 2),
            sensitivity: EventSensitivity::PublicGameplay,
            kind: CanonicalEventDraftKind::Dungeon(DungeonEvent {
                kind,
                dungeon_id: None,
                instance_id: Some("instance-1".into()),
                difficulty_id: Some(1),
                objective_map_key: None,
                objective_id: None,
                objective_value: None,
                objective_complete: None,
                objective_catalog: None,
                flow: None,
            }),
        }
    }

    #[test]
    fn writes_safe_scene_context_through_completion_and_resequences_the_run() {
        let directory =
            std::env::temp_dir().join(format!("rlogs-segmented-writer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let region = region();
        let mut envelopes = EventEnvelopeFactory::new("continuous", region.clone());
        let profile_context = envelopes
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 500,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 1664308034, 21),
                sensitivity: EventSensitivity::PersonalGameplay,
                kind: CanonicalEventDraftKind::CharacterProfileObserved {
                    profile: Box::new(GameProfileEvent {
                        game_plugin_id: "blue-protocol-star-resonance".into(),
                        payload_schema_id: "bpsr-character-profile".into(),
                        payload_schema_version: 1,
                        character: CharacterIdentity {
                            region: region.identity,
                            character_id: "3296036".into(),
                        },
                        payload: serde_json::json!({"season_cultivation": [{"season_id": 3}]}),
                    }),
                },
            })
            .unwrap();
        let before = envelopes
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 1_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 1, 2),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::RunBoundary {
                    state: RunState::Entered,
                    scene_id: None,
                    reason: BoundaryReason::AuthoritativePacket,
                }),
            })
            .unwrap();
        let entered = envelopes
            .emit(dungeon_draft(2, DungeonEventKind::Entered))
            .unwrap();
        let completed = envelopes
            .emit(dungeon_draft(3, DungeonEventKind::Completed))
            .unwrap();
        let completion_boundary = envelopes
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 3_000,
                    game_time_millis: Some(3),
                },
                provenance: EventProvenance::wire(3, 1, 2),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::RunBoundary {
                    state: RunState::Completed,
                    scene_id: None,
                    reason: BoundaryReason::AuthoritativePacket,
                }),
            })
            .unwrap();

        let mut writer =
            SegmentedDungeonLogWriter::new(&directory, "continuous", "unit-test").unwrap();
        assert!(writer.consume_batch([profile_context]).unwrap().is_empty());
        assert!(writer.consume_batch([before]).unwrap().is_empty());
        assert!(writer.consume_batch([entered]).unwrap().is_empty());
        let sealed = writer
            .consume_batch([completed, completion_boundary])
            .unwrap();
        assert_eq!(sealed.len(), 1);
        assert!(sealed[0].is_completed());
        assert_eq!(sealed[0].seal.event_count, 5);

        let file = File::open(&sealed[0].path).unwrap();
        let mut reader = RlogReader::new(BufReader::new(file), RlogLimits::default()).unwrap();
        assert_eq!(reader.header().session_id, "continuous.run-0001");
        let first = reader.next_event().unwrap().unwrap();
        assert_eq!(first.sequence, 1);
        assert!(matches!(
            first.event,
            CanonicalEvent::CharacterProfileObserved { .. }
        ));
        assert_eq!(first.time.observed_micros, 500);
        assert!(matches!(
            first.provenance.source,
            rlogs_events::EvidenceSource::Wire {
                capture_sequence: 1,
                connection_id: 1664308034,
                stream_id: 21,
            }
        ));
        let second = reader.next_event().unwrap().unwrap();
        assert_eq!(second.sequence, 2);
        assert!(matches!(
            second.event,
            CanonicalEvent::Timeline(rlogs_events::TimelineEvent {
                kind: TimelineEventKind::RunBoundary {
                    state: RunState::Entered,
                    ..
                },
                ..
            })
        ));
        let third = reader.next_event().unwrap().unwrap();
        assert_eq!(third.sequence, 3);
        assert!(matches!(
            third.event,
            CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Entered,
                ..
            })
        ));
        let fourth = reader.next_event().unwrap().unwrap();
        assert_eq!(fourth.sequence, 4);
        let fifth = reader.next_event().unwrap().unwrap();
        assert_eq!(fifth.sequence, 5);
        let CanonicalEvent::Timeline(timeline) = fifth.event else {
            panic!("expected completion timeline");
        };
        assert_eq!(timeline.sequence, 2);
        assert!(reader.next_event().unwrap().is_none());

        std::fs::remove_file(&sealed[0].path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
}
