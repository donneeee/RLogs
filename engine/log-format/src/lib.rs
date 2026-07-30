//! Versioned, sealed, streaming `.rlog` files containing canonical events.

use std::io::{BufRead, Write};

use rlogs_events::{
    CanonicalEvent, EVENT_SCHEMA_VERSION, EventEnvelope, EventSensitivity, RegionContext,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const RLOG_SCHEMA_VERSION: u16 = 1;
pub const MINIMUM_SUPPORTED_EVENT_SCHEMA_VERSION: u16 = 1;
pub const DEFAULT_MAXIMUM_LINE_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_MAXIMUM_EVENTS: u64 = 2_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RlogHeader {
    pub schema_version: u16,
    pub event_schema_version: u16,
    pub session_id: String,
    pub region: RegionContext,
    pub producer: String,
}

impl RlogHeader {
    pub fn new(
        session_id: impl Into<String>,
        region: RegionContext,
        producer: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: RLOG_SCHEMA_VERSION,
            event_schema_version: EVENT_SCHEMA_VERSION,
            session_id: session_id.into(),
            region,
            producer: producer.into(),
        }
    }

    pub fn validate(&self) -> Result<(), RlogError> {
        if self.schema_version != RLOG_SCHEMA_VERSION {
            return Err(RlogError::UnsupportedSchema {
                actual: self.schema_version,
            });
        }
        if !(MINIMUM_SUPPORTED_EVENT_SCHEMA_VERSION..=EVENT_SCHEMA_VERSION)
            .contains(&self.event_schema_version)
        {
            return Err(RlogError::UnsupportedEventSchema {
                actual: self.event_schema_version,
            });
        }
        if self.session_id.trim().is_empty() {
            return Err(RlogError::EmptySessionId);
        }
        if self.producer.trim().is_empty() {
            return Err(RlogError::EmptyProducer);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RlogSeal {
    pub event_count: u64,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RlogLimits {
    pub maximum_line_bytes: usize,
    pub maximum_events: u64,
}

impl Default for RlogLimits {
    fn default() -> Self {
        Self {
            maximum_line_bytes: DEFAULT_MAXIMUM_LINE_BYTES,
            maximum_events: DEFAULT_MAXIMUM_EVENTS,
        }
    }
}

impl RlogLimits {
    fn validate(self) -> Result<Self, RlogError> {
        if self.maximum_line_bytes == 0 || self.maximum_events == 0 {
            return Err(RlogError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RlogReplaySummary {
    pub event_count: u64,
    pub first_observed_micros: Option<u64>,
    pub last_observed_micros: Option<u64>,
    pub content_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum RlogRecord {
    Header { header: RlogHeader },
    Event { envelope: Box<EventEnvelope> },
    Seal { seal: RlogSeal },
}

pub struct RlogWriter<W: Write> {
    output: W,
    header: RlogHeader,
    hasher: Sha256,
    event_count: u64,
    previous_sequence: Option<u64>,
    previous_timeline_sequence: Option<u64>,
    previous_observed_micros: Option<u64>,
}

impl<W: Write> RlogWriter<W> {
    pub fn new(mut output: W, header: RlogHeader) -> Result<Self, RlogError> {
        header.validate()?;
        write_record(
            &mut output,
            &RlogRecord::Header {
                header: header.clone(),
            },
        )?;
        Ok(Self {
            output,
            header,
            hasher: Sha256::new(),
            event_count: 0,
            previous_sequence: None,
            previous_timeline_sequence: None,
            previous_observed_micros: None,
        })
    }

    pub fn push(&mut self, envelope: &EventEnvelope) -> Result<(), RlogError> {
        validate_envelope(
            &self.header,
            envelope,
            self.previous_sequence,
            self.previous_timeline_sequence,
            self.previous_observed_micros,
        )?;
        update_content_digest(&mut self.hasher, envelope)?;
        write_record(
            &mut self.output,
            &RlogRecord::Event {
                envelope: Box::new(envelope.clone()),
            },
        )?;
        self.event_count = self
            .event_count
            .checked_add(1)
            .ok_or(RlogError::EventCountOverflow)?;
        self.previous_sequence = Some(envelope.sequence);
        if let CanonicalEvent::Timeline(event) = &envelope.event {
            self.previous_timeline_sequence = Some(event.sequence);
        }
        self.previous_observed_micros = Some(envelope.time.observed_micros);
        Ok(())
    }

    pub fn finish(self) -> Result<W, RlogError> {
        self.finish_with_seal().map(|(output, _)| output)
    }

    pub fn finish_with_seal(mut self) -> Result<(W, RlogSeal), RlogError> {
        let content_sha256 = digest_string(self.hasher.finalize());
        let seal = RlogSeal {
            event_count: self.event_count,
            content_sha256,
        };
        write_record(&mut self.output, &RlogRecord::Seal { seal: seal.clone() })?;
        self.output.flush()?;
        Ok((self.output, seal))
    }
}

pub struct RlogReader<R: BufRead> {
    input: R,
    limits: RlogLimits,
    header: RlogHeader,
    line_number: u64,
    hasher: Sha256,
    event_count: u64,
    previous_sequence: Option<u64>,
    previous_timeline_sequence: Option<u64>,
    previous_observed_micros: Option<u64>,
    first_observed_micros: Option<u64>,
    finished: bool,
    summary: Option<RlogReplaySummary>,
}

impl<R: BufRead> RlogReader<R> {
    pub fn new(mut input: R, limits: RlogLimits) -> Result<Self, RlogError> {
        let limits = limits.validate()?;
        let line = read_bounded_line(&mut input, limits.maximum_line_bytes, 1)?
            .ok_or(RlogError::MissingHeader)?;
        let record = parse_record(&line, 1)?;
        let RlogRecord::Header { header } = record else {
            return Err(RlogError::HeaderMustBeFirst);
        };
        header.validate()?;
        Ok(Self {
            input,
            limits,
            header,
            line_number: 1,
            hasher: Sha256::new(),
            event_count: 0,
            previous_sequence: None,
            previous_timeline_sequence: None,
            previous_observed_micros: None,
            first_observed_micros: None,
            finished: false,
            summary: None,
        })
    }

    pub fn header(&self) -> &RlogHeader {
        &self.header
    }

    /// Returns one canonical event at a time while preserving all ordering,
    /// privacy, limit, and integrity state in the reader.
    ///
    /// `None` means the integrity seal and end-of-file were validated. Callers
    /// may pause between events without loading the complete log into memory.
    pub fn next_event(&mut self) -> Result<Option<EventEnvelope>, RlogError> {
        if self.finished {
            return Ok(None);
        }
        self.line_number = self
            .line_number
            .checked_add(1)
            .ok_or(RlogError::LineNumberOverflow)?;
        let Some(line) = read_bounded_line(
            &mut self.input,
            self.limits.maximum_line_bytes,
            self.line_number,
        )?
        else {
            return Err(RlogError::MissingSeal);
        };
        match parse_record(&line, self.line_number)? {
            RlogRecord::Header { .. } => Err(RlogError::DuplicateHeader),
            RlogRecord::Event { envelope } => {
                if self.event_count >= self.limits.maximum_events {
                    return Err(RlogError::EventLimitExceeded {
                        maximum: self.limits.maximum_events,
                    });
                }
                validate_envelope(
                    &self.header,
                    &envelope,
                    self.previous_sequence,
                    self.previous_timeline_sequence,
                    self.previous_observed_micros,
                )?;
                update_content_digest(&mut self.hasher, &envelope)?;
                self.event_count += 1;
                self.previous_sequence = Some(envelope.sequence);
                if let CanonicalEvent::Timeline(event) = &envelope.event {
                    self.previous_timeline_sequence = Some(event.sequence);
                }
                self.previous_observed_micros = Some(envelope.time.observed_micros);
                self.first_observed_micros
                    .get_or_insert(envelope.time.observed_micros);
                Ok(Some(*envelope))
            }
            RlogRecord::Seal { seal } => {
                if seal.event_count != self.event_count {
                    return Err(RlogError::SealEventCountMismatch {
                        expected: self.event_count,
                        actual: seal.event_count,
                    });
                }
                let actual_digest = digest_string(self.hasher.clone().finalize());
                if seal.content_sha256 != actual_digest {
                    return Err(RlogError::SealDigestMismatch {
                        expected: seal.content_sha256,
                        actual: actual_digest,
                    });
                }
                ensure_end_of_file(
                    &mut self.input,
                    self.limits.maximum_line_bytes,
                    self.line_number,
                )?;
                self.finished = true;
                self.summary = Some(RlogReplaySummary {
                    event_count: self.event_count,
                    first_observed_micros: self.first_observed_micros,
                    last_observed_micros: self.previous_observed_micros,
                    content_sha256: actual_digest,
                });
                Ok(None)
            }
        }
    }

    pub fn summary(&self) -> Option<&RlogReplaySummary> {
        self.summary.as_ref()
    }

    pub fn replay(
        mut self,
        mut on_event: impl FnMut(&EventEnvelope) -> Result<(), String>,
    ) -> Result<RlogReplaySummary, RlogError> {
        while let Some(envelope) = self.next_event()? {
            on_event(&envelope).map_err(|detail| RlogError::Consumer { detail })?;
        }
        self.summary.take().ok_or(RlogError::MissingSeal)
    }
}

fn validate_envelope(
    header: &RlogHeader,
    envelope: &EventEnvelope,
    previous_sequence: Option<u64>,
    previous_timeline_sequence: Option<u64>,
    previous_observed_micros: Option<u64>,
) -> Result<(), RlogError> {
    if envelope.schema_version != header.event_schema_version {
        return Err(RlogError::EnvelopeSchemaMismatch {
            expected: header.event_schema_version,
            actual: envelope.schema_version,
        });
    }
    if envelope.session_id != header.session_id {
        return Err(RlogError::SessionMismatch {
            expected: header.session_id.clone(),
            actual: envelope.session_id.clone(),
        });
    }
    if envelope.region != header.region {
        return Err(RlogError::RegionMismatch);
    }
    let expected_sequence = previous_sequence.map_or(1, |sequence| sequence.saturating_add(1));
    if envelope.sequence != expected_sequence {
        return Err(RlogError::SequenceMismatch {
            expected: expected_sequence,
            actual: envelope.sequence,
        });
    }
    if let CanonicalEvent::Timeline(event) = &envelope.event {
        let expected_timeline_sequence =
            previous_timeline_sequence.map_or(1, |sequence| sequence.saturating_add(1));
        if event.sequence != expected_timeline_sequence {
            return Err(RlogError::TimelineSequenceMismatch {
                expected: expected_timeline_sequence,
                actual: event.sequence,
            });
        }
        if let rlogs_events::TimelineEventKind::RecorderPause(pause) = &event.kind {
            if pause.resumed_micros < pause.started_micros {
                return Err(RlogError::RecorderPauseMovedBackward {
                    started_micros: pause.started_micros,
                    resumed_micros: pause.resumed_micros,
                });
            }
            if pause.resumed_micros != envelope.time.observed_micros {
                return Err(RlogError::RecorderPauseResumeMismatch {
                    event_observed_micros: envelope.time.observed_micros,
                    resumed_micros: pause.resumed_micros,
                });
            }
        }
    }
    if let Some(previous) = previous_observed_micros
        && envelope.time.observed_micros < previous
    {
        return Err(RlogError::ObservedTimeMovedBackward {
            previous,
            next: envelope.time.observed_micros,
        });
    }
    if envelope.sensitivity == EventSensitivity::LocalSensitive {
        return Err(RlogError::LocalSensitiveEvent);
    }
    Ok(())
}

fn update_content_digest(hasher: &mut Sha256, envelope: &EventEnvelope) -> Result<(), RlogError> {
    hasher.update(serde_json::to_vec(envelope)?);
    hasher.update(b"\n");
    Ok(())
}

fn digest_string(digest: impl std::fmt::LowerHex) -> String {
    format!("sha256:{digest:x}")
}

fn write_record(output: &mut impl Write, record: &RlogRecord) -> Result<(), RlogError> {
    serde_json::to_writer(&mut *output, record)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn parse_record(line: &[u8], line_number: u64) -> Result<RlogRecord, RlogError> {
    serde_json::from_slice(line).map_err(|source| RlogError::InvalidRecord {
        line: line_number,
        source,
    })
}

fn read_bounded_line(
    input: &mut impl BufRead,
    maximum: usize,
    line_number: u64,
) -> Result<Option<Vec<u8>>, RlogError> {
    let mut output = Vec::new();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            if output.is_empty() {
                return Ok(None);
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if output.len().saturating_add(take) > maximum.saturating_add(1) {
            return Err(RlogError::LineTooLong {
                line: line_number,
                maximum,
            });
        }
        output.extend_from_slice(&available[..take]);
        input.consume(take);
        if newline.is_some() {
            break;
        }
    }
    if output.last() == Some(&b'\n') {
        output.pop();
    }
    if output.last() == Some(&b'\r') {
        output.pop();
    }
    if output.is_empty() {
        return Err(RlogError::EmptyLine { line: line_number });
    }
    if output.len() > maximum {
        return Err(RlogError::LineTooLong {
            line: line_number,
            maximum,
        });
    }
    Ok(Some(output))
}

fn ensure_end_of_file(
    input: &mut impl BufRead,
    maximum: usize,
    seal_line: u64,
) -> Result<(), RlogError> {
    if read_bounded_line(input, maximum, seal_line.saturating_add(1))?.is_some() {
        return Err(RlogError::RecordAfterSeal);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum RlogError {
    #[error("rlog I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("rlog JSON failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid rlog record on line {line}: {source}")]
    InvalidRecord {
        line: u64,
        source: serde_json::Error,
    },

    #[error("rlog schema version {actual} is unsupported")]
    UnsupportedSchema { actual: u16 },

    #[error("canonical event schema version {actual} is unsupported")]
    UnsupportedEventSchema { actual: u16 },

    #[error("rlog session ID cannot be empty")]
    EmptySessionId,

    #[error("rlog producer cannot be empty")]
    EmptyProducer,

    #[error("rlog replay limits must be greater than zero")]
    InvalidLimits,

    #[error("rlog is missing its header")]
    MissingHeader,

    #[error("the rlog header must be the first record")]
    HeaderMustBeFirst,

    #[error("rlog contains more than one header")]
    DuplicateHeader,

    #[error("rlog is missing its integrity seal")]
    MissingSeal,

    #[error("rlog contains a record after its seal")]
    RecordAfterSeal,

    #[error("rlog line {line} exceeds the {maximum}-byte limit")]
    LineTooLong { line: u64, maximum: usize },

    #[error("rlog line {line} is empty")]
    EmptyLine { line: u64 },

    #[error("rlog line number space is exhausted")]
    LineNumberOverflow,

    #[error("rlog event count space is exhausted")]
    EventCountOverflow,

    #[error("rlog exceeds the {maximum}-event replay limit")]
    EventLimitExceeded { maximum: u64 },

    #[error("event schema should be {expected}, but was {actual}")]
    EnvelopeSchemaMismatch { expected: u16, actual: u16 },

    #[error("event session should be {expected}, but was {actual}")]
    SessionMismatch { expected: String, actual: String },

    #[error("event region does not match the rlog header")]
    RegionMismatch,

    #[error("event sequence should be {expected}, but was {actual}")]
    SequenceMismatch { expected: u64, actual: u64 },

    #[error("timeline event sequence should be {expected}, but was {actual}")]
    TimelineSequenceMismatch { expected: u64, actual: u64 },

    #[error("recorder pause ended at {resumed_micros}us before it started at {started_micros}us")]
    RecorderPauseMovedBackward {
        started_micros: u64,
        resumed_micros: u64,
    },

    #[error(
        "recorder pause resumed at {resumed_micros}us but its canonical event was observed at {event_observed_micros}us"
    )]
    RecorderPauseResumeMismatch {
        event_observed_micros: u64,
        resumed_micros: u64,
    },

    #[error("event time moved backward from {previous}us to {next}us")]
    ObservedTimeMovedBackward { previous: u64, next: u64 },

    #[error("local-sensitive events are prohibited from replayable rlog files")]
    LocalSensitiveEvent,

    #[error("rlog consumer rejected an event: {detail}")]
    Consumer { detail: String },

    #[error("rlog seal expected {expected} events, but declared {actual}")]
    SealEventCountMismatch { expected: u64, actual: u64 },

    #[error("rlog seal digest mismatch: declared {expected}, computed {actual}")]
    SealDigestMismatch { expected: String, actual: String },
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use rlogs_events::{
        BoundaryReason, CanonicalEvent, CombatState, EventProvenance, EventTime,
        RecorderPauseEvent, RegionEvidence, RegionEvidenceKind, RegionIdentity, TimelineEvent,
        TimelineEventKind,
    };

    use super::*;

    fn region() -> RegionContext {
        RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: Some("world-1".into()),
            },
            client_build: "fixture".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: vec![RegionEvidence {
                kind: RegionEvidenceKind::ReplayManifest,
                reference: "unit-test".into(),
            }],
        }
    }

    fn envelope(sequence: u64, observed_micros: u64) -> EventEnvelope {
        let provenance = EventProvenance::wire(sequence, 1, 1);
        EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "session-1".into(),
            sequence,
            region: region(),
            time: EventTime {
                observed_micros,
                game_time_millis: None,
            },
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time: EventTime {
                    observed_micros,
                    game_time_millis: None,
                },
                provenance,
                kind: TimelineEventKind::CombatBoundary {
                    state: if sequence == 1 {
                        CombatState::Started
                    } else {
                        CombatState::Ended
                    },
                    reason: BoundaryReason::Manual,
                },
            }),
        }
    }

    fn encoded_log() -> Vec<u8> {
        let header = RlogHeader::new("session-1", region(), "unit-test");
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        writer.push(&envelope(1, 10)).unwrap();
        writer.push(&envelope(2, 20)).unwrap();
        writer.finish().unwrap()
    }

    #[test]
    fn sealed_log_streams_and_verifies() {
        let bytes = encoded_log();
        let reader =
            RlogReader::new(BufReader::new(Cursor::new(bytes)), RlogLimits::default()).unwrap();
        let mut sequences = Vec::new();
        let summary = reader
            .replay(|event| {
                sequences.push(event.sequence);
                Ok(())
            })
            .unwrap();

        assert_eq!(sequences, vec![1, 2]);
        assert_eq!(summary.event_count, 2);
        assert_eq!(summary.first_observed_micros, Some(10));
        assert_eq!(summary.last_observed_micros, Some(20));
        assert!(summary.content_sha256.starts_with("sha256:"));
    }

    #[test]
    fn incremental_reader_pauses_without_buffering_the_complete_log() {
        let bytes = encoded_log();
        let mut reader =
            RlogReader::new(BufReader::new(Cursor::new(bytes)), RlogLimits::default()).unwrap();

        assert_eq!(reader.next_event().unwrap().unwrap().sequence, 1);
        assert!(reader.summary().is_none());
        assert_eq!(reader.next_event().unwrap().unwrap().sequence, 2);
        assert!(reader.summary().is_none());
        assert!(reader.next_event().unwrap().is_none());
        assert_eq!(reader.summary().unwrap().event_count, 2);
        assert!(reader.next_event().unwrap().is_none());
    }

    #[test]
    fn tampered_event_is_rejected_by_the_seal() {
        let bytes = encoded_log();
        let text = String::from_utf8(bytes).unwrap();
        let tampered = text.replace("\"observed_micros\":20", "\"observed_micros\":21");
        let reader = RlogReader::new(
            BufReader::new(Cursor::new(tampered.into_bytes())),
            RlogLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            reader.replay(|_| Ok(())),
            Err(RlogError::SealDigestMismatch { .. })
        ));
    }

    #[test]
    fn oversized_lines_stop_before_unbounded_growth() {
        let input = vec![b'x'; 32];
        assert!(matches!(
            RlogReader::new(
                BufReader::new(Cursor::new(input)),
                RlogLimits {
                    maximum_line_bytes: 8,
                    maximum_events: 10,
                }
            ),
            Err(RlogError::LineTooLong { .. })
        ));
    }

    #[test]
    fn timeline_sequences_are_validated_independently() {
        let header = RlogHeader::new("session-1", region(), "unit-test");
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        let mut event = envelope(1, 10);
        let CanonicalEvent::Timeline(timeline) = &mut event.event else {
            unreachable!();
        };
        timeline.sequence = 2;

        assert!(matches!(
            writer.push(&event),
            Err(RlogError::TimelineSequenceMismatch {
                expected: 1,
                actual: 2
            })
        ));
    }

    #[test]
    fn malformed_recorder_pause_intervals_are_rejected_before_sealing() {
        let header = RlogHeader::new("session-1", region(), "unit-test");
        let mut writer = RlogWriter::new(Vec::new(), header.clone()).unwrap();
        let mut backward = envelope(1, 20);
        let CanonicalEvent::Timeline(timeline) = &mut backward.event else {
            unreachable!();
        };
        timeline.kind = TimelineEventKind::RecorderPause(RecorderPauseEvent {
            started_micros: 21,
            resumed_micros: 20,
        });
        assert!(matches!(
            writer.push(&backward),
            Err(RlogError::RecorderPauseMovedBackward {
                started_micros: 21,
                resumed_micros: 20
            })
        ));

        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        let mut mismatched = envelope(1, 20);
        let CanonicalEvent::Timeline(timeline) = &mut mismatched.event else {
            unreachable!();
        };
        timeline.kind = TimelineEventKind::RecorderPause(RecorderPauseEvent {
            started_micros: 10,
            resumed_micros: 19,
        });
        assert!(matches!(
            writer.push(&mismatched),
            Err(RlogError::RecorderPauseResumeMismatch {
                event_observed_micros: 20,
                resumed_micros: 19
            })
        ));
    }

    #[test]
    fn version_one_event_logs_remain_readable_after_additive_schema_updates() {
        let mut header = RlogHeader::new("session-1", region(), "unit-test");
        header.event_schema_version = 1;
        assert!(header.validate().is_ok());
    }
}
