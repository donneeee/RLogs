use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use rlogs_events::{
    CanonicalEvent, EventEnvelope, EventSensitivity, StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogError, RlogHeader, RlogSeal, RlogWriter};
use thiserror::Error;

use crate::{TrainingDummyPhase, TrainingDummyState};

#[derive(Debug, Clone)]
pub struct SealedTrainingDummyLog {
    pub session_id: String,
    pub path: PathBuf,
    pub seal: RlogSeal,
    pub result: TrainingDummyState,
}

pub struct TrainingDummyLogWriter {
    output_directory: PathBuf,
    base_session_id: String,
    producer: String,
    next_index: u32,
    armed: bool,
    context: TrainingContext,
    active: Option<ActiveWriter>,
}

struct ActiveWriter {
    session_id: String,
    partial_path: PathBuf,
    final_path: PathBuf,
    writer: RlogWriter<BufWriter<File>>,
    region: rlogs_events::RegionContext,
    next_sequence: u64,
    next_timeline_sequence: u64,
}

#[derive(Default)]
struct TrainingContext {
    world: Option<EventEnvelope>,
    personal_profile: Option<EventEnvelope>,
    party: Option<EventEnvelope>,
    actors: HashMap<u64, EventEnvelope>,
    ownership: HashMap<u64, EventEnvelope>,
    statuses: HashMap<(u64, u64, i64), EventEnvelope>,
}

impl TrainingDummyLogWriter {
    pub fn new(
        output_directory: impl Into<PathBuf>,
        base_session_id: impl Into<String>,
        producer: impl Into<String>,
    ) -> Result<Self, TrainingDummyRecordingError> {
        let output_directory = output_directory.into();
        std::fs::create_dir_all(&output_directory)?;
        Ok(Self {
            output_directory: std::fs::canonicalize(output_directory)?,
            base_session_id: base_session_id.into(),
            producer: producer.into(),
            next_index: 1,
            armed: false,
            context: TrainingContext::default(),
            active: None,
        })
    }

    pub fn arm(&mut self) -> Result<(), TrainingDummyRecordingError> {
        self.discard_active()?;
        self.armed = true;
        Ok(())
    }

    pub fn disarm(&mut self) -> Result<(), TrainingDummyRecordingError> {
        self.armed = false;
        self.discard_active()
    }

    pub fn observe(
        &mut self,
        event: &EventEnvelope,
        state: &TrainingDummyState,
    ) -> Result<Option<SealedTrainingDummyLog>, TrainingDummyRecordingError> {
        if !self.armed {
            self.context.observe(event);
            return Ok(None);
        }

        if self.active.is_none() && state.phase == TrainingDummyPhase::Running {
            self.open(event)?;
        }
        if self.active.is_some() {
            self.write(event.clone())?;
        } else {
            self.context.observe(event);
        }

        match state.phase {
            TrainingDummyPhase::Finished if self.active.is_some() => {
                self.armed = false;
                self.seal(state.clone()).map(Some)
            }
            TrainingDummyPhase::Invalid => {
                self.disarm()?;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn open(&mut self, first_event: &EventEnvelope) -> Result<(), TrainingDummyRecordingError> {
        let index = self.next_index;
        self.next_index = self
            .next_index
            .checked_add(1)
            .ok_or(TrainingDummyRecordingError::IndexExhausted)?;
        let session_id = format!("{}.training-{index:04}", self.base_session_id);
        let final_path = self.output_directory.join(format!("{session_id}.rlog"));
        let partial_path = self
            .output_directory
            .join(format!("{session_id}.partial.rlog"));
        if final_path.exists() || partial_path.exists() {
            return Err(TrainingDummyRecordingError::OutputExists(final_path));
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
        self.active = Some(ActiveWriter {
            session_id,
            partial_path,
            final_path,
            writer: RlogWriter::new(BufWriter::new(file), header)?,
            region: first_event.region.clone(),
            next_sequence: 1,
            next_timeline_sequence: 1,
        });
        for event in self.context.events() {
            self.write(event)?;
        }
        Ok(())
    }

    fn write(&mut self, mut event: EventEnvelope) -> Result<(), TrainingDummyRecordingError> {
        let active = self
            .active
            .as_mut()
            .ok_or(TrainingDummyRecordingError::NotActive)?;
        event.session_id.clone_from(&active.session_id);
        event.region.clone_from(&active.region);
        event.sequence = active.next_sequence;
        active.next_sequence = active
            .next_sequence
            .checked_add(1)
            .ok_or(TrainingDummyRecordingError::SequenceExhausted)?;
        if let CanonicalEvent::Timeline(timeline) = &mut event.event {
            timeline.sequence = active.next_timeline_sequence;
            active.next_timeline_sequence = active
                .next_timeline_sequence
                .checked_add(1)
                .ok_or(TrainingDummyRecordingError::SequenceExhausted)?;
        }
        active.writer.push(&event)?;
        Ok(())
    }

    fn seal(
        &mut self,
        result: TrainingDummyState,
    ) -> Result<SealedTrainingDummyLog, TrainingDummyRecordingError> {
        let active = self
            .active
            .take()
            .ok_or(TrainingDummyRecordingError::NotActive)?;
        let (mut output, seal) = active.writer.finish_with_seal()?;
        output.flush()?;
        output.get_ref().sync_all()?;
        drop(output);
        std::fs::rename(&active.partial_path, &active.final_path)?;
        Ok(SealedTrainingDummyLog {
            session_id: active.session_id,
            path: active.final_path,
            seal,
            result,
        })
    }

    fn discard_active(&mut self) -> Result<(), TrainingDummyRecordingError> {
        if let Some(active) = self.active.take() {
            drop(active.writer);
            match std::fs::remove_file(active.partial_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

impl TrainingContext {
    fn observe(&mut self, event: &EventEnvelope) {
        match &event.event {
            CanonicalEvent::WorldChanged(_) => {
                self.world = Some(event.clone());
                self.actors.clear();
                self.ownership.clear();
                self.statuses.clear();
            }
            CanonicalEvent::CharacterProfileObserved { .. }
                if event.sensitivity == EventSensitivity::PersonalGameplay =>
            {
                self.personal_profile = Some(event.clone());
            }
            CanonicalEvent::PartyRosterObserved(_) | CanonicalEvent::PartyChanged { .. } => {
                self.party = Some(event.clone());
            }
            CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Actor(actor) => {
                    self.actors.insert(actor.actor.actor_id.0, event.clone());
                }
                TimelineEventKind::EntityAttributes(attributes)
                    if attributes.ownership.is_some() =>
                {
                    self.ownership
                        .insert(attributes.actor.actor_id.0, event.clone());
                }
                TimelineEventKind::Status(status) => {
                    let Some(source) = status.source else { return };
                    let key = (source.actor_id.0, status.target.actor_id.0, status.effect.0);
                    match status.state {
                        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                            self.statuses.insert(key, event.clone());
                        }
                        StatusState::Consumed | StatusState::Removed => {
                            self.statuses.remove(&key);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn events(&self) -> Vec<EventEnvelope> {
        let mut events = Vec::new();
        events.extend(self.personal_profile.iter().cloned());
        events.extend(self.world.iter().cloned());
        events.extend(self.party.iter().cloned());
        events.extend(self.actors.values().cloned());
        events.extend(self.ownership.values().cloned());
        events.extend(self.statuses.values().cloned());
        events.sort_by_key(|event| (event.time.observed_micros, event.sequence));
        events.dedup_by_key(|event| event.sequence);
        events
    }
}

impl Drop for TrainingDummyLogWriter {
    fn drop(&mut self) {
        let _ = self.discard_active();
    }
}

#[derive(Debug, Error)]
pub enum TrainingDummyRecordingError {
    #[error("refusing to overwrite training log {0}")]
    OutputExists(PathBuf),
    #[error("training log writer is not active")]
    NotActive,
    #[error("training log index space is exhausted")]
    IndexExhausted,
    #[error("training log event sequence space is exhausted")]
    SequenceExhausted,
    #[error(transparent)]
    Rlog(#[from] RlogError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
