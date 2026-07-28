use std::{collections::VecDeque, io::Read};

use bytes::{Buf, Bytes, BytesMut};
use rlogs_network::{TcpFlowKey, TcpStreamChunk, TcpStreamGap};
use serde::{Deserialize, Serialize};

use crate::{CompressionState, FragmentKind, PacketDirection, RouteKey, RoutedMessage};

const FRAME_HEADER_BYTES: usize = 6;
const COMPRESSION_FLAG: u16 = 0x8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BpsrCallLayout {
    /// `[service:u64][stub:u32][call:u32][method:u32][body]`
    WithCallId,
    /// `[service:u64][stub:u32][method:u32][body]`
    WithoutCallId,
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BpsrReturnLayout {
    TwelveByteHeader,
    FourByteHeader,
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BpsrFrameUpLayout {
    /// Preserve the wrapper for a region/build protocol pack.
    Opaque,
    /// Four-byte wrapper prefix followed by ordinary nested BPSR frames.
    NestedAfterFourBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BpsrFramingConfig {
    pub max_frame_bytes: usize,
    pub max_buffered_bytes: usize,
    pub max_buffered_chunks: usize,
    pub max_decompressed_bytes: usize,
    pub max_nesting_depth: u8,
    /// Applied only while locating a boundary after capture starts mid-stream.
    pub max_resync_fragment_type: u16,
    pub call_layout: BpsrCallLayout,
    pub return_layout: BpsrReturnLayout,
    pub frame_up_layout: BpsrFrameUpLayout,
}

impl Default for BpsrFramingConfig {
    fn default() -> Self {
        Self {
            max_frame_bytes: 16 * 1024 * 1024,
            max_buffered_bytes: 16 * 1024 * 1024,
            max_buffered_chunks: 4_096,
            max_decompressed_bytes: 64 * 1024 * 1024,
            max_nesting_depth: 8,
            max_resync_fragment_type: 0x7fff,
            call_layout: BpsrCallLayout::WithCallId,
            return_layout: BpsrReturnLayout::TwelveByteHeader,
            frame_up_layout: BpsrFrameUpLayout::Opaque,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpsrFramingConfigError {
    FrameTooSmall,
    ZeroBufferedBytes,
    FrameExceedsBuffer,
    ZeroBufferedChunks,
    ZeroDecompressedBytes,
    ZeroNestingDepth,
}

impl std::fmt::Display for BpsrFramingConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FrameTooSmall => "max_frame_bytes must allow the six-byte frame header",
            Self::ZeroBufferedBytes => "max_buffered_bytes must be greater than zero",
            Self::FrameExceedsBuffer => "max_frame_bytes cannot exceed max_buffered_bytes",
            Self::ZeroBufferedChunks => "max_buffered_chunks must be greater than zero",
            Self::ZeroDecompressedBytes => "max_decompressed_bytes must be greater than zero",
            Self::ZeroNestingDepth => "max_nesting_depth must be greater than zero",
        })
    }
}

impl std::error::Error for BpsrFramingConfigError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BpsrFrame {
    pub flow: TcpFlowKey,
    pub direction: PacketDirection,
    pub stream_offset: u64,
    pub capture_sequence: u64,
    pub observed_micros: u64,
    pub nesting_depth: u8,
    pub fragment: FragmentKind,
    pub compressed_on_wire: bool,
    pub compression: CompressionState,
    pub route: Option<RoutedMessage>,
    /// Complete length-prefixed frame. Contiguous TCP frames remain shared
    /// slices; only frames crossing chunks are coalesced.
    pub wire_bytes: Bytes,
    /// Route-header-stripped body, decompressed when the wire flag is known.
    pub application_bytes: Option<Bytes>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", content = "data", rename_all = "snake_case")]
pub enum BpsrFramingIssueReason {
    StreamChanged,
    StreamDiscontinuity {
        expected_offset: u64,
        actual_offset: u64,
    },
    TcpGap,
    BufferLimit,
    ChunkLimit,
    InvalidFrameLength {
        declared_bytes: u32,
    },
    Resynchronized {
        discarded_bytes: u64,
    },
    MalformedRouteHeader,
    DecompressionFailed,
    DecompressedLimit,
    MalformedNestedFrames,
    NestingLimit,
    StreamLimit,
    TotalBufferLimit,
    IdleTimeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BpsrFramingIssue {
    pub flow: Option<TcpFlowKey>,
    pub stream_offset: u64,
    pub capture_sequence: u64,
    pub observed_micros: u64,
    pub discarded_bytes: u64,
    pub reason: BpsrFramingIssueReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum BpsrFramingEvent {
    Frame(BpsrFrame),
    Issue(BpsrFramingIssue),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BpsrFramingMetrics {
    pub chunks_seen: u64,
    pub stream_bytes_seen: u64,
    pub frames_emitted: u64,
    pub nested_frames_emitted: u64,
    pub wire_bytes_emitted: u64,
    pub cross_chunk_frames: u64,
    pub decompressions_succeeded: u64,
    pub decompressions_failed: u64,
    pub decompressed_bytes: u64,
    pub resynchronizations: u64,
    pub bytes_discarded: u64,
    pub buffered_chunks: u64,
    pub buffered_bytes: u64,
    pub buffered_bytes_high_water: u64,
}

#[derive(Debug)]
struct BufferedChunk {
    stream_offset: u64,
    capture_sequence: u64,
    observed_micros: u64,
    bytes: Bytes,
}

#[derive(Debug)]
struct CompleteFrame {
    stream_offset: u64,
    capture_sequence: u64,
    observed_micros: u64,
    crossed_chunks: bool,
    bytes: Bytes,
}

/// One allocation-conscious framer for one directional TCP stream.
#[derive(Debug)]
pub struct BpsrStreamFramer {
    config: BpsrFramingConfig,
    direction: PacketDirection,
    flow: Option<TcpFlowKey>,
    expected_stream_offset: Option<u64>,
    synchronized: bool,
    pending_resync_discarded: u64,
    chunks: VecDeque<BufferedChunk>,
    buffered_bytes: usize,
    metrics: BpsrFramingMetrics,
}

impl BpsrStreamFramer {
    pub fn new(direction: PacketDirection) -> Self {
        Self::try_with_config(direction, BpsrFramingConfig::default())
            .expect("the built-in BPSR framing configuration is valid")
    }

    pub fn try_with_config(
        direction: PacketDirection,
        config: BpsrFramingConfig,
    ) -> Result<Self, BpsrFramingConfigError> {
        validate_config(config)?;
        Ok(Self {
            config,
            direction,
            flow: None,
            expected_stream_offset: None,
            synchronized: false,
            pending_resync_discarded: 0,
            chunks: VecDeque::new(),
            buffered_bytes: 0,
            metrics: BpsrFramingMetrics::default(),
        })
    }

    pub fn config(&self) -> BpsrFramingConfig {
        self.config
    }

    pub fn metrics(&self) -> &BpsrFramingMetrics {
        &self.metrics
    }

    pub fn direction(&self) -> PacketDirection {
        self.direction
    }

    pub fn buffered_bytes(&self) -> usize {
        self.buffered_bytes
    }

    pub(crate) fn buffered_evidence_bytes(&self) -> u64 {
        (self.buffered_bytes as u64).saturating_add(self.pending_resync_discarded)
    }

    pub fn process(&mut self, chunk: TcpStreamChunk, mut emit: impl FnMut(BpsrFramingEvent)) {
        self.metrics.chunks_seen = self.metrics.chunks_seen.saturating_add(1);
        self.metrics.stream_bytes_seen = self
            .metrics
            .stream_bytes_seen
            .saturating_add(chunk.bytes.len() as u64);

        if chunk.bytes.is_empty() {
            return;
        }

        if self.flow.is_some_and(|flow| flow != chunk.flow) {
            self.discard_buffer(
                BpsrFramingIssueReason::StreamChanged,
                chunk.stream_offset,
                chunk.capture_sequence,
                chunk.observed_micros,
                &mut emit,
            );
            self.flow = None;
            self.expected_stream_offset = None;
        }

        if let Some(expected_offset) = self.expected_stream_offset {
            if chunk.stream_offset != expected_offset {
                self.discard_buffer(
                    BpsrFramingIssueReason::StreamDiscontinuity {
                        expected_offset,
                        actual_offset: chunk.stream_offset,
                    },
                    chunk.stream_offset,
                    chunk.capture_sequence,
                    chunk.observed_micros,
                    &mut emit,
                );
            }
        }

        self.flow = Some(chunk.flow);
        self.expected_stream_offset =
            Some(chunk.stream_offset.saturating_add(chunk.bytes.len() as u64));

        if self.chunks.len() >= self.config.max_buffered_chunks {
            self.discard_buffer(
                BpsrFramingIssueReason::ChunkLimit,
                chunk.stream_offset,
                chunk.capture_sequence,
                chunk.observed_micros,
                &mut emit,
            );
        }
        if self.buffered_bytes.saturating_add(chunk.bytes.len()) > self.config.max_buffered_bytes {
            self.discard_buffer(
                BpsrFramingIssueReason::BufferLimit,
                chunk.stream_offset,
                chunk.capture_sequence,
                chunk.observed_micros,
                &mut emit,
            );
        }
        if chunk.bytes.len() > self.config.max_buffered_bytes {
            self.observe_discard(chunk.bytes.len());
            emit(BpsrFramingEvent::Issue(BpsrFramingIssue {
                flow: Some(chunk.flow),
                stream_offset: chunk.stream_offset,
                capture_sequence: chunk.capture_sequence,
                observed_micros: chunk.observed_micros,
                discarded_bytes: chunk.bytes.len() as u64,
                reason: BpsrFramingIssueReason::BufferLimit,
            }));
            return;
        }

        self.buffered_bytes = self.buffered_bytes.saturating_add(chunk.bytes.len());
        self.chunks.push_back(BufferedChunk {
            stream_offset: chunk.stream_offset,
            capture_sequence: chunk.capture_sequence,
            observed_micros: chunk.observed_micros,
            bytes: chunk.bytes,
        });
        self.update_gauges();
        self.drain(&mut emit);
    }

    pub fn process_gap(&mut self, gap: &TcpStreamGap, mut emit: impl FnMut(BpsrFramingEvent)) {
        self.discard_buffer(
            BpsrFramingIssueReason::TcpGap,
            gap.stream_offset,
            gap.capture_sequence,
            gap.observed_micros,
            &mut emit,
        );
        self.flow = Some(gap.flow);
        self.expected_stream_offset = Some(gap.stream_offset);
    }

    fn drain(&mut self, emit: &mut impl FnMut(BpsrFramingEvent)) {
        loop {
            let Some(header) = self.peek_header() else {
                return;
            };
            let declared_bytes = u32::from_be_bytes(header[..4].try_into().unwrap());
            let raw_fragment = u16::from_be_bytes(header[4..].try_into().unwrap());
            let fragment_type = raw_fragment & !COMPRESSION_FLAG;
            let length = declared_bytes as usize;
            let valid_length = (FRAME_HEADER_BYTES..=self.config.max_frame_bytes).contains(&length);
            let valid_resync_type = fragment_type <= self.config.max_resync_fragment_type;

            if !valid_length || (!self.synchronized && !valid_resync_type) {
                if self.synchronized && !valid_length {
                    let front = self.chunks.front().expect("header requires buffered data");
                    emit(BpsrFramingEvent::Issue(BpsrFramingIssue {
                        flow: self.flow,
                        stream_offset: front.stream_offset,
                        capture_sequence: front.capture_sequence,
                        observed_micros: front.observed_micros,
                        discarded_bytes: 1,
                        reason: BpsrFramingIssueReason::InvalidFrameLength { declared_bytes },
                    }));
                }
                self.synchronized = false;
                self.discard_prefix(1);
                self.pending_resync_discarded = self.pending_resync_discarded.saturating_add(1);
                continue;
            }

            if self.buffered_bytes < length {
                return;
            }

            if !self.synchronized {
                self.synchronized = true;
                if self.pending_resync_discarded > 0 {
                    let front = self.chunks.front().expect("complete frame is buffered");
                    self.metrics.resynchronizations =
                        self.metrics.resynchronizations.saturating_add(1);
                    emit(BpsrFramingEvent::Issue(BpsrFramingIssue {
                        flow: self.flow,
                        stream_offset: front.stream_offset,
                        capture_sequence: front.capture_sequence,
                        observed_micros: front.observed_micros,
                        discarded_bytes: self.pending_resync_discarded,
                        reason: BpsrFramingIssueReason::Resynchronized {
                            discarded_bytes: self.pending_resync_discarded,
                        },
                    }));
                    self.pending_resync_discarded = 0;
                }
            }

            let complete = self.take_frame(length);
            if complete.crossed_chunks {
                self.metrics.cross_chunk_frames = self.metrics.cross_chunk_frames.saturating_add(1);
            }
            let mut decompression_budget = self.config.max_decompressed_bytes;
            self.decode_complete_frame(complete, 0, &mut decompression_budget, emit);
        }
    }

    fn decode_complete_frame(
        &mut self,
        complete: CompleteFrame,
        nesting_depth: u8,
        decompression_budget: &mut usize,
        emit: &mut impl FnMut(BpsrFramingEvent),
    ) {
        let raw_fragment = u16::from_be_bytes([complete.bytes[4], complete.bytes[5]]);
        let compressed = raw_fragment & COMPRESSION_FLAG != 0;
        let fragment = FragmentKind::from_wire_id(raw_fragment & !COMPRESSION_FLAG);
        let payload = complete.bytes.slice(FRAME_HEADER_BYTES..);
        let mut route = None;
        let mut compression = if compressed {
            CompressionState::Unknown
        } else {
            CompressionState::NotCompressed
        };
        let mut application_bytes = None;
        let mut issue = None;

        match fragment {
            FragmentKind::Notify if payload.len() >= 16 => {
                let service_id = read_u64(&payload, 0);
                let stub_id = read_u32(&payload, 8);
                let method_id = read_u32(&payload, 12);
                route = Some(RoutedMessage {
                    key: RouteKey::new(self.direction, fragment, service_id, method_id),
                    stub_id,
                    call_id: None,
                });
                match self.decode_body(payload.slice(16..), compressed, decompression_budget) {
                    Ok((state, body)) => {
                        compression = state;
                        application_bytes = Some(body);
                    }
                    Err(reason) => {
                        compression = CompressionState::ZstdFailed;
                        issue = Some(reason);
                    }
                }
            }
            FragmentKind::Call => {
                let route_body = match self.config.call_layout {
                    BpsrCallLayout::WithCallId if payload.len() >= 20 => {
                        let service_id = read_u64(&payload, 0);
                        let stub_id = read_u32(&payload, 8);
                        let call_id = read_u32(&payload, 12);
                        let method_id = read_u32(&payload, 16);
                        route = Some(RoutedMessage {
                            key: RouteKey::new(self.direction, fragment, service_id, method_id),
                            stub_id,
                            call_id: Some(call_id),
                        });
                        Some(payload.slice(20..))
                    }
                    BpsrCallLayout::WithoutCallId if payload.len() >= 16 => {
                        let service_id = read_u64(&payload, 0);
                        let stub_id = read_u32(&payload, 8);
                        let method_id = read_u32(&payload, 12);
                        route = Some(RoutedMessage {
                            key: RouteKey::new(self.direction, fragment, service_id, method_id),
                            stub_id,
                            call_id: None,
                        });
                        Some(payload.slice(16..))
                    }
                    BpsrCallLayout::Opaque => {
                        if !compressed {
                            application_bytes = Some(payload);
                        }
                        None
                    }
                    BpsrCallLayout::WithCallId | BpsrCallLayout::WithoutCallId => {
                        issue = Some(BpsrFramingIssueReason::MalformedRouteHeader);
                        None
                    }
                };
                if let Some(body) = route_body {
                    match self.decode_body(body, compressed, decompression_budget) {
                        Ok((state, body)) => {
                            compression = state;
                            application_bytes = Some(body);
                        }
                        Err(reason) => {
                            compression = CompressionState::ZstdFailed;
                            issue = Some(reason);
                        }
                    }
                }
            }
            FragmentKind::Return => {
                let header_bytes = match self.config.return_layout {
                    BpsrReturnLayout::TwelveByteHeader => Some(12),
                    BpsrReturnLayout::FourByteHeader => Some(4),
                    BpsrReturnLayout::Opaque => None,
                };
                if let Some(header_bytes) = header_bytes {
                    if payload.len() < header_bytes {
                        issue = Some(BpsrFramingIssueReason::MalformedRouteHeader);
                    } else {
                        match self.decode_body(
                            payload.slice(header_bytes..),
                            compressed,
                            decompression_budget,
                        ) {
                            Ok((state, body)) => {
                                compression = state;
                                application_bytes = Some(body);
                            }
                            Err(reason) => {
                                compression = CompressionState::ZstdFailed;
                                issue = Some(reason);
                            }
                        }
                    }
                } else if !compressed {
                    application_bytes = Some(payload);
                }
            }
            FragmentKind::FrameDown if payload.len() >= 4 => {
                match self.decode_body(payload.slice(4..), compressed, decompression_budget) {
                    Ok((state, body)) => {
                        compression = state;
                        application_bytes = Some(body);
                    }
                    Err(reason) => {
                        compression = CompressionState::ZstdFailed;
                        issue = Some(reason);
                    }
                }
            }
            FragmentKind::FrameUp if payload.len() >= 4 => match self.config.frame_up_layout {
                BpsrFrameUpLayout::Opaque => {
                    if !compressed {
                        application_bytes = Some(payload.slice(4..));
                    }
                }
                BpsrFrameUpLayout::NestedAfterFourBytes => {
                    match self.decode_body(payload.slice(4..), compressed, decompression_budget) {
                        Ok((state, body)) => {
                            compression = state;
                            application_bytes = Some(body);
                        }
                        Err(reason) => {
                            compression = CompressionState::ZstdFailed;
                            issue = Some(reason);
                        }
                    }
                }
            },
            FragmentKind::Echo => match self.decode_body(payload, compressed, decompression_budget)
            {
                Ok((state, body)) => {
                    compression = state;
                    application_bytes = Some(body);
                }
                Err(reason) => {
                    compression = CompressionState::ZstdFailed;
                    issue = Some(reason);
                }
            },
            FragmentKind::Unknown(_) => {
                if !compressed {
                    application_bytes = Some(payload);
                }
            }
            FragmentKind::Notify => {
                issue = Some(BpsrFramingIssueReason::MalformedRouteHeader);
            }
            FragmentKind::FrameDown | FragmentKind::FrameUp => {
                issue = Some(BpsrFramingIssueReason::MalformedNestedFrames);
            }
        }

        self.metrics.frames_emitted = self.metrics.frames_emitted.saturating_add(1);
        self.metrics.wire_bytes_emitted = self
            .metrics
            .wire_bytes_emitted
            .saturating_add(complete.bytes.len() as u64);
        if nesting_depth > 0 {
            self.metrics.nested_frames_emitted =
                self.metrics.nested_frames_emitted.saturating_add(1);
        }

        emit(BpsrFramingEvent::Frame(BpsrFrame {
            flow: self.flow.expect("frames require a bound flow"),
            direction: self.direction,
            stream_offset: complete.stream_offset,
            capture_sequence: complete.capture_sequence,
            observed_micros: complete.observed_micros,
            nesting_depth,
            fragment,
            compressed_on_wire: compressed,
            compression,
            route,
            wire_bytes: complete.bytes.clone(),
            application_bytes: application_bytes.clone(),
        }));

        if let Some(reason) = issue {
            self.emit_frame_issue(&complete, reason, emit);
        }

        let is_nested_container = fragment == FragmentKind::FrameDown
            || (fragment == FragmentKind::FrameUp
                && self.config.frame_up_layout == BpsrFrameUpLayout::NestedAfterFourBytes);
        if is_nested_container {
            let Some(nested) = application_bytes else {
                return;
            };
            if nesting_depth >= self.config.max_nesting_depth {
                self.emit_frame_issue(&complete, BpsrFramingIssueReason::NestingLimit, emit);
                return;
            }
            self.decode_nested(
                nested,
                nesting_depth + 1,
                complete.stream_offset,
                complete.capture_sequence,
                complete.observed_micros,
                decompression_budget,
                emit,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_nested(
        &mut self,
        bytes: Bytes,
        nesting_depth: u8,
        stream_offset: u64,
        capture_sequence: u64,
        observed_micros: u64,
        decompression_budget: &mut usize,
        emit: &mut impl FnMut(BpsrFramingEvent),
    ) {
        let mut offset = 0usize;
        while offset < bytes.len() {
            let Some(header) = bytes.get(offset..offset.saturating_add(FRAME_HEADER_BYTES)) else {
                self.emit_nested_issue(
                    stream_offset,
                    capture_sequence,
                    observed_micros,
                    BpsrFramingIssueReason::MalformedNestedFrames,
                    emit,
                );
                return;
            };
            let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
            let Some(end) = offset.checked_add(length) else {
                self.emit_nested_issue(
                    stream_offset,
                    capture_sequence,
                    observed_micros,
                    BpsrFramingIssueReason::MalformedNestedFrames,
                    emit,
                );
                return;
            };
            if !(FRAME_HEADER_BYTES..=self.config.max_frame_bytes).contains(&length)
                || end > bytes.len()
            {
                self.emit_nested_issue(
                    stream_offset,
                    capture_sequence,
                    observed_micros,
                    BpsrFramingIssueReason::MalformedNestedFrames,
                    emit,
                );
                return;
            }
            self.decode_complete_frame(
                CompleteFrame {
                    stream_offset,
                    capture_sequence,
                    observed_micros,
                    crossed_chunks: false,
                    bytes: bytes.slice(offset..end),
                },
                nesting_depth,
                decompression_budget,
                emit,
            );
            offset = end;
        }
    }

    fn decode_body(
        &mut self,
        body: Bytes,
        compressed: bool,
        decompression_budget: &mut usize,
    ) -> Result<(CompressionState, Bytes), BpsrFramingIssueReason> {
        if !compressed {
            return Ok((CompressionState::NotCompressed, body));
        }

        match decompress_limited(&body, *decompression_budget) {
            Ok(decompressed) => {
                *decompression_budget = decompression_budget.saturating_sub(decompressed.len());
                self.metrics.decompressions_succeeded =
                    self.metrics.decompressions_succeeded.saturating_add(1);
                self.metrics.decompressed_bytes = self
                    .metrics
                    .decompressed_bytes
                    .saturating_add(decompressed.len() as u64);
                Ok((CompressionState::ZstdDecoded, decompressed))
            }
            Err(DecompressionError::Limit) => {
                self.metrics.decompressions_failed =
                    self.metrics.decompressions_failed.saturating_add(1);
                Err(BpsrFramingIssueReason::DecompressedLimit)
            }
            Err(DecompressionError::Decode) => {
                self.metrics.decompressions_failed =
                    self.metrics.decompressions_failed.saturating_add(1);
                Err(BpsrFramingIssueReason::DecompressionFailed)
            }
        }
    }

    fn peek_header(&self) -> Option<[u8; FRAME_HEADER_BYTES]> {
        if self.buffered_bytes < FRAME_HEADER_BYTES {
            return None;
        }
        let mut header = [0_u8; FRAME_HEADER_BYTES];
        let mut copied = 0usize;
        for chunk in &self.chunks {
            let take = (FRAME_HEADER_BYTES - copied).min(chunk.bytes.len());
            header[copied..copied + take].copy_from_slice(&chunk.bytes[..take]);
            copied += take;
            if copied == FRAME_HEADER_BYTES {
                return Some(header);
            }
        }
        None
    }

    fn take_frame(&mut self, length: usize) -> CompleteFrame {
        let first = self.chunks.front().expect("frame bytes are buffered");
        let stream_offset = first.stream_offset;
        let crossed_chunks = first.bytes.len() < length;
        let mut capture_sequence = first.capture_sequence;
        let mut observed_micros = first.observed_micros;

        let bytes = if !crossed_chunks {
            let front = self.chunks.front_mut().expect("frame bytes are buffered");
            let bytes = front.bytes.split_to(length);
            front.stream_offset = front.stream_offset.saturating_add(length as u64);
            if front.bytes.is_empty() {
                self.chunks.pop_front();
            }
            bytes
        } else {
            let mut output = BytesMut::with_capacity(length);
            let mut remaining = length;
            while remaining > 0 {
                let front = self.chunks.front_mut().expect("frame bytes are buffered");
                let take = remaining.min(front.bytes.len());
                output.extend_from_slice(&front.bytes[..take]);
                front.bytes.advance(take);
                front.stream_offset = front.stream_offset.saturating_add(take as u64);
                capture_sequence = capture_sequence.max(front.capture_sequence);
                observed_micros = observed_micros.max(front.observed_micros);
                remaining -= take;
                if front.bytes.is_empty() {
                    self.chunks.pop_front();
                }
            }
            output.freeze()
        };
        self.buffered_bytes = self.buffered_bytes.saturating_sub(length);
        self.update_gauges();

        CompleteFrame {
            stream_offset,
            capture_sequence,
            observed_micros,
            crossed_chunks,
            bytes,
        }
    }

    fn discard_prefix(&mut self, mut bytes: usize) {
        let original = bytes;
        while bytes > 0 {
            let front = self
                .chunks
                .front_mut()
                .expect("discarded bytes are buffered");
            let take = bytes.min(front.bytes.len());
            front.bytes.advance(take);
            front.stream_offset = front.stream_offset.saturating_add(take as u64);
            bytes -= take;
            if front.bytes.is_empty() {
                self.chunks.pop_front();
            }
        }
        self.buffered_bytes = self.buffered_bytes.saturating_sub(original);
        self.observe_discard(original);
        self.update_gauges();
    }

    fn discard_buffer(
        &mut self,
        reason: BpsrFramingIssueReason,
        stream_offset: u64,
        capture_sequence: u64,
        observed_micros: u64,
        emit: &mut impl FnMut(BpsrFramingEvent),
    ) {
        let discarded_bytes = self.buffered_bytes;
        let evidence_bytes = (discarded_bytes as u64).saturating_add(self.pending_resync_discarded);
        self.chunks.clear();
        self.buffered_bytes = 0;
        self.synchronized = false;
        self.pending_resync_discarded = 0;
        self.observe_discard(discarded_bytes);
        self.update_gauges();
        emit(BpsrFramingEvent::Issue(BpsrFramingIssue {
            flow: self.flow,
            stream_offset,
            capture_sequence,
            observed_micros,
            discarded_bytes: evidence_bytes,
            reason,
        }));
    }

    fn emit_frame_issue(
        &self,
        complete: &CompleteFrame,
        reason: BpsrFramingIssueReason,
        emit: &mut impl FnMut(BpsrFramingEvent),
    ) {
        self.emit_nested_issue(
            complete.stream_offset,
            complete.capture_sequence,
            complete.observed_micros,
            reason,
            emit,
        );
    }

    fn emit_nested_issue(
        &self,
        stream_offset: u64,
        capture_sequence: u64,
        observed_micros: u64,
        reason: BpsrFramingIssueReason,
        emit: &mut impl FnMut(BpsrFramingEvent),
    ) {
        emit(BpsrFramingEvent::Issue(BpsrFramingIssue {
            flow: self.flow,
            stream_offset,
            capture_sequence,
            observed_micros,
            discarded_bytes: 0,
            reason,
        }));
    }

    fn observe_discard(&mut self, bytes: usize) {
        self.metrics.bytes_discarded = self.metrics.bytes_discarded.saturating_add(bytes as u64);
    }

    fn update_gauges(&mut self) {
        self.metrics.buffered_chunks = self.chunks.len() as u64;
        self.metrics.buffered_bytes = self.buffered_bytes as u64;
        self.metrics.buffered_bytes_high_water = self
            .metrics
            .buffered_bytes_high_water
            .max(self.metrics.buffered_bytes);
    }
}

fn validate_config(config: BpsrFramingConfig) -> Result<(), BpsrFramingConfigError> {
    if config.max_frame_bytes < FRAME_HEADER_BYTES {
        return Err(BpsrFramingConfigError::FrameTooSmall);
    }
    if config.max_buffered_bytes == 0 {
        return Err(BpsrFramingConfigError::ZeroBufferedBytes);
    }
    if config.max_frame_bytes > config.max_buffered_bytes {
        return Err(BpsrFramingConfigError::FrameExceedsBuffer);
    }
    if config.max_buffered_chunks == 0 {
        return Err(BpsrFramingConfigError::ZeroBufferedChunks);
    }
    if config.max_decompressed_bytes == 0 {
        return Err(BpsrFramingConfigError::ZeroDecompressedBytes);
    }
    if config.max_nesting_depth == 0 {
        return Err(BpsrFramingConfigError::ZeroNestingDepth);
    }
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[derive(Debug)]
enum DecompressionError {
    Limit,
    Decode,
}

fn decompress_limited(input: &[u8], max_bytes: usize) -> Result<Bytes, DecompressionError> {
    let decoder =
        zstd::stream::read::Decoder::new(input).map_err(|_| DecompressionError::Decode)?;
    let limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut limited = decoder.take(limit);
    let mut output = Vec::new();
    limited
        .read_to_end(&mut output)
        .map_err(|_| DecompressionError::Decode)?;
    if output.len() > max_bytes {
        return Err(DecompressionError::Limit);
    }
    Ok(Bytes::from(output))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use rlogs_network::IpEndpoint;

    use super::*;

    fn flow() -> TcpFlowKey {
        TcpFlowKey::new(
            IpEndpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 31_000),
            IpEndpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 32_000),
        )
    }

    fn chunk(offset: u64, sequence: u64, bytes: Bytes) -> TcpStreamChunk {
        TcpStreamChunk {
            flow: flow(),
            stream_offset: offset,
            capture_sequence: sequence,
            observed_micros: sequence,
            bytes,
        }
    }

    fn frame(fragment: u16, payload: &[u8]) -> Bytes {
        let length = FRAME_HEADER_BYTES + payload.len();
        let mut bytes = BytesMut::with_capacity(length);
        bytes.extend_from_slice(&(length as u32).to_be_bytes());
        bytes.extend_from_slice(&fragment.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes.freeze()
    }

    fn notify_payload(body: &[u8]) -> Bytes {
        let mut payload = BytesMut::new();
        payload.extend_from_slice(&7_u64.to_be_bytes());
        payload.extend_from_slice(&8_u32.to_be_bytes());
        payload.extend_from_slice(&9_u32.to_be_bytes());
        payload.extend_from_slice(body);
        payload.freeze()
    }

    fn call_payload(with_call_id: bool, body: &[u8]) -> Bytes {
        let mut payload = BytesMut::new();
        payload.extend_from_slice(&7_u64.to_be_bytes());
        payload.extend_from_slice(&8_u32.to_be_bytes());
        if with_call_id {
            payload.extend_from_slice(&10_u32.to_be_bytes());
        }
        payload.extend_from_slice(&9_u32.to_be_bytes());
        payload.extend_from_slice(body);
        payload.freeze()
    }

    #[test]
    fn contiguous_notify_frame_stays_zero_copy_and_extracts_route() {
        let wire = frame(2, &notify_payload(b"body"));
        let allocation_start = wire.as_ptr() as usize;
        let allocation_end = allocation_start + wire.len();
        let mut events = Vec::new();
        BpsrStreamFramer::new(PacketDirection::ServerToClient)
            .process(chunk(0, 1, wire), |event| events.push(event));

        let [BpsrFramingEvent::Frame(frame)] = events.as_slice() else {
            panic!("expected one frame, got {events:?}");
        };
        assert_eq!(frame.fragment, FragmentKind::Notify);
        assert_eq!(frame.route.unwrap().key.service_id, 7);
        assert_eq!(frame.route.unwrap().stub_id, 8);
        assert_eq!(frame.route.unwrap().key.method_id, 9);
        assert_eq!(frame.application_bytes.as_deref(), Some(b"body".as_slice()));
        assert!((allocation_start..allocation_end).contains(&(frame.wire_bytes.as_ptr() as usize)));
    }

    #[test]
    fn a_frame_crossing_tcp_chunks_is_coalesced_once() {
        let wire = frame(2, &notify_payload(b"split"));
        let mut framer = BpsrStreamFramer::new(PacketDirection::ServerToClient);
        let mut events = Vec::new();
        framer.process(chunk(0, 1, wire.slice(..5)), |event| events.push(event));
        framer.process(chunk(5, 2, wire.slice(5..)), |event| events.push(event));

        assert!(matches!(
            events.as_slice(),
            [BpsrFramingEvent::Frame(BpsrFrame {
                application_bytes: Some(body),
                ..
            })] if body == b"split".as_slice()
        ));
        assert_eq!(framer.metrics().cross_chunk_frames, 1);
    }

    #[test]
    fn every_possible_two_chunk_split_preserves_the_same_frame() {
        let wire = frame(2, &notify_payload(b"all-splits"));
        for split in 1..wire.len() {
            let mut framer = BpsrStreamFramer::new(PacketDirection::ServerToClient);
            let mut events = Vec::new();
            framer.process(chunk(0, 1, wire.slice(..split)), |event| events.push(event));
            framer.process(chunk(split as u64, 2, wire.slice(split..)), |event| {
                events.push(event)
            });

            assert!(matches!(
                events.as_slice(),
                [BpsrFramingEvent::Frame(BpsrFrame {
                    application_bytes: Some(body),
                    ..
                })] if body == b"all-splits".as_slice()
            ));
        }
    }

    #[test]
    fn call_header_variants_are_explicit_instead_of_guessed() {
        for (layout, with_call_id) in [
            (BpsrCallLayout::WithCallId, true),
            (BpsrCallLayout::WithoutCallId, false),
        ] {
            let config = BpsrFramingConfig {
                call_layout: layout,
                ..BpsrFramingConfig::default()
            };
            let wire = frame(1, &call_payload(with_call_id, b"call-body"));
            let mut events = Vec::new();
            BpsrStreamFramer::try_with_config(PacketDirection::ClientToServer, config)
                .unwrap()
                .process(chunk(0, 1, wire), |event| events.push(event));

            let [BpsrFramingEvent::Frame(frame)] = events.as_slice() else {
                panic!("expected one Call frame, got {events:?}");
            };
            assert_eq!(frame.route.unwrap().key.method_id, 9);
            assert_eq!(frame.route.unwrap().call_id, with_call_id.then_some(10));
            assert_eq!(
                frame.application_bytes.as_deref(),
                Some(b"call-body".as_slice())
            );
        }
    }

    #[test]
    fn compressed_notify_body_is_bounded_and_decoded() {
        let compressed = zstd::stream::encode_all(b"compressed".as_slice(), 1).unwrap();
        let wire = frame(2 | COMPRESSION_FLAG, &notify_payload(&compressed));
        let mut events = Vec::new();
        let mut framer = BpsrStreamFramer::new(PacketDirection::ServerToClient);
        framer.process(chunk(0, 1, wire), |event| events.push(event));

        assert!(matches!(
            events.as_slice(),
            [BpsrFramingEvent::Frame(BpsrFrame {
                compression: CompressionState::ZstdDecoded,
                application_bytes: Some(body),
                ..
            })] if body == b"compressed".as_slice()
        ));
        assert_eq!(framer.metrics().decompressions_succeeded, 1);
    }

    #[test]
    fn decompression_bombs_stop_at_the_configured_output_limit() {
        let config = BpsrFramingConfig {
            max_decompressed_bytes: 8,
            ..BpsrFramingConfig::default()
        };
        let compressed = zstd::stream::encode_all(b"more than eight bytes".as_slice(), 1).unwrap();
        let wire = frame(2 | COMPRESSION_FLAG, &notify_payload(&compressed));
        let mut events = Vec::new();
        let mut framer =
            BpsrStreamFramer::try_with_config(PacketDirection::ServerToClient, config).unwrap();
        framer.process(chunk(0, 1, wire), |event| events.push(event));

        assert!(matches!(
            events.as_slice(),
            [
                BpsrFramingEvent::Frame(BpsrFrame {
                    compression: CompressionState::ZstdFailed,
                    application_bytes: None,
                    ..
                }),
                BpsrFramingEvent::Issue(BpsrFramingIssue {
                    reason: BpsrFramingIssueReason::DecompressedLimit,
                    ..
                })
            ]
        ));
        assert_eq!(framer.metrics().decompressions_failed, 1);
    }

    #[test]
    fn nested_frames_share_one_cumulative_decompression_budget() {
        let nested_body = zstd::stream::encode_all(b"compressed".as_slice(), 1).unwrap();
        let nested = frame(2 | COMPRESSION_FLAG, &notify_payload(&nested_body));
        let outer_body = zstd::stream::encode_all(nested.as_ref(), 1).unwrap();
        let mut outer_payload = BytesMut::new();
        outer_payload.extend_from_slice(&1_u32.to_be_bytes());
        outer_payload.extend_from_slice(&outer_body);
        let outer = frame(6 | COMPRESSION_FLAG, &outer_payload);
        let config = BpsrFramingConfig {
            max_decompressed_bytes: nested.len() + 5,
            ..BpsrFramingConfig::default()
        };
        let mut events = Vec::new();
        BpsrStreamFramer::try_with_config(PacketDirection::ServerToClient, config)
            .unwrap()
            .process(chunk(0, 1, outer), |event| events.push(event));

        assert!(events.iter().any(|event| matches!(
            event,
            BpsrFramingEvent::Frame(BpsrFrame {
                nesting_depth: 1,
                compression: CompressionState::ZstdFailed,
                ..
            })
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            BpsrFramingEvent::Issue(BpsrFramingIssue {
                reason: BpsrFramingIssueReason::DecompressedLimit,
                ..
            })
        )));
    }

    #[test]
    fn frame_down_preserves_parent_and_emits_nested_frames() {
        let nested = frame(2, &notify_payload(b"nested"));
        let mut payload = BytesMut::new();
        payload.extend_from_slice(&1_u32.to_be_bytes());
        payload.extend_from_slice(&nested);
        let outer = frame(6, &payload);
        let mut events = Vec::new();
        BpsrStreamFramer::new(PacketDirection::ServerToClient)
            .process(chunk(0, 1, outer), |event| events.push(event));

        assert!(matches!(
            events.as_slice(),
            [
                BpsrFramingEvent::Frame(BpsrFrame {
                    fragment: FragmentKind::FrameDown,
                    nesting_depth: 0,
                    ..
                }),
                BpsrFramingEvent::Frame(BpsrFrame {
                    fragment: FragmentKind::Notify,
                    nesting_depth: 1,
                    application_bytes: Some(body),
                    ..
                })
            ] if body == b"nested".as_slice()
        ));
    }

    #[test]
    fn frame_up_is_nested_only_when_the_protocol_variant_declares_it() {
        let nested = frame(2, &notify_payload(b"uplink"));
        let mut payload = BytesMut::new();
        payload.extend_from_slice(&1_u32.to_be_bytes());
        payload.extend_from_slice(&nested);
        let outer = frame(5, &payload);
        let config = BpsrFramingConfig {
            frame_up_layout: BpsrFrameUpLayout::NestedAfterFourBytes,
            ..BpsrFramingConfig::default()
        };
        let mut events = Vec::new();
        BpsrStreamFramer::try_with_config(PacketDirection::ClientToServer, config)
            .unwrap()
            .process(chunk(0, 1, outer), |event| events.push(event));

        assert!(matches!(
            events.as_slice(),
            [
                BpsrFramingEvent::Frame(BpsrFrame {
                    fragment: FragmentKind::FrameUp,
                    nesting_depth: 0,
                    ..
                }),
                BpsrFramingEvent::Frame(BpsrFrame {
                    fragment: FragmentKind::Notify,
                    nesting_depth: 1,
                    ..
                })
            ]
        ));
    }

    #[test]
    fn midstream_noise_is_discarded_with_visible_resynchronization() {
        let valid = frame(4, b"");
        let mut bytes = BytesMut::from(&b"noise"[..]);
        bytes.extend_from_slice(&valid);
        let mut events = Vec::new();
        BpsrStreamFramer::new(PacketDirection::Unknown)
            .process(chunk(0, 1, bytes.freeze()), |event| events.push(event));

        assert!(matches!(
            events.as_slice(),
            [
                BpsrFramingEvent::Issue(BpsrFramingIssue {
                    reason: BpsrFramingIssueReason::Resynchronized { discarded_bytes: 5 },
                    ..
                }),
                BpsrFramingEvent::Frame(BpsrFrame {
                    fragment: FragmentKind::Echo,
                    ..
                })
            ]
        ));
    }
}
