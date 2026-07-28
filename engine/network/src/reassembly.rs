use std::collections::HashMap;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::{TcpFlowKey, TcpSegment};

const HALF_SEQUENCE_SPACE: u32 = 1 << 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReassemblyConfig {
    pub max_active_flows: usize,
    pub max_buffered_segments_per_flow: usize,
    pub max_buffered_bytes_per_flow: usize,
    pub max_total_buffered_bytes: usize,
    pub idle_timeout_micros: u64,
}

impl Default for ReassemblyConfig {
    fn default() -> Self {
        Self {
            max_active_flows: 4_096,
            max_buffered_segments_per_flow: 256,
            max_buffered_bytes_per_flow: 4 * 1024 * 1024,
            max_total_buffered_bytes: 64 * 1024 * 1024,
            idle_timeout_micros: 120 * 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReassemblyConfigError {
    ZeroActiveFlows,
    ZeroBufferedSegments,
    ZeroPerFlowBytes,
    ZeroTotalBytes,
    PerFlowExceedsTotal,
    SequenceWindowTooLarge,
}

impl std::fmt::Display for ReassemblyConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ZeroActiveFlows => "max_active_flows must be greater than zero",
            Self::ZeroBufferedSegments => {
                "max_buffered_segments_per_flow must be greater than zero"
            }
            Self::ZeroPerFlowBytes => "max_buffered_bytes_per_flow must be greater than zero",
            Self::ZeroTotalBytes => "max_total_buffered_bytes must be greater than zero",
            Self::PerFlowExceedsTotal => {
                "max_buffered_bytes_per_flow cannot exceed max_total_buffered_bytes"
            }
            Self::SequenceWindowTooLarge => {
                "TCP reorder byte limits must stay below half the sequence space"
            }
        })
    }
}

impl std::error::Error for ReassemblyConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapReason {
    ReorderLimit,
    FlowLimit,
    IdleTimeout,
    ConnectionReset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TcpStreamChunk {
    pub flow: TcpFlowKey,
    /// Byte position in this observed directional stream.
    pub stream_offset: u64,
    pub capture_sequence: u64,
    pub observed_micros: u64,
    /// An O(1) shared slice of the captured frame.
    pub bytes: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TcpStreamGap {
    pub flow: TcpFlowKey,
    pub stream_offset: u64,
    pub reason: GapReason,
    pub expected_sequence: Option<u32>,
    pub next_sequence: Option<u32>,
    pub estimated_missing_bytes: Option<u32>,
    pub discarded_buffered_bytes: u64,
    pub capture_sequence: u64,
    pub observed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum TcpStreamEvent {
    Chunk(TcpStreamChunk),
    Gap(TcpStreamGap),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReassemblyMetrics {
    pub segments_seen: u64,
    pub payload_bytes_seen: u64,
    pub active_flows: u64,
    pub flows_opened: u64,
    pub flows_evicted: u64,
    pub flows_expired: u64,
    pub flows_reset: u64,
    pub in_order_segments: u64,
    pub reordered_segments: u64,
    pub duplicate_segments: u64,
    pub retransmitted_bytes: u64,
    pub overlap_bytes: u64,
    pub buffered_segments: u64,
    pub buffered_bytes: u64,
    pub buffered_bytes_high_water: u64,
    pub forced_gaps: u64,
    pub discarded_buffered_bytes: u64,
    pub stream_chunks: u64,
    pub stream_bytes: u64,
}

#[derive(Debug)]
struct BufferedSegment {
    sequence: u32,
    capture_sequence: u64,
    observed_micros: u64,
    payload: Bytes,
    fin: bool,
}

impl BufferedSegment {
    fn sequence_len(&self) -> usize {
        self.payload.len().saturating_add(self.fin as usize)
    }
}

#[derive(Debug, Default)]
struct FlowState {
    initialized: bool,
    expected_sequence: u32,
    stream_offset: u64,
    syn_sequence: Option<u32>,
    buffered: Option<Vec<BufferedSegment>>,
    buffered_bytes: usize,
    last_capture_sequence: u64,
    last_observed_micros: u64,
}

#[derive(Debug)]
pub struct TcpReassembler {
    config: ReassemblyConfig,
    flows: HashMap<TcpFlowKey, FlowState>,
    total_buffered_bytes: usize,
    metrics: ReassemblyMetrics,
}

impl Default for TcpReassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl TcpReassembler {
    pub fn new() -> Self {
        Self::try_with_config(ReassemblyConfig::default())
            .expect("the built-in reassembly configuration is valid")
    }

    pub fn try_with_config(config: ReassemblyConfig) -> Result<Self, ReassemblyConfigError> {
        validate_config(config)?;
        Ok(Self {
            config,
            flows: HashMap::new(),
            total_buffered_bytes: 0,
            metrics: ReassemblyMetrics::default(),
        })
    }

    pub fn config(&self) -> ReassemblyConfig {
        self.config
    }

    pub fn metrics(&self) -> &ReassemblyMetrics {
        &self.metrics
    }

    /// Processes one segment without allocating an output collection.
    ///
    /// The callback runs synchronously. Consumers can frame protocol messages
    /// immediately or copy only the chunks they intentionally retain.
    pub fn process(&mut self, segment: TcpSegment, mut emit: impl FnMut(TcpStreamEvent)) {
        self.metrics.segments_seen = self.metrics.segments_seen.saturating_add(1);
        self.metrics.payload_bytes_seen = self
            .metrics
            .payload_bytes_seen
            .saturating_add(segment.payload.len() as u64);

        let flow = segment.flow;
        let should_remove = if let Some(state) = self.flows.get_mut(&flow) {
            // One hash lookup on the established-flow hot path.
            process_segment(
                state,
                segment,
                self.config,
                &mut self.total_buffered_bytes,
                &mut self.metrics,
                &mut emit,
            )
        } else {
            if self.flows.len() >= self.config.max_active_flows {
                self.evict_oldest_flow(GapReason::FlowLimit, &mut emit);
            }
            self.flows.insert(flow, FlowState::default());
            self.metrics.flows_opened = self.metrics.flows_opened.saturating_add(1);
            self.metrics.active_flows = self.flows.len() as u64;

            let state = self.flows.get_mut(&flow).expect("flow was inserted");
            process_segment(
                state,
                segment,
                self.config,
                &mut self.total_buffered_bytes,
                &mut self.metrics,
                &mut emit,
            )
        };

        if should_remove {
            self.remove_flow(flow, GapReason::ConnectionReset, &mut emit);
            self.metrics.flows_reset = self.metrics.flows_reset.saturating_add(1);
        }
    }

    /// Removes idle flow state. Call this from a coarse maintenance timer,
    /// rather than scanning all flows for every captured packet.
    pub fn expire_idle(&mut self, observed_micros: u64, mut emit: impl FnMut(TcpStreamEvent)) {
        let timeout = self.config.idle_timeout_micros;
        let expired: Vec<_> = self
            .flows
            .iter()
            .filter_map(|(flow, state)| {
                (observed_micros.saturating_sub(state.last_observed_micros) >= timeout)
                    .then_some(*flow)
            })
            .collect();

        for flow in expired {
            self.remove_flow(flow, GapReason::IdleTimeout, &mut emit);
            self.metrics.flows_expired = self.metrics.flows_expired.saturating_add(1);
        }
    }

    fn evict_oldest_flow(&mut self, reason: GapReason, emit: &mut impl FnMut(TcpStreamEvent)) {
        let oldest = self
            .flows
            .iter()
            .min_by_key(|(flow, state)| {
                (
                    state.last_observed_micros,
                    state.last_capture_sequence,
                    **flow,
                )
            })
            .map(|(flow, _)| *flow);

        if let Some(flow) = oldest {
            self.remove_flow(flow, reason, emit);
            self.metrics.flows_evicted = self.metrics.flows_evicted.saturating_add(1);
        }
    }

    fn remove_flow(
        &mut self,
        flow: TcpFlowKey,
        reason: GapReason,
        emit: &mut impl FnMut(TcpStreamEvent),
    ) {
        let Some(state) = self.flows.remove(&flow) else {
            return;
        };

        self.total_buffered_bytes = self
            .total_buffered_bytes
            .saturating_sub(state.buffered_bytes);
        update_buffer_gauges(
            &mut self.metrics,
            self.total_buffered_bytes,
            -(state.buffered.as_ref().map_or(0, Vec::len) as i64),
        );
        self.metrics.active_flows = self.flows.len() as u64;
        self.metrics.discarded_buffered_bytes = self
            .metrics
            .discarded_buffered_bytes
            .saturating_add(state.buffered_bytes as u64);

        emit(TcpStreamEvent::Gap(TcpStreamGap {
            flow,
            stream_offset: state.stream_offset,
            reason,
            expected_sequence: state.initialized.then_some(state.expected_sequence),
            next_sequence: None,
            estimated_missing_bytes: None,
            discarded_buffered_bytes: state.buffered_bytes as u64,
            capture_sequence: state.last_capture_sequence,
            observed_micros: state.last_observed_micros,
        }));
    }
}

fn validate_config(config: ReassemblyConfig) -> Result<(), ReassemblyConfigError> {
    if config.max_active_flows == 0 {
        return Err(ReassemblyConfigError::ZeroActiveFlows);
    }
    if config.max_buffered_segments_per_flow == 0 {
        return Err(ReassemblyConfigError::ZeroBufferedSegments);
    }
    if config.max_buffered_bytes_per_flow == 0 {
        return Err(ReassemblyConfigError::ZeroPerFlowBytes);
    }
    if config.max_total_buffered_bytes == 0 {
        return Err(ReassemblyConfigError::ZeroTotalBytes);
    }
    if config.max_buffered_bytes_per_flow > config.max_total_buffered_bytes {
        return Err(ReassemblyConfigError::PerFlowExceedsTotal);
    }
    if config.max_buffered_bytes_per_flow >= HALF_SEQUENCE_SPACE as usize
        || config.max_total_buffered_bytes >= HALF_SEQUENCE_SPACE as usize
    {
        return Err(ReassemblyConfigError::SequenceWindowTooLarge);
    }
    Ok(())
}

fn process_segment(
    state: &mut FlowState,
    segment: TcpSegment,
    config: ReassemblyConfig,
    total_buffered_bytes: &mut usize,
    metrics: &mut ReassemblyMetrics,
    emit: &mut impl FnMut(TcpStreamEvent),
) -> bool {
    state.last_capture_sequence = segment.capture_sequence;
    state.last_observed_micros = segment.observed_micros;

    if segment.flags.syn {
        if state.initialized && state.syn_sequence != Some(segment.sequence_number) {
            discard_buffer(
                state,
                segment.flow,
                GapReason::ConnectionReset,
                segment.capture_sequence,
                segment.observed_micros,
                total_buffered_bytes,
                metrics,
                emit,
            );
            *state = FlowState {
                initialized: true,
                expected_sequence: segment.sequence_number.wrapping_add(1),
                syn_sequence: Some(segment.sequence_number),
                last_capture_sequence: segment.capture_sequence,
                last_observed_micros: segment.observed_micros,
                ..FlowState::default()
            };
            metrics.flows_reset = metrics.flows_reset.saturating_add(1);
        } else if !state.initialized {
            state.initialized = true;
            state.expected_sequence = segment.sequence_number.wrapping_add(1);
            state.syn_sequence = Some(segment.sequence_number);
        }
    }

    let buffered = BufferedSegment {
        sequence: segment.payload_sequence_number(),
        capture_sequence: segment.capture_sequence,
        observed_micros: segment.observed_micros,
        payload: segment.payload,
        fin: segment.flags.fin,
    };

    if !state.initialized {
        if buffered.sequence_len() == 0 {
            return segment.flags.rst;
        }
        state.initialized = true;
        state.expected_sequence = buffered.sequence;
    }

    // Pure ACK/window-update packets carry no stream sequence space. Keeping
    // them in a reorder queue would turn ordinary control traffic into work
    // and could manufacture a gap before a delayed payload arrives.
    if buffered.sequence_len() == 0 {
        return segment.flags.rst;
    }

    match relation(buffered.sequence, state.expected_sequence) {
        SequenceRelation::Future(_) => {
            buffer_segment(state, buffered, total_buffered_bytes, metrics);
            enforce_limits(
                state,
                segment.flow,
                config,
                total_buffered_bytes,
                metrics,
                emit,
            );
        }
        SequenceRelation::Exact | SequenceRelation::Past(_) => {
            metrics.in_order_segments = metrics.in_order_segments.saturating_add(1);
            consume_ready_segment(state, segment.flow, buffered, metrics, emit);
            drain_ready(state, segment.flow, total_buffered_bytes, metrics, emit);
        }
    }

    segment.flags.rst
}

fn buffer_segment(
    state: &mut FlowState,
    segment: BufferedSegment,
    total_buffered_bytes: &mut usize,
    metrics: &mut ReassemblyMetrics,
) {
    let bytes = segment.payload.len();
    state.buffered_bytes = state.buffered_bytes.saturating_add(bytes);
    *total_buffered_bytes = total_buffered_bytes.saturating_add(bytes);
    state.buffered.get_or_insert_with(Vec::new).push(segment);

    metrics.reordered_segments = metrics.reordered_segments.saturating_add(1);
    update_buffer_gauges(metrics, *total_buffered_bytes, 1);
}

fn enforce_limits(
    state: &mut FlowState,
    flow: TcpFlowKey,
    config: ReassemblyConfig,
    total_buffered_bytes: &mut usize,
    metrics: &mut ReassemblyMetrics,
    emit: &mut impl FnMut(TcpStreamEvent),
) {
    while state.buffered.as_ref().map_or(0, Vec::len) > config.max_buffered_segments_per_flow
        || state.buffered_bytes > config.max_buffered_bytes_per_flow
        || *total_buffered_bytes > config.max_total_buffered_bytes
    {
        let Some((next_sequence, capture_sequence, observed_micros)) =
            earliest_future_segment(state)
        else {
            drain_ready(state, flow, total_buffered_bytes, metrics, emit);
            break;
        };

        let missing = next_sequence.wrapping_sub(state.expected_sequence);
        emit(TcpStreamEvent::Gap(TcpStreamGap {
            flow,
            stream_offset: state.stream_offset,
            reason: GapReason::ReorderLimit,
            expected_sequence: Some(state.expected_sequence),
            next_sequence: Some(next_sequence),
            estimated_missing_bytes: Some(missing),
            discarded_buffered_bytes: 0,
            capture_sequence,
            observed_micros,
        }));
        metrics.forced_gaps = metrics.forced_gaps.saturating_add(1);
        state.expected_sequence = next_sequence;
        state.stream_offset = state.stream_offset.saturating_add(u64::from(missing));

        drain_ready(state, flow, total_buffered_bytes, metrics, emit);
    }
}

fn earliest_future_segment(state: &FlowState) -> Option<(u32, u64, u64)> {
    state
        .buffered
        .as_ref()?
        .iter()
        .filter_map(
            |segment| match relation(segment.sequence, state.expected_sequence) {
                SequenceRelation::Future(distance) => Some((
                    distance,
                    segment.capture_sequence,
                    segment.sequence,
                    segment.observed_micros,
                )),
                SequenceRelation::Exact | SequenceRelation::Past(_) => None,
            },
        )
        .min()
        .map(|(_, capture_sequence, sequence, observed_micros)| {
            (sequence, capture_sequence, observed_micros)
        })
}

fn drain_ready(
    state: &mut FlowState,
    flow: TcpFlowKey,
    total_buffered_bytes: &mut usize,
    metrics: &mut ReassemblyMetrics,
    emit: &mut impl FnMut(TcpStreamEvent),
) {
    loop {
        let candidate = state.buffered.as_ref().and_then(|buffered| {
            buffered
                .iter()
                .enumerate()
                .filter_map(|(index, segment)| {
                    let relation = relation(segment.sequence, state.expected_sequence);
                    let reaches_expected = match relation {
                        SequenceRelation::Exact => true,
                        SequenceRelation::Past(overlap) => {
                            (overlap as usize) < segment.sequence_len()
                        }
                        SequenceRelation::Future(_) => false,
                    };
                    reaches_expected.then_some((segment.capture_sequence, index))
                })
                .min()
                .map(|(_, index)| index)
        });

        let Some(index) = candidate else {
            clear_obsolete_buffered(state, total_buffered_bytes, metrics);
            break;
        };

        let segment = state
            .buffered
            .as_mut()
            .expect("candidate came from the buffer")
            .swap_remove(index);
        state.buffered_bytes = state.buffered_bytes.saturating_sub(segment.payload.len());
        *total_buffered_bytes = total_buffered_bytes.saturating_sub(segment.payload.len());
        update_buffer_gauges(metrics, *total_buffered_bytes, -1);

        consume_ready_segment(state, flow, segment, metrics, emit);
    }

    // Once a flow has actually reordered, retain the empty Vec capacity for
    // later bursts. Flows that stay in order never allocate it at all.
}

fn clear_obsolete_buffered(
    state: &mut FlowState,
    total_buffered_bytes: &mut usize,
    metrics: &mut ReassemblyMetrics,
) {
    let Some(buffered) = state.buffered.as_mut() else {
        return;
    };

    let mut index = 0;
    while index < buffered.len() {
        let segment = &buffered[index];
        let obsolete = matches!(
            relation(segment.sequence, state.expected_sequence),
            SequenceRelation::Past(overlap) if overlap as usize >= segment.sequence_len()
        );
        if obsolete {
            let segment = buffered.swap_remove(index);
            let retransmitted = segment.payload.len() as u64;
            state.buffered_bytes = state.buffered_bytes.saturating_sub(segment.payload.len());
            *total_buffered_bytes = total_buffered_bytes.saturating_sub(segment.payload.len());
            metrics.duplicate_segments = metrics.duplicate_segments.saturating_add(1);
            metrics.retransmitted_bytes = metrics.retransmitted_bytes.saturating_add(retransmitted);
            update_buffer_gauges(metrics, *total_buffered_bytes, -1);
        } else {
            index += 1;
        }
    }
}

fn consume_ready_segment(
    state: &mut FlowState,
    flow: TcpFlowKey,
    segment: BufferedSegment,
    metrics: &mut ReassemblyMetrics,
    emit: &mut impl FnMut(TcpStreamEvent),
) {
    let overlap = match relation(segment.sequence, state.expected_sequence) {
        SequenceRelation::Past(overlap) => overlap as usize,
        SequenceRelation::Exact => 0,
        SequenceRelation::Future(_) => return,
    };

    let payload_overlap = overlap.min(segment.payload.len());
    if payload_overlap > 0 {
        metrics.overlap_bytes = metrics.overlap_bytes.saturating_add(payload_overlap as u64);
        metrics.retransmitted_bytes = metrics
            .retransmitted_bytes
            .saturating_add(payload_overlap as u64);
    }

    let remaining = segment.sequence_len().saturating_sub(overlap);
    if remaining == 0 {
        if segment.sequence_len() > 0 {
            metrics.duplicate_segments = metrics.duplicate_segments.saturating_add(1);
        }
        return;
    }

    if payload_overlap < segment.payload.len() {
        let bytes = segment.payload.slice(payload_overlap..);
        let length = bytes.len();
        emit(TcpStreamEvent::Chunk(TcpStreamChunk {
            flow,
            stream_offset: state.stream_offset,
            capture_sequence: segment.capture_sequence,
            observed_micros: segment.observed_micros,
            bytes,
        }));
        state.expected_sequence = state.expected_sequence.wrapping_add(length as u32);
        state.stream_offset = state.stream_offset.saturating_add(length as u64);
        metrics.stream_chunks = metrics.stream_chunks.saturating_add(1);
        metrics.stream_bytes = metrics.stream_bytes.saturating_add(length as u64);
    }

    if segment.fin && overlap <= segment.payload.len() {
        state.expected_sequence = state.expected_sequence.wrapping_add(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn discard_buffer(
    state: &mut FlowState,
    flow: TcpFlowKey,
    reason: GapReason,
    capture_sequence: u64,
    observed_micros: u64,
    total_buffered_bytes: &mut usize,
    metrics: &mut ReassemblyMetrics,
    emit: &mut impl FnMut(TcpStreamEvent),
) {
    let discarded_segments = state.buffered.as_ref().map_or(0, Vec::len);
    let discarded_bytes = state.buffered_bytes;
    *total_buffered_bytes = total_buffered_bytes.saturating_sub(discarded_bytes);
    update_buffer_gauges(metrics, *total_buffered_bytes, -(discarded_segments as i64));
    metrics.discarded_buffered_bytes = metrics
        .discarded_buffered_bytes
        .saturating_add(discarded_bytes as u64);
    state.buffered = None;
    state.buffered_bytes = 0;

    emit(TcpStreamEvent::Gap(TcpStreamGap {
        flow,
        stream_offset: state.stream_offset,
        reason,
        expected_sequence: state.initialized.then_some(state.expected_sequence),
        next_sequence: None,
        estimated_missing_bytes: None,
        discarded_buffered_bytes: discarded_bytes as u64,
        capture_sequence,
        observed_micros,
    }));
}

fn update_buffer_gauges(
    metrics: &mut ReassemblyMetrics,
    total_buffered_bytes: usize,
    segment_delta: i64,
) {
    metrics.buffered_bytes = total_buffered_bytes as u64;
    metrics.buffered_bytes_high_water = metrics
        .buffered_bytes_high_water
        .max(metrics.buffered_bytes);
    metrics.buffered_segments = if segment_delta >= 0 {
        metrics
            .buffered_segments
            .saturating_add(segment_delta as u64)
    } else {
        metrics
            .buffered_segments
            .saturating_sub(segment_delta.unsigned_abs())
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceRelation {
    Past(u32),
    Exact,
    Future(u32),
}

fn relation(sequence: u32, expected: u32) -> SequenceRelation {
    let distance = sequence.wrapping_sub(expected);
    if distance == 0 {
        SequenceRelation::Exact
    } else if distance < HALF_SEQUENCE_SPACE {
        SequenceRelation::Future(distance)
    } else {
        SequenceRelation::Past(expected.wrapping_sub(sequence))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use crate::{IpEndpoint, TcpFlags};

    fn flow(port: u16) -> TcpFlowKey {
        TcpFlowKey::new(
            IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), port),
            IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 443),
        )
    }

    fn segment(flow: TcpFlowKey, sequence: u32, payload: &'static [u8]) -> TcpSegment {
        TcpSegment {
            flow,
            sequence_number: sequence,
            acknowledgment_number: 0,
            flags: TcpFlags::default(),
            capture_sequence: u64::from(sequence),
            observed_micros: u64::from(sequence),
            payload: Bytes::from_static(payload),
        }
    }

    fn chunks(events: &[TcpStreamEvent]) -> Vec<&[u8]> {
        events
            .iter()
            .filter_map(|event| match event {
                TcpStreamEvent::Chunk(chunk) => Some(chunk.bytes.as_ref()),
                TcpStreamEvent::Gap(_) => None,
            })
            .collect()
    }

    #[test]
    fn in_order_fast_path_never_creates_a_reorder_buffer() {
        let flow = flow(10_000);
        let mut reassembler = TcpReassembler::new();
        let mut events = Vec::new();

        reassembler.process(segment(flow, 100, b"abcd"), |event| events.push(event));
        reassembler.process(segment(flow, 104, b"ef"), |event| events.push(event));

        assert_eq!(chunks(&events), vec![b"abcd".as_slice(), b"ef".as_slice()]);
        assert_eq!(reassembler.metrics().buffered_segments, 0);
        assert_eq!(reassembler.metrics().buffered_bytes_high_water, 0);
        assert_eq!(reassembler.metrics().reordered_segments, 0);
        assert_eq!(reassembler.metrics().stream_bytes, 6);
    }

    #[test]
    fn out_of_order_segments_are_emitted_in_sequence_order() {
        let mut reassembler = TcpReassembler::new();
        let mut events = Vec::new();

        // Starting mid-connection makes the first observed payload the stream
        // baseline. Exercise real reordering after establishing that baseline.
        let flow = flow(10_002);
        reassembler.process(segment(flow, 100, b"ab"), |event| events.push(event));
        reassembler.process(segment(flow, 104, b"ef"), |event| events.push(event));
        reassembler.process(segment(flow, 102, b"cd"), |event| events.push(event));

        assert_eq!(
            chunks(&events),
            vec![b"ab".as_slice(), b"cd".as_slice(), b"ef".as_slice()]
        );
        assert_eq!(reassembler.metrics().buffered_segments, 0);
        assert!(reassembler.metrics().reordered_segments >= 1);
    }

    #[test]
    fn retransmissions_and_overlaps_do_not_duplicate_stream_bytes() {
        let flow = flow(10_003);
        let mut reassembler = TcpReassembler::new();
        let mut events = Vec::new();

        reassembler.process(segment(flow, 100, b"abcde"), |event| events.push(event));
        reassembler.process(segment(flow, 103, b"defgh"), |event| events.push(event));
        reassembler.process(segment(flow, 100, b"abcde"), |event| events.push(event));

        assert_eq!(
            chunks(&events),
            vec![b"abcde".as_slice(), b"fgh".as_slice()]
        );
        assert_eq!(reassembler.metrics().stream_bytes, 8);
        assert_eq!(reassembler.metrics().overlap_bytes, 7);
        assert_eq!(reassembler.metrics().duplicate_segments, 1);
        assert_eq!(reassembler.metrics().retransmitted_bytes, 7);
    }

    #[test]
    fn sequence_number_wraparound_remains_contiguous() {
        let flow = flow(10_004);
        let mut reassembler = TcpReassembler::new();
        let mut events = Vec::new();

        reassembler.process(segment(flow, u32::MAX - 1, b"abc"), |event| {
            events.push(event)
        });
        reassembler.process(segment(flow, 1, b"de"), |event| events.push(event));

        assert_eq!(chunks(&events), vec![b"abc".as_slice(), b"de".as_slice()]);
        assert_eq!(reassembler.metrics().stream_bytes, 5);
    }

    #[test]
    fn memory_pressure_advances_with_an_explicit_gap() {
        let config = ReassemblyConfig {
            max_buffered_bytes_per_flow: 4,
            max_total_buffered_bytes: 4,
            ..ReassemblyConfig::default()
        };
        let mut reassembler = TcpReassembler::try_with_config(config).unwrap();
        let flow = flow(10_005);
        let mut events = Vec::new();

        reassembler.process(segment(flow, 100, b"a"), |event| events.push(event));
        reassembler.process(segment(flow, 105, b"world"), |event| events.push(event));

        assert!(matches!(
            &events[1],
            TcpStreamEvent::Gap(TcpStreamGap {
                reason: GapReason::ReorderLimit,
                estimated_missing_bytes: Some(4),
                ..
            })
        ));
        assert_eq!(chunks(&events), vec![b"a".as_slice(), b"world".as_slice()]);
        assert_eq!(reassembler.metrics().forced_gaps, 1);
        assert_eq!(reassembler.metrics().buffered_bytes, 0);
    }

    #[test]
    fn syn_is_sequence_space_not_application_data() {
        let flow = flow(10_006);
        let mut reassembler = TcpReassembler::new();
        let mut events = Vec::new();
        let mut syn = segment(flow, 500, b"");
        syn.flags.syn = true;
        syn.capture_sequence = 1;
        let mut payload = segment(flow, 501, b"data");
        payload.capture_sequence = 2;

        reassembler.process(syn, |event| events.push(event));
        reassembler.process(payload, |event| events.push(event));

        assert_eq!(chunks(&events), vec![b"data".as_slice()]);
        assert_eq!(reassembler.metrics().stream_bytes, 4);
        assert_eq!(reassembler.metrics().duplicate_segments, 0);
    }

    #[test]
    fn pure_ack_packets_never_allocate_reorder_state() {
        let flow = flow(10_007);
        let mut reassembler = TcpReassembler::new();
        let mut events = Vec::new();

        reassembler.process(segment(flow, 100, b"data"), |event| events.push(event));
        let mut ack = segment(flow, 999, b"");
        ack.flags.ack = true;
        reassembler.process(ack, |event| events.push(event));

        assert_eq!(chunks(&events), vec![b"data".as_slice()]);
        assert_eq!(reassembler.metrics().buffered_segments, 0);
        assert_eq!(reassembler.metrics().buffered_bytes_high_water, 0);
    }

    #[test]
    fn flow_and_idle_limits_evict_deterministically_with_evidence() {
        let config = ReassemblyConfig {
            max_active_flows: 1,
            idle_timeout_micros: 10,
            ..ReassemblyConfig::default()
        };
        let mut reassembler = TcpReassembler::try_with_config(config).unwrap();
        let mut events = Vec::new();

        reassembler.process(segment(flow(1), 1, b"a"), |event| events.push(event));
        reassembler.process(segment(flow(2), 1, b"b"), |event| events.push(event));
        assert!(events.iter().any(|event| matches!(
            event,
            TcpStreamEvent::Gap(TcpStreamGap {
                reason: GapReason::FlowLimit,
                ..
            })
        )));

        reassembler.expire_idle(100, |event| events.push(event));
        assert!(events.iter().any(|event| matches!(
            event,
            TcpStreamEvent::Gap(TcpStreamGap {
                reason: GapReason::IdleTimeout,
                ..
            })
        )));
        assert_eq!(reassembler.metrics().active_flows, 0);
    }

    #[test]
    fn invalid_memory_budgets_are_rejected() {
        let config = ReassemblyConfig {
            max_buffered_bytes_per_flow: 10,
            max_total_buffered_bytes: 5,
            ..ReassemblyConfig::default()
        };

        assert!(matches!(
            TcpReassembler::try_with_config(config),
            Err(ReassemblyConfigError::PerFlowExceedsTotal)
        ));
    }
}
