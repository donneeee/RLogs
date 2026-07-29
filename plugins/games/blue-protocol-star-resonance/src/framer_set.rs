use std::collections::HashMap;

use rlogs_network::{TcpFlowKey, TcpStreamEvent};
use serde::{Deserialize, Serialize};

use crate::{
    BpsrFramingConfig, BpsrFramingConfigError, BpsrFramingEvent, BpsrFramingIssue,
    BpsrFramingIssueReason, BpsrStreamFramer, PacketDirection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BpsrFramerSetConfig {
    pub max_active_streams: usize,
    pub max_total_buffered_bytes: usize,
    pub idle_timeout_micros: u64,
    pub stream: BpsrFramingConfig,
}

impl Default for BpsrFramerSetConfig {
    fn default() -> Self {
        Self {
            max_active_streams: 128,
            max_total_buffered_bytes: 64 * 1024 * 1024,
            idle_timeout_micros: 120 * 1_000_000,
            stream: BpsrFramingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpsrFramerSetConfigError {
    ZeroActiveStreams,
    ZeroTotalBytes,
    StreamExceedsTotal,
    ZeroIdleTimeout,
    InvalidStream(BpsrFramingConfigError),
}

impl std::fmt::Display for BpsrFramerSetConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroActiveStreams => {
                formatter.write_str("max_active_streams must be greater than zero")
            }
            Self::ZeroTotalBytes => {
                formatter.write_str("max_total_buffered_bytes must be greater than zero")
            }
            Self::StreamExceedsTotal => formatter
                .write_str("per-stream buffered bytes cannot exceed the total framing byte budget"),
            Self::ZeroIdleTimeout => {
                formatter.write_str("idle_timeout_micros must be greater than zero")
            }
            Self::InvalidStream(error) => {
                write!(formatter, "invalid stream framing config: {error}")
            }
        }
    }
}

impl std::error::Error for BpsrFramerSetConfigError {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BpsrFramerSetMetrics {
    pub active_streams: u64,
    pub streams_opened: u64,
    pub streams_evicted: u64,
    pub streams_expired: u64,
    pub total_buffered_bytes: u64,
    pub total_buffered_bytes_high_water: u64,
    pub discarded_buffered_bytes: u64,
    pub discarded_incoming_bytes: u64,
}

#[derive(Debug)]
struct StreamState {
    framer: BpsrStreamFramer,
    first_capture_sequence: u64,
    last_capture_sequence: u64,
    last_observed_micros: u64,
    last_stream_offset: u64,
}

/// Owns the bounded set of directional BPSR stream framers.
#[derive(Debug)]
pub struct BpsrFramerSet {
    config: BpsrFramerSetConfig,
    streams: HashMap<TcpFlowKey, StreamState>,
    total_buffered_bytes: usize,
    metrics: BpsrFramerSetMetrics,
}

impl Default for BpsrFramerSet {
    fn default() -> Self {
        Self::new()
    }
}

impl BpsrFramerSet {
    pub fn new() -> Self {
        Self::try_with_config(BpsrFramerSetConfig::default())
            .expect("the built-in BPSR framer-set configuration is valid")
    }

    pub fn try_with_config(config: BpsrFramerSetConfig) -> Result<Self, BpsrFramerSetConfigError> {
        validate_config(config)?;
        Ok(Self {
            config,
            streams: HashMap::new(),
            total_buffered_bytes: 0,
            metrics: BpsrFramerSetMetrics::default(),
        })
    }

    pub fn config(&self) -> BpsrFramerSetConfig {
        self.config
    }

    pub fn metrics(&self) -> &BpsrFramerSetMetrics {
        &self.metrics
    }

    pub fn process(
        &mut self,
        direction: PacketDirection,
        event: TcpStreamEvent,
        mut emit: impl FnMut(BpsrFramingEvent),
    ) {
        let (flow, stream_offset, capture_sequence, observed_micros, incoming_bytes) = match &event
        {
            TcpStreamEvent::Chunk(chunk) => (
                chunk.flow,
                chunk.stream_offset,
                chunk.capture_sequence,
                chunk.observed_micros,
                chunk.bytes.len(),
            ),
            TcpStreamEvent::Gap(gap) => (
                gap.flow,
                gap.stream_offset,
                gap.capture_sequence,
                gap.observed_micros,
                0,
            ),
        };

        self.expire(observed_micros, &mut emit);

        if incoming_bytes > self.config.max_total_buffered_bytes {
            self.remove_stream(
                flow,
                BpsrFramingIssueReason::TotalBufferLimit,
                capture_sequence,
                observed_micros,
                stream_offset,
                false,
                &mut emit,
            );
            self.metrics.discarded_incoming_bytes = self
                .metrics
                .discarded_incoming_bytes
                .saturating_add(incoming_bytes as u64);
            emit(BpsrFramingEvent::Issue(BpsrFramingIssue {
                flow: Some(flow),
                stream_offset,
                capture_sequence,
                observed_micros,
                discarded_bytes: incoming_bytes as u64,
                reason: BpsrFramingIssueReason::TotalBufferLimit,
            }));
            return;
        }

        if self
            .streams
            .get(&flow)
            .is_some_and(|state| state.framer.direction() != direction)
        {
            self.remove_stream(
                flow,
                BpsrFramingIssueReason::StreamChanged,
                capture_sequence,
                observed_micros,
                stream_offset,
                false,
                &mut emit,
            );
        }

        if !self.streams.contains_key(&flow) {
            if self.streams.len() >= self.config.max_active_streams {
                self.evict_oldest(
                    None,
                    BpsrFramingIssueReason::StreamLimit,
                    capture_sequence,
                    observed_micros,
                    stream_offset,
                    &mut emit,
                );
            }
            self.insert_stream(
                flow,
                direction,
                capture_sequence,
                observed_micros,
                stream_offset,
            );
        }

        while self.total_buffered_bytes.saturating_add(incoming_bytes)
            > self.config.max_total_buffered_bytes
        {
            if !self.evict_oldest(
                Some(flow),
                BpsrFramingIssueReason::TotalBufferLimit,
                capture_sequence,
                observed_micros,
                stream_offset,
                &mut emit,
            ) {
                self.remove_stream(
                    flow,
                    BpsrFramingIssueReason::TotalBufferLimit,
                    capture_sequence,
                    observed_micros,
                    stream_offset,
                    false,
                    &mut emit,
                );
                self.insert_stream(
                    flow,
                    direction,
                    capture_sequence,
                    observed_micros,
                    stream_offset,
                );
                break;
            }
        }

        let state = self.streams.get_mut(&flow).expect("stream was inserted");
        let before = state.framer.buffered_bytes();
        match event {
            TcpStreamEvent::Chunk(chunk) => state.framer.process(chunk, &mut emit),
            TcpStreamEvent::Gap(gap) => state.framer.process_gap(&gap, &mut emit),
        }
        let after = state.framer.buffered_bytes();
        self.total_buffered_bytes = self
            .total_buffered_bytes
            .saturating_sub(before)
            .saturating_add(after);
        state.last_capture_sequence = capture_sequence;
        state.last_observed_micros = observed_micros;
        state.last_stream_offset = stream_offset;
        self.update_gauges();
    }

    pub fn expire(&mut self, observed_micros: u64, mut emit: impl FnMut(BpsrFramingEvent)) {
        let expired: Vec<_> = self
            .streams
            .iter()
            .filter_map(|(flow, state)| {
                (observed_micros.saturating_sub(state.last_observed_micros)
                    >= self.config.idle_timeout_micros)
                    .then_some(*flow)
            })
            .collect();
        for flow in expired {
            let state = self.streams.get(&flow).expect("expired stream exists");
            let capture_sequence = state.last_capture_sequence;
            let stream_offset = state.last_stream_offset;
            self.remove_stream(
                flow,
                BpsrFramingIssueReason::IdleTimeout,
                capture_sequence,
                observed_micros,
                stream_offset,
                true,
                &mut emit,
            );
        }
    }

    fn insert_stream(
        &mut self,
        flow: TcpFlowKey,
        direction: PacketDirection,
        capture_sequence: u64,
        observed_micros: u64,
        stream_offset: u64,
    ) {
        let framer = BpsrStreamFramer::try_with_config(direction, self.config.stream)
            .expect("framer-set stream configuration was validated");
        self.streams.insert(
            flow,
            StreamState {
                framer,
                first_capture_sequence: capture_sequence,
                last_capture_sequence: capture_sequence,
                last_observed_micros: observed_micros,
                last_stream_offset: stream_offset,
            },
        );
        self.metrics.streams_opened = self.metrics.streams_opened.saturating_add(1);
        self.update_gauges();
    }

    #[allow(clippy::too_many_arguments)]
    fn remove_stream(
        &mut self,
        flow: TcpFlowKey,
        reason: BpsrFramingIssueReason,
        capture_sequence: u64,
        observed_micros: u64,
        stream_offset: u64,
        expired: bool,
        emit: &mut impl FnMut(BpsrFramingEvent),
    ) {
        let Some(state) = self.streams.remove(&flow) else {
            return;
        };
        let buffered_bytes = state.framer.buffered_bytes();
        self.total_buffered_bytes = self.total_buffered_bytes.saturating_sub(buffered_bytes);
        self.metrics.discarded_buffered_bytes = self
            .metrics
            .discarded_buffered_bytes
            .saturating_add(buffered_bytes as u64);
        if expired {
            self.metrics.streams_expired = self.metrics.streams_expired.saturating_add(1);
        } else {
            self.metrics.streams_evicted = self.metrics.streams_evicted.saturating_add(1);
        }
        self.update_gauges();
        emit(BpsrFramingEvent::Issue(BpsrFramingIssue {
            flow: Some(flow),
            stream_offset,
            capture_sequence: capture_sequence.max(state.last_capture_sequence),
            observed_micros: observed_micros.max(state.last_observed_micros),
            discarded_bytes: state.framer.buffered_evidence_bytes(),
            reason,
        }));
    }

    fn evict_oldest(
        &mut self,
        except: Option<TcpFlowKey>,
        reason: BpsrFramingIssueReason,
        capture_sequence: u64,
        observed_micros: u64,
        stream_offset: u64,
        emit: &mut impl FnMut(BpsrFramingEvent),
    ) -> bool {
        let oldest = self
            .streams
            .iter()
            .filter(|(flow, _)| except != Some(**flow))
            .min_by_key(|(flow, state)| {
                (
                    state.last_observed_micros,
                    state.first_capture_sequence,
                    **flow,
                )
            })
            .map(|(flow, _)| *flow);
        let Some(flow) = oldest else {
            return false;
        };
        self.remove_stream(
            flow,
            reason,
            capture_sequence,
            observed_micros,
            stream_offset,
            false,
            emit,
        );
        true
    }

    fn update_gauges(&mut self) {
        self.metrics.active_streams = self.streams.len() as u64;
        self.metrics.total_buffered_bytes = self.total_buffered_bytes as u64;
        self.metrics.total_buffered_bytes_high_water = self
            .metrics
            .total_buffered_bytes_high_water
            .max(self.metrics.total_buffered_bytes);
    }
}

fn validate_config(config: BpsrFramerSetConfig) -> Result<(), BpsrFramerSetConfigError> {
    BpsrStreamFramer::try_with_config(PacketDirection::Unknown, config.stream)
        .map_err(BpsrFramerSetConfigError::InvalidStream)?;
    if config.max_active_streams == 0 {
        return Err(BpsrFramerSetConfigError::ZeroActiveStreams);
    }
    if config.max_total_buffered_bytes == 0 {
        return Err(BpsrFramerSetConfigError::ZeroTotalBytes);
    }
    if config.stream.max_buffered_bytes > config.max_total_buffered_bytes {
        return Err(BpsrFramerSetConfigError::StreamExceedsTotal);
    }
    if config.idle_timeout_micros == 0 {
        return Err(BpsrFramerSetConfigError::ZeroIdleTimeout);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use bytes::Bytes;
    use rlogs_network::{IpEndpoint, TcpFlags, TcpReassembler, TcpSegment, TcpStreamChunk};

    use super::*;

    fn flow(port: u16) -> TcpFlowKey {
        TcpFlowKey::new(
            IpEndpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            IpEndpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 32_000),
        )
    }

    fn partial(flow: TcpFlowKey, sequence: u64) -> TcpStreamEvent {
        TcpStreamEvent::Chunk(TcpStreamChunk {
            flow,
            stream_offset: 0,
            capture_sequence: sequence,
            observed_micros: sequence,
            bytes: Bytes::from_static(&[0, 0, 0, 8, 0, 4]),
        })
    }

    #[test]
    fn total_memory_pressure_evicts_the_oldest_partial_stream() {
        let stream = BpsrFramingConfig {
            max_frame_bytes: 8,
            max_buffered_bytes: 8,
            max_buffered_chunks: 2,
            max_decompressed_bytes: 8,
            max_nesting_depth: 1,
            max_resync_fragment_type: 8,
            call_layout: crate::BpsrCallLayout::WithCallId,
            return_layout: crate::BpsrReturnLayout::TwelveByteHeader,
            frame_up_layout: crate::BpsrFrameUpLayout::Opaque,
        };
        let config = BpsrFramerSetConfig {
            max_active_streams: 2,
            max_total_buffered_bytes: 10,
            idle_timeout_micros: 100,
            stream,
        };
        let mut set = BpsrFramerSet::try_with_config(config).unwrap();
        let mut events = Vec::new();
        set.process(
            PacketDirection::ServerToClient,
            partial(flow(1), 1),
            |event| events.push(event),
        );
        set.process(
            PacketDirection::ServerToClient,
            partial(flow(2), 2),
            |event| events.push(event),
        );

        assert!(events.iter().any(|event| matches!(
            event,
            BpsrFramingEvent::Issue(BpsrFramingIssue {
                flow: Some(evicted),
                reason: BpsrFramingIssueReason::TotalBufferLimit,
                discarded_bytes: 6,
                ..
            }) if *evicted == flow(1)
        )));
        assert_eq!(set.metrics().active_streams, 1);
        assert_eq!(set.metrics().total_buffered_bytes, 6);
    }

    #[test]
    fn idle_streams_release_partial_frames_with_evidence() {
        let config = BpsrFramerSetConfig {
            idle_timeout_micros: 10,
            ..BpsrFramerSetConfig::default()
        };
        let mut set = BpsrFramerSet::try_with_config(config).unwrap();
        set.process(PacketDirection::ServerToClient, partial(flow(1), 1), |_| {});
        let mut events = Vec::new();
        set.expire(11, |event| events.push(event));

        assert!(matches!(
            events.as_slice(),
            [BpsrFramingEvent::Issue(BpsrFramingIssue {
                reason: BpsrFramingIssueReason::IdleTimeout,
                discarded_bytes: 6,
                ..
            })]
        ));
        assert_eq!(set.metrics().active_streams, 0);
        assert_eq!(set.metrics().total_buffered_bytes, 0);
    }

    #[test]
    fn reordered_tcp_chunks_feed_one_complete_bpsr_frame() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&26_u32.to_be_bytes());
        wire.extend_from_slice(&2_u16.to_be_bytes());
        wire.extend_from_slice(&7_u64.to_be_bytes());
        wire.extend_from_slice(&8_u32.to_be_bytes());
        wire.extend_from_slice(&9_u32.to_be_bytes());
        wire.extend_from_slice(b"body");
        let wire = Bytes::from(wire);
        let split = 10usize;
        let stream_flow = flow(1);
        let mut tcp = TcpReassembler::new();
        let mut framers = BpsrFramerSet::new();
        let mut framing_events = Vec::new();
        let mut feed = |segment| {
            tcp.process(segment, |event| {
                framers.process(PacketDirection::ServerToClient, event, |event| {
                    framing_events.push(event)
                });
            });
        };

        feed(TcpSegment {
            flow: stream_flow,
            sequence_number: 99,
            acknowledgment_number: 0,
            flags: TcpFlags {
                syn: true,
                ..TcpFlags::default()
            },
            capture_sequence: 1,
            observed_micros: 1,
            payload: Bytes::new(),
        });
        feed(TcpSegment {
            flow: stream_flow,
            sequence_number: 100 + split as u32,
            acknowledgment_number: 0,
            flags: TcpFlags::default(),
            capture_sequence: 2,
            observed_micros: 2,
            payload: wire.slice(split..),
        });
        feed(TcpSegment {
            flow: stream_flow,
            sequence_number: 100,
            acknowledgment_number: 0,
            flags: TcpFlags::default(),
            capture_sequence: 3,
            observed_micros: 3,
            payload: wire.slice(..split),
        });

        assert!(matches!(
            framing_events.as_slice(),
            [BpsrFramingEvent::Frame(crate::BpsrFrame {
                application_bytes: Some(body),
                ..
            })] if body == b"body".as_slice()
        ));
    }
}
