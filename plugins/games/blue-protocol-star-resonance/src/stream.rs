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

#[derive(Serialize)]
#[serde(tag = "line", content = "data", rename_all = "snake_case")]
enum BorrowedJsonlJournalLine<'a> {
    Record(BorrowedCaptureRecord<'a>),
}

#[derive(Serialize)]
struct BorrowedCaptureRecord<'a> {
    sequence: u64,
    observed_micros: u64,
    wall_clock_unix_micros: Option<i64>,
    kind: &'a crate::CaptureRecordKind,
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
        let record = CaptureRecord {
            sequence: self.next_sequence,
            observed_micros: draft.observed_micros,
            wall_clock_unix_micros: draft.wall_clock_unix_micros,
            kind: draft.kind,
        };
        self.append_record(&record)
    }

    /// Appends an already sequenced capture record without cloning its packet payload.
    ///
    /// The journal owns its compact sequence space, so callers may selectively
    /// retain records from a broader live stream without creating sequence gaps.
    pub fn append_record(&mut self, record: &CaptureRecord) -> Result<u64, JsonlJournalError> {
        if let Some(previous_micros) = self.last_observed_micros {
            if record.observed_micros < previous_micros {
                return Err(JournalError::ObservedTimeMovedBackward {
                    previous_micros,
                    next_micros: record.observed_micros,
                }
                .into());
            }
        }
        if self.next_sequence == u64::MAX {
            return Err(JsonlJournalError::SequenceExhausted);
        }

        let sequence = self.next_sequence;
        write_line(
            &mut self.writer,
            &BorrowedJsonlJournalLine::Record(BorrowedCaptureRecord {
                sequence,
                observed_micros: record.observed_micros,
                wall_clock_unix_micros: record.wall_clock_unix_micros,
                kind: &record.kind,
            }),
        )?;

        self.coverage.observe(record);
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

/// Validated, memory-bounded reader over one research journal.
///
/// The session header is retained once while packet payloads are yielded one
/// record at a time. This is the preferred input for long-running protocol
/// correlation jobs; callers do not need to materialize a `ProtocolJournal`
/// merely to inspect every record.
pub struct JsonlJournalRecordStream<R: BufRead> {
    reader: R,
    max_line_bytes: usize,
    session: CaptureSession,
    line: String,
    line_number: usize,
    record_count: u64,
    previous_observed_micros: Option<u64>,
    coverage: CoverageReport,
    finished: bool,
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

    pub fn read(self) -> Result<ProtocolJournal, JsonlJournalError> {
        let mut stream = self.into_record_stream()?;
        let mut journal = ProtocolJournal::new(stream.session().clone());
        while let Some(record) = stream.next_record()? {
            journal.push(CaptureRecordDraft {
                observed_micros: record.observed_micros,
                wall_clock_unix_micros: record.wall_clock_unix_micros,
                kind: record.kind,
            })?;
        }
        Ok(journal)
    }

    pub fn summarize(self) -> Result<JsonlJournalSummary, JsonlJournalError> {
        let mut stream = self.into_record_stream()?;
        while stream.next_record()?.is_some() {}
        Ok(stream.summary())
    }

    pub fn into_record_stream(mut self) -> Result<JsonlJournalRecordStream<R>, JsonlJournalError> {
        let mut line = String::new();
        let mut line_number = 0usize;

        loop {
            line.clear();
            let bytes_read = read_limited_line(
                &mut self.reader,
                &mut line,
                line_number + 1,
                self.max_line_bytes,
            )?;
            if bytes_read == 0 {
                return Err(JsonlJournalError::MissingSession);
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
                    return Ok(JsonlJournalRecordStream {
                        reader: self.reader,
                        max_line_bytes: self.max_line_bytes,
                        session,
                        line,
                        line_number,
                        record_count: 0,
                        previous_observed_micros: None,
                        coverage: CoverageReport::default(),
                        finished: false,
                    });
                }
                JsonlJournalLine::Record(_) => {
                    return Err(JsonlJournalError::RecordBeforeSession { line: line_number });
                }
            }
        }
    }
}

impl<R: BufRead> JsonlJournalRecordStream<R> {
    pub fn session(&self) -> &CaptureSession {
        &self.session
    }

    pub fn record_count(&self) -> u64 {
        self.record_count
    }

    pub fn coverage(&self) -> &CoverageReport {
        &self.coverage
    }

    /// Describes an unterminated final line after a JSON end-of-input error.
    ///
    /// This is intentionally narrow: research replay may account for a capture
    /// process that stopped mid-write, but must not suppress malformed complete
    /// lines or corruption in the middle of a journal.
    pub fn truncated_tail(&self) -> Option<(usize, usize, u64)> {
        if self.finished || self.line.ends_with('\n') || self.line.trim().is_empty() {
            return None;
        }
        Some((
            self.line_number,
            self.line.len(),
            self.previous_observed_micros.unwrap_or_default(),
        ))
    }

    pub fn next_record(&mut self) -> Result<Option<CaptureRecord>, JsonlJournalError> {
        if self.finished {
            return Ok(None);
        }

        loop {
            self.line.clear();
            let bytes_read = read_limited_line(
                &mut self.reader,
                &mut self.line,
                self.line_number + 1,
                self.max_line_bytes,
            )?;
            if bytes_read == 0 {
                self.finished = true;
                return Ok(None);
            }
            self.line_number += 1;
            if self.line.trim().is_empty() {
                continue;
            }

            let journal_line: JsonlJournalLine =
                serde_json::from_str(&self.line).map_err(|source| {
                    JsonlJournalError::InvalidJson {
                        line: self.line_number,
                        source,
                    }
                })?;
            let JsonlJournalLine::Record(record) = journal_line else {
                return Err(JsonlJournalError::DuplicateSession {
                    line: self.line_number,
                });
            };

            let expected_sequence = self.record_count + 1;
            if record.sequence != expected_sequence {
                return Err(JournalError::InvalidSequence {
                    expected: expected_sequence,
                    actual: record.sequence,
                }
                .into());
            }
            if let Some(previous_micros) = self.previous_observed_micros {
                if record.observed_micros < previous_micros {
                    return Err(JournalError::ObservedTimeMovedBackward {
                        previous_micros,
                        next_micros: record.observed_micros,
                    }
                    .into());
                }
            }

            self.coverage.observe(&record);
            self.record_count += 1;
            self.previous_observed_micros = Some(record.observed_micros);
            return Ok(Some(record));
        }
    }

    pub fn summary(&self) -> JsonlJournalSummary {
        JsonlJournalSummary {
            session: self.session.clone(),
            coverage: self.coverage.clone(),
            record_count: self.record_count,
        }
    }
}

fn write_line(writer: &mut impl Write, line: &impl Serialize) -> Result<(), JsonlJournalError> {
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
    fn borrowed_records_are_resequenced_without_cloning_source_identity() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = JsonlJournalWriter::new(cursor, session()).unwrap();
        let first_draft = packet(10, 1);
        let first = CaptureRecord {
            sequence: 41,
            observed_micros: first_draft.observed_micros,
            wall_clock_unix_micros: first_draft.wall_clock_unix_micros,
            kind: first_draft.kind,
        };
        let second_draft = packet(20, 2);
        let second = CaptureRecord {
            sequence: 99,
            observed_micros: second_draft.observed_micros,
            wall_clock_unix_micros: second_draft.wall_clock_unix_micros,
            kind: second_draft.kind,
        };

        assert_eq!(writer.append_record(&first).unwrap(), 1);
        assert_eq!(writer.append_record(&second).unwrap(), 2);

        let bytes = writer.into_inner().into_inner();
        let journal = JsonlJournalReader::new(BufReader::new(Cursor::new(bytes)))
            .read()
            .unwrap();
        assert_eq!(journal.records()[0].sequence, 1);
        assert_eq!(journal.records()[1].sequence, 2);
        assert_eq!(journal.records()[0].observed_micros, 10);
        assert_eq!(journal.records()[1].observed_micros, 20);
    }

    #[test]
    fn record_stream_is_incremental_and_tracks_bounded_coverage() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = JsonlJournalWriter::new(cursor, session()).unwrap();
        writer.append(packet(10, 1)).unwrap();
        writer.append(packet(20, 2)).unwrap();
        let bytes = writer.into_inner().into_inner();

        let mut stream = JsonlJournalReader::new(BufReader::new(Cursor::new(bytes)))
            .into_record_stream()
            .unwrap();
        assert_eq!(stream.session(), &session());
        assert_eq!(stream.record_count(), 0);
        assert_eq!(stream.coverage().packet_count, 0);

        let first = stream.next_record().unwrap().unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(stream.record_count(), 1);
        assert_eq!(stream.coverage().packet_count, 1);

        let second = stream.next_record().unwrap().unwrap();
        assert_eq!(second.sequence, 2);
        assert_eq!(stream.record_count(), 2);
        assert_eq!(stream.coverage().packet_count, 2);
        assert!(stream.next_record().unwrap().is_none());
        assert!(stream.next_record().unwrap().is_none());

        let summary = stream.summary();
        assert_eq!(summary.session, session());
        assert_eq!(summary.record_count, 2);
        assert_eq!(summary.coverage.routes().len(), 2);
    }

    #[test]
    fn record_stream_rejects_a_second_session_header() {
        let mut bytes = Vec::new();
        write_line(&mut bytes, &JsonlJournalLine::Session(session())).unwrap();
        write_line(&mut bytes, &JsonlJournalLine::Session(session())).unwrap();

        let mut stream = JsonlJournalReader::new(BufReader::new(Cursor::new(bytes)))
            .into_record_stream()
            .unwrap();
        assert!(matches!(
            stream.next_record(),
            Err(JsonlJournalError::DuplicateSession { line: 2 })
        ));
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

    #[test]
    fn record_stream_identifies_only_an_unterminated_json_tail() {
        let mut bytes = Vec::new();
        write_line(&mut bytes, &JsonlJournalLine::Session(session())).unwrap();
        bytes.extend_from_slice(br#"{"type":"record"#);

        let mut stream = JsonlJournalReader::new(BufReader::new(Cursor::new(bytes)))
            .into_record_stream()
            .unwrap();
        let error = stream.next_record().unwrap_err();

        assert!(matches!(
            error,
            JsonlJournalError::InvalidJson { line: 2, ref source } if source.is_eof()
        ));
        assert_eq!(stream.truncated_tail(), Some((2, 15, 0)));
    }

    #[test]
    fn record_stream_does_not_treat_a_complete_bad_line_as_a_truncated_tail() {
        let mut bytes = Vec::new();
        write_line(&mut bytes, &JsonlJournalLine::Session(session())).unwrap();
        bytes.extend_from_slice(b"not-json\n");

        let mut stream = JsonlJournalReader::new(BufReader::new(Cursor::new(bytes)))
            .into_record_stream()
            .unwrap();
        assert!(matches!(
            stream.next_record(),
            Err(JsonlJournalError::InvalidJson { line: 2, .. })
        ));
        assert_eq!(stream.truncated_tail(), None);
    }
}
