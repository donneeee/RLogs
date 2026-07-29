use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CaptureRecord, CaptureRecordDraft, CaptureSession, CoverageReport, JournalError,
    ProtocolJournal,
};

const DEFAULT_MAX_JSONL_LINE_BYTES: usize = 128 * 1024 * 1024;

/// Streaming, research-only JSONL representation of a protocol journal.
///
/// This is intentionally separate from the future compact `.rlog` container.
/// It lets opcode research begin without retaining an unbounded journal in
/// memory or committing the public log format too early.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "line", content = "data", rename_all = "snake_case")]
enum JsonlJournalLine {
    Session(CaptureSession),
    Record(CaptureRecord),
}

pub struct JsonlJournalWriter<W: Write> {
    writer: W,
    next_sequence: u64,
    last_observed_micros: Option<u64>,
    coverage: CoverageReport,
}

impl<W: Write> JsonlJournalWriter<W> {
    pub fn new(mut writer: W, session: CaptureSession) -> Result<Self, JsonlJournalError> {
        write_line(&mut writer, &JsonlJournalLine::Session(session))?;
        Ok(Self {
            writer,
            next_sequence: 1,
            last_observed_micros: None,
            coverage: CoverageReport::default(),
        })
    }

    pub fn append(&mut self, draft: CaptureRecordDraft) -> Result<u64, JsonlJournalError> {
        if let Some(previous_micros) = self.last_observed_micros {
            if draft.observed_micros < previous_micros {
                return Err(JournalError::ObservedTimeMovedBackward {
                    previous_micros,
                    next_micros: draft.observed_micros,
                }
                .into());
            }
        }
        if self.next_sequence == u64::MAX {
            return Err(JsonlJournalError::SequenceExhausted);
        }

        let sequence = self.next_sequence;
        let record = CaptureRecord {
            sequence,
            observed_micros: draft.observed_micros,
            wall_clock_unix_micros: draft.wall_clock_unix_micros,
            kind: draft.kind,
        };
        write_line(&mut self.writer, &JsonlJournalLine::Record(record.clone()))?;

        self.coverage.observe(&record);
        self.next_sequence += 1;
        self.last_observed_micros = Some(record.observed_micros);
        Ok(sequence)
    }

    pub fn coverage(&self) -> &CoverageReport {
        &self.coverage
    }

    pub fn inner_ref(&self) -> &W {
        &self.writer
    }

    pub fn flush(&mut self) -> Result<(), JsonlJournalError> {
        self.writer.flush().map_err(JsonlJournalError::Io)
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

pub struct JsonlJournalReader<R: BufRead> {
    reader: R,
    max_line_bytes: usize,
}

#[derive(Debug)]
pub struct JsonlJournalSummary {
    pub session: CaptureSession,
    pub coverage: CoverageReport,
    pub record_count: u64,
}

impl<R: BufRead> JsonlJournalReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            max_line_bytes: DEFAULT_MAX_JSONL_LINE_BYTES,
        }
    }

    pub fn with_max_line_bytes(reader: R, max_line_bytes: usize) -> Self {
        Self {
            reader,
            max_line_bytes,
        }
    }

    pub fn read(mut self) -> Result<ProtocolJournal, JsonlJournalError> {
        let mut line = String::new();
        let mut line_number = 0usize;
        let mut journal = None;

        loop {
            line.clear();
            let bytes_read = read_limited_line(
                &mut self.reader,
                &mut line,
                line_number + 1,
                self.max_line_bytes,
            )?;
            if bytes_read == 0 {
                break;
            }
            line_number += 1;
            if line.trim().is_empty() {
                continue;
            }

            let journal_line: JsonlJournalLine =
                serde_json::from_str(&line).map_err(|source| JsonlJournalError::InvalidJson {
                    line: line_number,
                    source,
                })?;

            match journal_line {
                JsonlJournalLine::Session(session) => {
                    if journal.is_some() {
                        return Err(JsonlJournalError::DuplicateSession { line: line_number });
                    }
                    journal = Some(ProtocolJournal::new(session));
                }
                JsonlJournalLine::Record(record) => {
                    let Some(journal) = journal.as_mut() else {
                        return Err(JsonlJournalError::RecordBeforeSession { line: line_number });
                    };
                    let expected_sequence = journal.len() as u64 + 1;
                    if record.sequence != expected_sequence {
                        return Err(JournalError::InvalidSequence {
                            expected: expected_sequence,
                            actual: record.sequence,
                        }
                        .into());
                    }

                    journal.push(CaptureRecordDraft {
                        observed_micros: record.observed_micros,
                        wall_clock_unix_micros: record.wall_clock_unix_micros,
                        kind: record.kind,
                    })?;
                }
            }
        }

        journal.ok_or(JsonlJournalError::MissingSession)
    }

    pub fn summarize(mut self) -> Result<JsonlJournalSummary, JsonlJournalError> {
        let mut line = String::new();
        let mut line_number = 0usize;
        let mut session = None;
        let mut coverage = CoverageReport::default();
        let mut record_count = 0u64;
        let mut previous_observed_micros = None;

        loop {
            line.clear();
            let bytes_read = read_limited_line(
                &mut self.reader,
                &mut line,
                line_number + 1,
                self.max_line_bytes,
            )?;
            if bytes_read == 0 {
                break;
            }
            line_number += 1;
            if line.trim().is_empty() {
                continue;
            }

            let journal_line: JsonlJournalLine =
                serde_json::from_str(&line).map_err(|source| JsonlJournalError::InvalidJson {
                    line: line_number,
                    source,
                })?;

            match journal_line {
                JsonlJournalLine::Session(next_session) => {
                    if session.is_some() {
                        return Err(JsonlJournalError::DuplicateSession { line: line_number });
                    }
                    session = Some(next_session);
                }
                JsonlJournalLine::Record(record) => {
                    if session.is_none() {
                        return Err(JsonlJournalError::RecordBeforeSession { line: line_number });
                    }

                    let expected_sequence = record_count + 1;
                    if record.sequence != expected_sequence {
                        return Err(JournalError::InvalidSequence {
                            expected: expected_sequence,
                            actual: record.sequence,
                        }
                        .into());
                    }
                    if let Some(previous_micros) = previous_observed_micros {
                        if record.observed_micros < previous_micros {
                            return Err(JournalError::ObservedTimeMovedBackward {
                                previous_micros,
                                next_micros: record.observed_micros,
                            }
                            .into());
                        }
                    }

                    coverage.observe(&record);
                    record_count += 1;
                    previous_observed_micros = Some(record.observed_micros);
                }
            }
        }

        Ok(JsonlJournalSummary {
            session: session.ok_or(JsonlJournalError::MissingSession)?,
            coverage,
            record_count,
        })
    }
}

fn write_line(writer: &mut impl Write, line: &JsonlJournalLine) -> Result<(), JsonlJournalError> {
    serde_json::to_writer(&mut *writer, line)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn read_limited_line(
    reader: &mut impl BufRead,
    line: &mut String,
    line_number: usize,
    max_line_bytes: usize,
) -> Result<usize, JsonlJournalError> {
    let limit = max_line_bytes.saturating_add(1) as u64;
    let mut limited = std::io::Read::take(reader, limit);
    let bytes_read = limited.read_line(line)?;
    if bytes_read > max_line_bytes {
        return Err(JsonlJournalError::LineTooLong {
            line: line_number,
            max_bytes: max_line_bytes,
        });
    }
    Ok(bytes_read)
}

#[derive(Debug, Error)]
pub enum JsonlJournalError {
    #[error("journal I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("journal serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("journal line {line} is invalid JSON: {source}")]
    InvalidJson {
        line: usize,
        source: serde_json::Error,
    },
    #[error("journal contains a second session header on line {line}")]
    DuplicateSession { line: usize },
    #[error("journal record appears before its session header on line {line}")]
    RecordBeforeSession { line: usize },
    #[error("journal has no session header")]
    MissingSession,
    #[error("journal line {line} exceeds the {max_bytes}-byte safety limit")]
    LineTooLong { line: usize, max_bytes: usize },
    #[error("journal sequence space is exhausted")]
    SequenceExhausted,
    #[error(transparent)]
    Journal(#[from] JournalError),
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use crate::{
        CaptureAdapter, CaptureGap, CaptureGapKind, CaptureRecordKind, GameBuild, PacketDirection,
        PacketEnvelope, PacketPayload, RouteKey, RoutedMessage,
    };

    use super::*;

    fn session() -> CaptureSession {
        CaptureSession {
            format_version: 1,
            capture_id: "stream-test".into(),
            started_unix_micros: Some(100),
            game_build: GameBuild {
                deployment_id: "global".into(),
                region_id: Some("north-america".into()),
                channel: "steam".into(),
                build_id: "test".into(),
                executable_version: None,
            },
            adapter: CaptureAdapter {
                name: "fixture".into(),
                version: None,
            },
            protocol_pack_digest: None,
        }
    }

    fn packet(observed_micros: u64, method_id: u32) -> CaptureRecordDraft {
        CaptureRecordDraft {
            observed_micros,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 1,
                stream_id: 2,
                source: None,
                destination: None,
                direction: PacketDirection::ServerToClient,
                fragment: Some(crate::FragmentKind::Notify),
                route: Some(RoutedMessage {
                    key: RouteKey::new(
                        PacketDirection::ServerToClient,
                        crate::FragmentKind::Notify,
                        10,
                        method_id,
                    ),
                    stub_id: 0,
                    call_id: None,
                }),
                compression: crate::CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: vec![1, 2, 3],
                    application_bytes: Some(vec![3]),
                },
            }),
        }
    }

    #[test]
    fn streaming_journal_round_trips_and_tracks_coverage() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = JsonlJournalWriter::new(cursor, session()).unwrap();
        writer.append(packet(10, 1)).unwrap();
        writer.append(packet(20, 2)).unwrap();
        writer
            .append(CaptureRecordDraft {
                observed_micros: 30,
                wall_clock_unix_micros: None,
                kind: CaptureRecordKind::Gap(CaptureGap {
                    kind: CaptureGapKind::QueueDrop,
                    connection_id: None,
                    stream_id: None,
                    lost_bytes: Some(50),
                    detail: "fixture gap".into(),
                }),
            })
            .unwrap();

        assert_eq!(writer.coverage().packet_count, 2);
        assert_eq!(writer.coverage().gap_count, 1);
        assert_eq!(writer.coverage().routes().len(), 2);

        let bytes = writer.into_inner().into_inner();
        let journal = JsonlJournalReader::new(BufReader::new(Cursor::new(bytes.clone())))
            .read()
            .unwrap();

        assert_eq!(journal.session(), &session());
        assert_eq!(journal.len(), 3);
        assert_eq!(journal.validate(), Ok(()));

        let summary = JsonlJournalReader::new(BufReader::new(Cursor::new(bytes)))
            .summarize()
            .unwrap();
        assert_eq!(summary.session, session());
        assert_eq!(summary.record_count, 3);
        assert_eq!(summary.coverage.packet_count, 2);
        assert_eq!(summary.coverage.gap_count, 1);
    }

    #[test]
    fn backward_time_is_rejected_before_a_line_is_written() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = JsonlJournalWriter::new(cursor, session()).unwrap();
        writer.append(packet(20, 1)).unwrap();
        let previous_len = writer.writer.get_ref().len();

        assert!(matches!(
            writer.append(packet(19, 2)),
            Err(JsonlJournalError::Journal(
                JournalError::ObservedTimeMovedBackward { .. }
            ))
        ));
        assert_eq!(writer.writer.get_ref().len(), previous_len);
    }

    #[test]
    fn reader_rejects_lines_over_the_configured_limit() {
        let bytes = vec![b'x'; 17];
        let error = JsonlJournalReader::with_max_line_bytes(BufReader::new(Cursor::new(bytes)), 16)
            .summarize()
            .unwrap_err();

        assert!(matches!(
            error,
            JsonlJournalError::LineTooLong {
                line: 1,
                max_bytes: 16
            }
        ));
    }
}
