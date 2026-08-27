//! Versioned, sealed, streaming `.rlog` files containing canonical events.

use std::io::{BufRead, Cursor, Read, Write};

use rlogs_events::{
    CanonicalEvent, EVENT_SCHEMA_VERSION, EventEnvelope, EventSensitivity, RegionContext,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const RLOG_SCHEMA_VERSION: u16 = 2;
pub const LEGACY_RLOG_SCHEMA_VERSION: u16 = 1;
pub const MINIMUM_SUPPORTED_EVENT_SCHEMA_VERSION: u16 = 1;
pub const DEFAULT_MAXIMUM_LINE_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_MAXIMUM_EVENTS: u64 = 2_000_000;
pub const DEFAULT_MAXIMUM_BLOCK_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_MAXIMUM_BLOCK_EVENTS: u32 = 512;

const RLOG_V2_MAGIC: &[u8; 8] = b"RLOG\x02\r\n\x1a";
const RLOG_V2_EVENT_BLOCK: u8 = 1;
const RLOG_V2_SEAL: u8 = 255;
const WRITER_BLOCK_TARGET_BYTES: usize = 512 * 1024;
const WRITER_BLOCK_TARGET_EVENTS: u32 = 256;
const ZSTD_COMPRESSION_LEVEL: i32 = 3;

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
        if !(LEGACY_RLOG_SCHEMA_VERSION..=RLOG_SCHEMA_VERSION).contains(&self.schema_version) {
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
    pub maximum_block_bytes: usize,
    pub maximum_block_events: u32,
}

impl Default for RlogLimits {
    fn default() -> Self {
        Self {
            maximum_line_bytes: DEFAULT_MAXIMUM_LINE_BYTES,
            maximum_events: DEFAULT_MAXIMUM_EVENTS,
            maximum_block_bytes: DEFAULT_MAXIMUM_BLOCK_BYTES,
            maximum_block_events: DEFAULT_MAXIMUM_BLOCK_EVENTS,
        }
    }
}

impl RlogLimits {
    fn validate(self) -> Result<Self, RlogError> {
        if self.maximum_line_bytes == 0
            || self.maximum_events == 0
            || self.maximum_block_bytes == 0
            || self.maximum_block_events == 0
        {
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
    encoding: RlogEncoding,
    block: Vec<u8>,
    block_event_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RlogEncoding {
    LegacyJsonLines,
    CompactBlocks,
}

impl<W: Write> RlogWriter<W> {
    pub fn new(mut output: W, header: RlogHeader) -> Result<Self, RlogError> {
        header.validate()?;
        let encoding = if header.schema_version == LEGACY_RLOG_SCHEMA_VERSION {
            write_record(
                &mut output,
                &RlogRecord::Header {
                    header: header.clone(),
                },
            )?;
            RlogEncoding::LegacyJsonLines
        } else {
            write_compact_header(&mut output, &header)?;
            RlogEncoding::CompactBlocks
        };
        Ok(Self {
            output,
            header,
            hasher: Sha256::new(),
            event_count: 0,
            previous_sequence: None,
            previous_timeline_sequence: None,
            previous_observed_micros: None,
            encoding,
            block: Vec::with_capacity(WRITER_BLOCK_TARGET_BYTES),
            block_event_count: 0,
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
        match self.encoding {
            RlogEncoding::LegacyJsonLines => {
                write_event_record(&mut self.output, &mut self.hasher, envelope)?;
            }
            RlogEncoding::CompactBlocks => self.push_compact(envelope)?,
        }
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
        if self.encoding == RlogEncoding::CompactBlocks {
            self.flush_compact_block()?;
        }
        let content_sha256 = digest_string(self.hasher.finalize());
        let seal = RlogSeal {
            event_count: self.event_count,
            content_sha256,
        };
        match self.encoding {
            RlogEncoding::LegacyJsonLines => {
                write_record(&mut self.output, &RlogRecord::Seal { seal: seal.clone() })?;
            }
            RlogEncoding::CompactBlocks => write_compact_seal(&mut self.output, &seal)?,
        }
        self.output.flush()?;
        Ok((self.output, seal))
    }

    fn push_compact(&mut self, envelope: &EventEnvelope) -> Result<(), RlogError> {
        let encoded = serde_json::to_vec(envelope)?;
        if encoded.len() > DEFAULT_MAXIMUM_LINE_BYTES {
            return Err(RlogError::EventTooLarge {
                actual: encoded.len(),
                maximum: DEFAULT_MAXIMUM_LINE_BYTES,
            });
        }
        let encoded_with_newline = encoded.len().saturating_add(1);
        if self.block_event_count > 0
            && (self.block.len().saturating_add(encoded_with_newline) > WRITER_BLOCK_TARGET_BYTES
                || self.block_event_count >= WRITER_BLOCK_TARGET_EVENTS)
        {
            self.flush_compact_block()?;
        }
        self.hasher.update(&encoded);
        self.hasher.update(b"\n");
        self.block.extend_from_slice(&encoded);
        self.block.push(b'\n');
        self.block_event_count = self
            .block_event_count
            .checked_add(1)
            .ok_or(RlogError::BlockEventCountOverflow)?;
        Ok(())
    }

    fn flush_compact_block(&mut self) -> Result<(), RlogError> {
        if self.block_event_count == 0 {
            return Ok(());
        }
        write_compact_event_block(
            &mut self.output,
            self.block_event_count,
            self.block.as_slice(),
        )?;
        self.block.clear();
        self.block_event_count = 0;
        Ok(())
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
    encoding: RlogEncoding,
    block: Vec<u8>,
    block_offset: usize,
    block_events_remaining: u32,
}

impl<R: BufRead> RlogReader<R> {
    pub fn new(mut input: R, limits: RlogLimits) -> Result<Self, RlogError> {
        let limits = limits.validate()?;
        let first = input
            .fill_buf()?
            .first()
            .copied()
            .ok_or(RlogError::MissingHeader)?;
        let (encoding, header) = if first == RLOG_V2_MAGIC[0] {
            let mut magic = [0_u8; RLOG_V2_MAGIC.len()];
            input.read_exact(&mut magic)?;
            if &magic != RLOG_V2_MAGIC {
                return Err(RlogError::InvalidCompactMagic);
            }
            let header_bytes = read_length_prefixed_bytes(
                &mut input,
                limits.maximum_line_bytes,
                "compact header",
            )?;
            let header: RlogHeader = serde_json::from_slice(&header_bytes)?;
            if header.schema_version != RLOG_SCHEMA_VERSION {
                return Err(RlogError::EncodingSchemaMismatch {
                    encoding: "compact",
                    actual: header.schema_version,
                });
            }
            (RlogEncoding::CompactBlocks, header)
        } else {
            let line = read_bounded_line(&mut input, limits.maximum_line_bytes, 1)?
                .ok_or(RlogError::MissingHeader)?;
            let record = parse_record(&line, 1)?;
            let RlogRecord::Header { header } = record else {
                return Err(RlogError::HeaderMustBeFirst);
            };
            if header.schema_version != LEGACY_RLOG_SCHEMA_VERSION {
                return Err(RlogError::EncodingSchemaMismatch {
                    encoding: "json-lines",
                    actual: header.schema_version,
                });
            }
            (RlogEncoding::LegacyJsonLines, header)
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
            encoding,
            block: Vec::new(),
            block_offset: 0,
            block_events_remaining: 0,
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
        match self.encoding {
            RlogEncoding::LegacyJsonLines => self.next_legacy_event(),
            RlogEncoding::CompactBlocks => self.next_compact_event(),
        }
    }

    fn next_legacy_event(&mut self) -> Result<Option<EventEnvelope>, RlogError> {
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
                let encoded = legacy_event_envelope_bytes(&line, self.line_number)?;
                let next_hasher = digest_after_encoded_event(&self.hasher, encoded);
                self.accept_event(*envelope, next_hasher).map(Some)
            }
            RlogRecord::Seal { seal } => self.finish_replay(seal, |reader| {
                ensure_end_of_file(
                    &mut reader.input,
                    reader.limits.maximum_line_bytes,
                    reader.line_number,
                )
            }),
        }
    }

    fn next_compact_event(&mut self) -> Result<Option<EventEnvelope>, RlogError> {
        loop {
            if self.block_events_remaining > 0 {
                let relative_end = self.block[self.block_offset..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .ok_or(RlogError::CompactBlockMissingDelimiter)?;
                let end = self.block_offset.saturating_add(relative_end);
                let event_bytes = &self.block[self.block_offset..end];
                if event_bytes.is_empty() {
                    return Err(RlogError::CompactBlockEmptyEvent);
                }
                if event_bytes.len() > self.limits.maximum_line_bytes {
                    return Err(RlogError::LineTooLong {
                        line: self.event_count.saturating_add(1),
                        maximum: self.limits.maximum_line_bytes,
                    });
                }
                let next_hasher = digest_after_encoded_event(&self.hasher, event_bytes);
                let envelope: EventEnvelope =
                    serde_json::from_slice(event_bytes).map_err(|source| {
                        RlogError::InvalidRecord {
                            line: self.event_count.saturating_add(1),
                            source,
                        }
                    })?;
                self.block_offset = end.saturating_add(1);
                self.block_events_remaining -= 1;
                if self.block_events_remaining == 0 && self.block_offset != self.block.len() {
                    return Err(RlogError::CompactBlockEventCountMismatch);
                }
                return self.accept_event(envelope, next_hasher).map(Some);
            }

            let Some(tag) = read_optional_byte(&mut self.input)? else {
                return Err(RlogError::MissingSeal);
            };
            match tag {
                RLOG_V2_EVENT_BLOCK => self.read_compact_block()?,
                RLOG_V2_SEAL => {
                    let seal_bytes = read_length_prefixed_bytes(
                        &mut self.input,
                        self.limits.maximum_line_bytes,
                        "compact seal",
                    )?;
                    let seal: RlogSeal = serde_json::from_slice(&seal_bytes)?;
                    return self.finish_replay(seal, |reader| {
                        ensure_binary_end_of_file(&mut reader.input)
                    });
                }
                actual => return Err(RlogError::UnknownCompactRecord { actual }),
            }
        }
    }

    fn read_compact_block(&mut self) -> Result<(), RlogError> {
        let declared_events = read_u32(&mut self.input, "compact block event count")?;
        let uncompressed_bytes = usize::try_from(read_u32(
            &mut self.input,
            "compact block uncompressed length",
        )?)
        .map_err(|_| RlogError::CompactLengthOverflow)?;
        let compressed_bytes = usize::try_from(read_u32(
            &mut self.input,
            "compact block compressed length",
        )?)
        .map_err(|_| RlogError::CompactLengthOverflow)?;
        if declared_events == 0 || declared_events > self.limits.maximum_block_events {
            return Err(RlogError::CompactBlockEventLimitExceeded {
                actual: declared_events,
                maximum: self.limits.maximum_block_events,
            });
        }
        if uncompressed_bytes == 0 || uncompressed_bytes > self.limits.maximum_block_bytes {
            return Err(RlogError::CompactBlockSizeLimitExceeded {
                kind: "uncompressed",
                actual: uncompressed_bytes,
                maximum: self.limits.maximum_block_bytes,
            });
        }
        if compressed_bytes == 0 || compressed_bytes > self.limits.maximum_block_bytes {
            return Err(RlogError::CompactBlockSizeLimitExceeded {
                kind: "compressed",
                actual: compressed_bytes,
                maximum: self.limits.maximum_block_bytes,
            });
        }
        if self.event_count.saturating_add(u64::from(declared_events)) > self.limits.maximum_events
        {
            return Err(RlogError::EventLimitExceeded {
                maximum: self.limits.maximum_events,
            });
        }
        let mut compressed = vec![0_u8; compressed_bytes];
        self.input.read_exact(&mut compressed)?;
        let decoder = zstd::stream::read::Decoder::new(Cursor::new(compressed))?;
        let mut limited = decoder.take(
            u64::try_from(uncompressed_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        );
        self.block.clear();
        limited.read_to_end(&mut self.block)?;
        if self.block.len() != uncompressed_bytes {
            return Err(RlogError::CompactBlockLengthMismatch {
                declared: uncompressed_bytes,
                actual: self.block.len(),
            });
        }
        let actual_events = self.block.iter().filter(|byte| **byte == b'\n').count();
        if self.block.last() != Some(&b'\n')
            || actual_events != usize::try_from(declared_events).unwrap_or(usize::MAX)
        {
            return Err(RlogError::CompactBlockEventCountMismatch);
        }
        self.block_offset = 0;
        self.block_events_remaining = declared_events;
        Ok(())
    }

    fn accept_event(
        &mut self,
        envelope: EventEnvelope,
        next_hasher: Sha256,
    ) -> Result<EventEnvelope, RlogError> {
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
        // The seal covers the exact envelope JSON stored in the file. Hashing
        // a deserialized envelope again would silently add fields introduced
        // by a newer event schema and invalidate otherwise untouched legacy
        // captures. Parsing and all canonical validation still happen above;
        // only the integrity input stays byte-for-byte stable.
        self.hasher = next_hasher;
        self.event_count += 1;
        self.previous_sequence = Some(envelope.sequence);
        if let CanonicalEvent::Timeline(event) = &envelope.event {
            self.previous_timeline_sequence = Some(event.sequence);
        }
        self.previous_observed_micros = Some(envelope.time.observed_micros);
        self.first_observed_micros
            .get_or_insert(envelope.time.observed_micros);
        Ok(envelope)
    }

    fn finish_replay(
        &mut self,
        seal: RlogSeal,
        ensure_end: impl FnOnce(&mut Self) -> Result<(), RlogError>,
    ) -> Result<Option<EventEnvelope>, RlogError> {
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
        ensure_end(self)?;
        self.finished = true;
        self.summary = Some(RlogReplaySummary {
            event_count: self.event_count,
            first_observed_micros: self.first_observed_micros,
            last_observed_micros: self.previous_observed_micros,
            content_sha256: actual_digest,
        });
        Ok(None)
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

fn digest_after_encoded_event(hasher: &Sha256, encoded: &[u8]) -> Sha256 {
    let mut next = hasher.clone();
    next.update(encoded);
    next.update(b"\n");
    next
}

fn legacy_event_envelope_bytes(line: &[u8], line_number: u64) -> Result<&[u8], RlogError> {
    const PREFIX: &[u8] = br#"{"record":"event","envelope":"#;
    line.strip_prefix(PREFIX)
        .and_then(|encoded| encoded.strip_suffix(b"}"))
        .ok_or(RlogError::LegacyEventEncoding { line: line_number })
}

fn digest_string(digest: impl std::fmt::LowerHex) -> String {
    format!("sha256:{digest:x}")
}

fn write_record(output: &mut impl Write, record: &RlogRecord) -> Result<(), RlogError> {
    serde_json::to_writer(&mut *output, record)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn write_compact_header(output: &mut impl Write, header: &RlogHeader) -> Result<(), RlogError> {
    output.write_all(RLOG_V2_MAGIC)?;
    write_length_prefixed_json(output, header)
}

fn write_compact_event_block(
    output: &mut impl Write,
    event_count: u32,
    uncompressed: &[u8],
) -> Result<(), RlogError> {
    let uncompressed_length =
        u32::try_from(uncompressed.len()).map_err(|_| RlogError::CompactLengthOverflow)?;
    let compressed = zstd::stream::encode_all(Cursor::new(uncompressed), ZSTD_COMPRESSION_LEVEL)?;
    let compressed_length =
        u32::try_from(compressed.len()).map_err(|_| RlogError::CompactLengthOverflow)?;
    output.write_all(&[RLOG_V2_EVENT_BLOCK])?;
    output.write_all(&event_count.to_le_bytes())?;
    output.write_all(&uncompressed_length.to_le_bytes())?;
    output.write_all(&compressed_length.to_le_bytes())?;
    output.write_all(&compressed)?;
    Ok(())
}

fn write_compact_seal(output: &mut impl Write, seal: &RlogSeal) -> Result<(), RlogError> {
    output.write_all(&[RLOG_V2_SEAL])?;
    write_length_prefixed_json(output, seal)
}

fn write_length_prefixed_json(
    output: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), RlogError> {
    let encoded = serde_json::to_vec(value)?;
    let length = u32::try_from(encoded.len()).map_err(|_| RlogError::CompactLengthOverflow)?;
    output.write_all(&length.to_le_bytes())?;
    output.write_all(&encoded)?;
    Ok(())
}

fn read_u32(input: &mut impl Read, context: &'static str) -> Result<u32, RlogError> {
    let mut bytes = [0_u8; 4];
    input
        .read_exact(&mut bytes)
        .map_err(|source| RlogError::TruncatedCompactRecord { context, source })?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_length_prefixed_bytes(
    input: &mut impl Read,
    maximum: usize,
    context: &'static str,
) -> Result<Vec<u8>, RlogError> {
    let length =
        usize::try_from(read_u32(input, context)?).map_err(|_| RlogError::CompactLengthOverflow)?;
    if length == 0 || length > maximum {
        return Err(RlogError::CompactRecordSizeLimitExceeded {
            context,
            actual: length,
            maximum,
        });
    }
    let mut bytes = vec![0_u8; length];
    input
        .read_exact(&mut bytes)
        .map_err(|source| RlogError::TruncatedCompactRecord { context, source })?;
    Ok(bytes)
}

fn read_optional_byte(input: &mut impl Read) -> Result<Option<u8>, RlogError> {
    let mut byte = [0_u8; 1];
    match input.read(&mut byte)? {
        0 => Ok(None),
        1 => Ok(Some(byte[0])),
        _ => unreachable!("a one-byte read cannot return more than one byte"),
    }
}

fn ensure_binary_end_of_file(input: &mut impl Read) -> Result<(), RlogError> {
    if read_optional_byte(input)?.is_some() {
        return Err(RlogError::RecordAfterSeal);
    }
    Ok(())
}

/// Writes the event wrapper while serializing the envelope exactly once.
///
/// The canonical content digest covers the envelope JSON, not the surrounding
/// rlog record. Hashing through this writer preserves that contract without
/// allocating a second JSON buffer or cloning the full event.
fn write_event_record(
    output: &mut impl Write,
    hasher: &mut Sha256,
    envelope: &EventEnvelope,
) -> Result<(), RlogError> {
    output.write_all(br#"{"record":"event","envelope":"#)?;
    {
        let mut digesting_output = DigestingWriter { output, hasher };
        serde_json::to_writer(&mut digesting_output, envelope)?;
    }
    output.write_all(b"}\n")?;
    hasher.update(b"\n");
    Ok(())
}

struct DigestingWriter<'a, W> {
    output: &'a mut W,
    hasher: &'a mut Sha256,
}

impl<W: Write> Write for DigestingWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.output.write_all(bytes)?;
        self.hasher.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
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

    #[error("legacy rlog event on line {line} does not use the canonical envelope wrapper")]
    LegacyEventEncoding { line: u64 },

    #[error("rlog compact container magic is invalid")]
    InvalidCompactMagic,

    #[error("rlog {encoding} encoding cannot carry schema version {actual}")]
    EncodingSchemaMismatch { encoding: &'static str, actual: u16 },

    #[error("rlog compact record {context} is truncated: {source}")]
    TruncatedCompactRecord {
        context: &'static str,
        source: std::io::Error,
    },

    #[error("rlog compact length does not fit the container")]
    CompactLengthOverflow,

    #[error("rlog compact record {context} is {actual} bytes; maximum is {maximum}")]
    CompactRecordSizeLimitExceeded {
        context: &'static str,
        actual: usize,
        maximum: usize,
    },

    #[error("rlog compact event block contains {actual} events; maximum is {maximum}")]
    CompactBlockEventLimitExceeded { actual: u32, maximum: u32 },

    #[error("rlog compact {kind} block is {actual} bytes; maximum is {maximum}")]
    CompactBlockSizeLimitExceeded {
        kind: &'static str,
        actual: usize,
        maximum: usize,
    },

    #[error("rlog compact block declared {declared} bytes but decoded {actual}")]
    CompactBlockLengthMismatch { declared: usize, actual: usize },

    #[error("rlog compact block event count does not match its payload")]
    CompactBlockEventCountMismatch,

    #[error("rlog compact block is missing an event delimiter")]
    CompactBlockMissingDelimiter,

    #[error("rlog compact block contains an empty event")]
    CompactBlockEmptyEvent,

    #[error("rlog compact record tag {actual} is unknown")]
    UnknownCompactRecord { actual: u8 },

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

    #[error("rlog compact block event count space is exhausted")]
    BlockEventCountOverflow,

    #[error("canonical event is {actual} bytes; maximum is {maximum}")]
    EventTooLarge { actual: usize, maximum: usize },

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

    fn encoded_legacy_log() -> Vec<u8> {
        let mut header = RlogHeader::new("session-1", region(), "unit-test");
        header.schema_version = LEGACY_RLOG_SCHEMA_VERSION;
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        writer.push(&envelope(1, 10)).unwrap();
        writer.push(&envelope(2, 20)).unwrap();
        writer.finish().unwrap()
    }

    #[test]
    fn sealed_log_streams_and_verifies() {
        let bytes = encoded_log();
        assert!(bytes.starts_with(RLOG_V2_MAGIC));
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
    fn legacy_seal_uses_stored_envelope_bytes_across_schema_evolution() {
        let text = String::from_utf8(encoded_legacy_log()).unwrap();
        let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
        lines[1] = lines[1].replacen(
            r#"{"record":"event","envelope":{"#,
            r#"{"record":"event","envelope":{"legacy_extension":true,"#,
            1,
        );

        let mut hasher = Sha256::new();
        for (index, line) in lines[1..=2].iter().enumerate() {
            let encoded = legacy_event_envelope_bytes(line.as_bytes(), index as u64 + 2).unwrap();
            hasher.update(encoded);
            hasher.update(b"\n");
        }
        let seal = RlogSeal {
            event_count: 2,
            content_sha256: digest_string(hasher.finalize()),
        };
        lines[3] = serde_json::to_string(&RlogRecord::Seal { seal }).unwrap();
        let encoded = format!("{}\n", lines.join("\n")).into_bytes();

        let summary = RlogReader::new(BufReader::new(Cursor::new(encoded)), RlogLimits::default())
            .unwrap()
            .replay(|_| Ok(()))
            .unwrap();
        assert_eq!(summary.event_count, 2);
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
        let bytes = encoded_legacy_log();
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
                    ..RlogLimits::default()
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

    #[test]
    fn compact_encoding_preserves_every_event_and_canonical_digest() {
        let mut compact_writer = RlogWriter::new(
            Vec::new(),
            RlogHeader::new("session-1", region(), "unit-test"),
        )
        .unwrap();
        let mut legacy_header = RlogHeader::new("session-1", region(), "unit-test");
        legacy_header.schema_version = LEGACY_RLOG_SCHEMA_VERSION;
        let mut legacy_writer = RlogWriter::new(Vec::new(), legacy_header).unwrap();
        let mut expected = Vec::new();
        for sequence in 1..=600 {
            let event = envelope(sequence, sequence * 10);
            compact_writer.push(&event).unwrap();
            legacy_writer.push(&event).unwrap();
            expected.push(event);
        }
        let (compact, compact_seal) = compact_writer.finish_with_seal().unwrap();
        let (legacy, legacy_seal) = legacy_writer.finish_with_seal().unwrap();
        assert_eq!(compact_seal.event_count, 600);
        assert_eq!(compact_seal, legacy_seal);
        assert!(compact.len() < legacy.len() / 2);

        for encoded in [compact, legacy] {
            let mut actual = Vec::new();
            let summary =
                RlogReader::new(BufReader::new(Cursor::new(encoded)), RlogLimits::default())
                    .unwrap()
                    .replay(|event| {
                        actual.push(event.clone());
                        Ok(())
                    })
                    .unwrap();
            assert_eq!(actual, expected);
            assert_eq!(summary.event_count, 600);
            assert_eq!(summary.content_sha256, compact_seal.content_sha256);
        }
    }
}
