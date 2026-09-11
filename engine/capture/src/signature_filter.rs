use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::process_filter::{ConnectionKey, extract_tcp_frame};
use crate::{
    CaptureError, CaptureSource, CaptureSourceKind, CaptureSourceMetadata, CapturedFrame,
    TcpConnection,
};

/// Direction established by a game-specific, exact TCP payload signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpPayloadDirection {
    ClientToServer,
    ServerToClient,
}

/// Result of inspecting a bounded, contiguous TCP payload prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpPayloadSignatureResult {
    /// The bytes seen so far remain a possible signature prefix.
    NeedMore,
    /// The current prefix does not match the protocol signature.
    Reject,
    /// The prefix proves the direction of this exact protocol flow.
    Match(TcpPayloadDirection),
}

/// A classifier must return a direction only for an exact, game-specific
/// payload signature. Returning `None` keeps the frame inside the bounded
/// transient privacy buffer and prevents it from reaching persistence or
/// protocol decoding.
pub type TcpPayloadSignature = fn(&[u8]) -> Option<TcpPayloadDirection>;

/// Incremental classifier used when a signature can cross TCP segment boundaries.
pub type TcpPayloadPrefixSignature = fn(&[u8]) -> TcpPayloadSignatureResult;

pub const MAX_TCP_SIGNATURE_PREFIX_BYTES: usize = 64 * 1024;
const MAX_TRACKED_PREFIX_EPOCHS: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureFlowCaptureConfig {
    pub pending_ttl_micros: u64,
    pub confirmed_idle_timeout_micros: u64,
    pub max_pending_frames: usize,
    pub max_pending_bytes: usize,
}

impl Default for SignatureFlowCaptureConfig {
    fn default() -> Self {
        Self {
            pending_ttl_micros: 2_000_000,
            confirmed_idle_timeout_micros: 10_000_000,
            max_pending_frames: 8_192,
            max_pending_bytes: 16 * 1024 * 1024,
        }
    }
}

impl SignatureFlowCaptureConfig {
    pub fn validate(self) -> Result<Self, SignatureFlowCaptureConfigError> {
        if self.pending_ttl_micros == 0 {
            return Err(SignatureFlowCaptureConfigError::ZeroPendingTtl);
        }
        if self.confirmed_idle_timeout_micros == 0 {
            return Err(SignatureFlowCaptureConfigError::ZeroConfirmedIdleTimeout);
        }
        if self.max_pending_frames == 0 {
            return Err(SignatureFlowCaptureConfigError::ZeroPendingFrames);
        }
        if self.max_pending_bytes == 0 {
            return Err(SignatureFlowCaptureConfigError::ZeroPendingBytes);
        }
        Ok(self)
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SignatureFlowCaptureConfigError {
    #[error("pending-frame TTL must be greater than zero")]
    ZeroPendingTtl,
    #[error("pending frame limit must be greater than zero")]
    ZeroPendingFrames,
    #[error("confirmed-flow idle timeout must be greater than zero")]
    ZeroConfirmedIdleTimeout,
    #[error("pending byte limit must be greater than zero")]
    ZeroPendingBytes,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureFlowCaptureMetrics {
    pub ingress_frames: u64,
    pub ingress_bytes: u64,
    pub emitted_frames: u64,
    pub emitted_bytes: u64,
    pub non_tcp_frames_discarded: u64,
    pub unidentified_frames_discarded: u64,
    pub pending_limit_evictions: u64,
    pub signature_matches: u64,
    pub confirmed_connections: u64,
    pub peak_pending_frames: usize,
    pub peak_pending_bytes: usize,
}

#[derive(Debug)]
struct PendingFrame {
    frame: CapturedFrame,
    connection: ConnectionKey,
    flow: crate::process_filter::DirectedTcpFlow,
    payload_sequence: u32,
    epoch: u64,
    disposition: PendingDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingDisposition {
    Unknown,
    Confirmed,
}

#[derive(Debug, Default)]
struct ConnectionEpoch {
    id: u64,
    last_seen_micros: u64,
    primary_syn: Option<(crate::process_filter::DirectedTcpFlow, u32)>,
    syn_roots: BTreeMap<crate::process_filter::DirectedTcpFlow, u32>,
    rejected_flows: BTreeSet<crate::process_filter::DirectedTcpFlow>,
}

#[derive(Debug, Clone, Copy)]
enum FlowSignature {
    Packet(TcpPayloadSignature),
    Prefix(TcpPayloadPrefixSignature),
}

#[derive(Debug, Clone, Copy)]
struct ConfirmedFlow {
    connection: TcpConnection,
    last_seen_micros: u64,
    epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefixEvaluation {
    Classified(TcpPayloadSignatureResult),
    ConflictingOverlap,
}

/// Filters broad in-memory TCP ingress by an exact game-protocol signature.
///
/// Unknown traffic is held only in a short, bounded transient queue. It is
/// never returned to callers, persisted, or decoded. Once a payload proves a
/// connection belongs to the game, both directions of that exact four-tuple
/// are emitted. This proves protocol ownership only; it does not claim a
/// launcher, geographic region, or exact client build.
pub struct SignatureFlowCapture<S> {
    source: S,
    signature: FlowSignature,
    config: SignatureFlowCaptureConfig,
    metadata: CaptureSourceMetadata,
    confirmed: BTreeMap<ConnectionKey, ConfirmedFlow>,
    epochs: BTreeMap<ConnectionKey, ConnectionEpoch>,
    pending: VecDeque<PendingFrame>,
    ready: VecDeque<CapturedFrame>,
    pending_bytes: usize,
    prefix_scratch: Vec<u8>,
    prefix_segment_scratch: Vec<(u32, usize)>,
    next_sequence: u64,
    next_epoch: u64,
    metrics: SignatureFlowCaptureMetrics,
    source_finished: bool,
    #[cfg(test)]
    reclaim_pending_scans: std::cell::Cell<usize>,
}

impl<S: std::fmt::Debug> std::fmt::Debug for SignatureFlowCapture<S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignatureFlowCapture")
            .field("source", &self.source)
            .field("config", &self.config)
            .field("metadata", &self.metadata)
            .field("confirmed", &self.confirmed)
            .field("pending_frames", &self.pending.len())
            .field("ready_frames", &self.ready.len())
            .field("metrics", &self.metrics)
            .field("source_finished", &self.source_finished)
            .finish()
    }
}

impl<S: CaptureSource> SignatureFlowCapture<S> {
    pub fn new(
        source: S,
        signature: TcpPayloadSignature,
        config: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        Self::with_signature(source, FlowSignature::Packet(signature), config)
    }

    /// Creates a filter whose classifier receives a bounded, sequence-aware,
    /// contiguous TCP payload prefix assembled entirely inside this privacy boundary.
    pub fn new_prefix(
        source: S,
        signature: TcpPayloadPrefixSignature,
        config: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        Self::with_signature(source, FlowSignature::Prefix(signature), config)
    }

    fn with_signature(
        source: S,
        signature: FlowSignature,
        config: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let config = config.validate().map_err(|error| CaptureError::Adapter {
            adapter: "signature-flow-filter".into(),
            message: error.to_string(),
        })?;
        let (link_types, file_format) = {
            let source_metadata = source.metadata();
            (
                source_metadata.link_types.clone(),
                source_metadata.file_format,
            )
        };
        Ok(Self {
            source,
            signature,
            config,
            metadata: CaptureSourceMetadata {
                source_id: "signature-filtered-live".into(),
                display_name: "Protocol-signature-filtered live capture".into(),
                kind: CaptureSourceKind::Live,
                link_types,
                file_format,
            },
            confirmed: BTreeMap::new(),
            epochs: BTreeMap::new(),
            pending: VecDeque::new(),
            ready: VecDeque::new(),
            pending_bytes: 0,
            prefix_scratch: Vec::with_capacity(MAX_TCP_SIGNATURE_PREFIX_BYTES),
            prefix_segment_scratch: Vec::new(),
            next_sequence: 1,
            next_epoch: 1,
            metrics: SignatureFlowCaptureMetrics::default(),
            source_finished: false,
            #[cfg(test)]
            reclaim_pending_scans: std::cell::Cell::new(0),
        })
    }

    pub fn metrics(&self) -> &SignatureFlowCaptureMetrics {
        &self.metrics
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        self.confirmed
            .values()
            .map(|flow| flow.connection)
            .collect()
    }

    #[cfg(windows)]
    pub(crate) fn source(&self) -> &S {
        &self.source
    }

    fn emit(&mut self, mut frame: CapturedFrame) {
        frame.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.metrics.emitted_frames = self.metrics.emitted_frames.saturating_add(1);
        self.metrics.emitted_bytes = self
            .metrics
            .emitted_bytes
            .saturating_add(frame.bytes.len() as u64);
        self.ready.push_back(frame);
    }

    fn discard_pending(&mut self, pending: PendingFrame) {
        self.pending_bytes = self.pending_bytes.saturating_sub(pending.frame.bytes.len());
        self.metrics.unidentified_frames_discarded =
            self.metrics.unidentified_frames_discarded.saturating_add(1);
    }

    fn purge_unknown_where(&mut self, mut predicate: impl FnMut(&PendingFrame) -> bool) {
        let mut retained = VecDeque::with_capacity(self.pending.len());
        while let Some(pending) = self.pending.pop_front() {
            if pending.disposition == PendingDisposition::Unknown && predicate(&pending) {
                self.discard_pending(pending);
            } else {
                retained.push_back(pending);
            }
        }
        self.pending = retained;
    }

    fn begin_or_continue_epoch(
        &mut self,
        key: ConnectionKey,
        flow: crate::process_filter::DirectedTcpFlow,
        syn: bool,
        ack: bool,
        payload_sequence: u32,
        observed_micros: u64,
    ) -> u64 {
        let fresh_syn = syn
            && (self.confirmed.contains_key(&key)
                || self.epochs.get(&key).is_some_and(|epoch| {
                    if ack {
                        epoch
                            .syn_roots
                            .get(&flow)
                            .is_some_and(|root| *root != payload_sequence)
                    } else {
                        epoch.primary_syn != Some((flow, payload_sequence))
                    }
                }));
        if fresh_syn {
            self.confirmed.remove(&key);
            self.purge_unknown_where(|pending| pending.connection == key);
            self.epochs.remove(&key);
        }
        if !self.epochs.contains_key(&key) {
            self.enforce_epoch_limit();
            let id = self.next_epoch;
            self.next_epoch = self.next_epoch.saturating_add(1);
            self.epochs.insert(
                key,
                ConnectionEpoch {
                    id,
                    last_seen_micros: observed_micros,
                    primary_syn: None,
                    syn_roots: BTreeMap::new(),
                    rejected_flows: BTreeSet::new(),
                },
            );
        }
        let epoch = self.epochs.get_mut(&key).expect("epoch was inserted");
        epoch.last_seen_micros = observed_micros;
        if syn {
            epoch.syn_roots.insert(flow, payload_sequence);
            if !ack {
                epoch.primary_syn = Some((flow, payload_sequence));
            }
        }
        epoch.id
    }

    fn purge_epoch(&mut self, key: ConnectionKey) {
        self.epochs.remove(&key);
        self.confirmed.remove(&key);
        let mut retained = VecDeque::with_capacity(self.pending.len());
        while let Some(pending) = self.pending.pop_front() {
            if pending.connection == key {
                self.discard_pending(pending);
                self.metrics.pending_limit_evictions =
                    self.metrics.pending_limit_evictions.saturating_add(1);
            } else {
                retained.push_back(pending);
            }
        }
        self.pending = retained;
    }

    fn enforce_epoch_limit(&mut self) {
        let limit = self
            .config
            .max_pending_frames
            .clamp(1, MAX_TRACKED_PREFIX_EPOCHS);
        while self.epochs.len() >= limit {
            let Some(oldest) = self
                .epochs
                .iter()
                .min_by_key(|(key, epoch)| (epoch.last_seen_micros, **key))
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.purge_epoch(oldest);
        }
    }

    fn reclaim_idle_epochs(&mut self, observed_micros: u64) {
        let ttl = self.config.pending_ttl_micros;
        let idle = self
            .epochs
            .iter()
            .filter(|(key, epoch)| {
                observed_micros.saturating_sub(epoch.last_seen_micros) >= ttl
                    && !self.confirmed.contains_key(key)
                    && {
                        #[cfg(test)]
                        self.reclaim_pending_scans
                            .set(self.reclaim_pending_scans.get().saturating_add(1));
                        !self
                            .pending
                            .iter()
                            .any(|pending| pending.connection == **key)
                    }
            })
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();
        for key in idle {
            self.epochs.remove(&key);
        }
    }

    fn flow_is_rejected(
        &self,
        key: ConnectionKey,
        epoch: u64,
        flow: crate::process_filter::DirectedTcpFlow,
    ) -> bool {
        self.epochs
            .get(&key)
            .is_some_and(|state| state.id == epoch && state.rejected_flows.contains(&flow))
    }

    fn reject_flow(
        &mut self,
        key: ConnectionKey,
        epoch: u64,
        flow: crate::process_filter::DirectedTcpFlow,
    ) {
        if let Some(state) = self.epochs.get_mut(&key).filter(|state| state.id == epoch) {
            state.rejected_flows.insert(flow);
        }
        self.purge_unknown_where(|pending| pending.flow == flow && pending.epoch == epoch);
    }

    fn discard_expired(&mut self, observed_micros: u64) {
        let ttl = self.config.pending_ttl_micros;
        let mut retained = VecDeque::with_capacity(self.pending.len());
        while let Some(pending) = self.pending.pop_front() {
            if pending.disposition == PendingDisposition::Unknown
                && pending.frame.observed_micros.saturating_add(ttl) <= observed_micros
            {
                self.discard_pending(pending);
            } else {
                retained.push_back(pending);
            }
        }
        self.pending = retained;
    }

    fn enforce_limits(&mut self) {
        while self.pending.len() > self.config.max_pending_frames
            || self.pending_bytes > self.config.max_pending_bytes
        {
            let Some(pending) = self.pending.pop_front() else {
                break;
            };
            self.pending_bytes = self.pending_bytes.saturating_sub(pending.frame.bytes.len());
            if pending.disposition == PendingDisposition::Confirmed {
                self.emit(pending.frame);
            } else {
                self.metrics.unidentified_frames_discarded =
                    self.metrics.unidentified_frames_discarded.saturating_add(1);
                self.metrics.pending_limit_evictions =
                    self.metrics.pending_limit_evictions.saturating_add(1);
            }
        }
    }

    fn drain_resolved_front(&mut self) {
        while self
            .pending
            .front()
            .is_some_and(|pending| pending.disposition == PendingDisposition::Confirmed)
        {
            let pending = self.pending.pop_front().expect("front exists");
            self.pending_bytes = self.pending_bytes.saturating_sub(pending.frame.bytes.len());
            self.emit(pending.frame);
        }
    }

    fn classify_prefix(
        &mut self,
        flow: crate::process_filter::DirectedTcpFlow,
        epoch: u64,
        signature: TcpPayloadPrefixSignature,
    ) -> PrefixEvaluation {
        self.prefix_segment_scratch.clear();
        for (index, pending) in self.pending.iter().enumerate() {
            if pending.flow == flow
                && pending.epoch == epoch
                && pending.disposition == PendingDisposition::Unknown
                && extract_tcp_frame(&pending.frame).is_some_and(|view| !view.payload.is_empty())
            {
                self.prefix_segment_scratch
                    .push((pending.payload_sequence, index));
            }
        }
        let root = self.epochs.get(&flow.key()).and_then(|state| {
            (state.id == epoch)
                .then(|| state.syn_roots.get(&flow).copied())
                .flatten()
        });
        let Some(anchor) = root.or_else(|| {
            self.prefix_segment_scratch
                .first()
                .map(|(sequence, _)| *sequence)
        }) else {
            return PrefixEvaluation::Classified(TcpPayloadSignatureResult::NeedMore);
        };
        self.prefix_segment_scratch
            .sort_by_key(|(sequence, _)| sequence.wrapping_sub(anchor) as i32);

        self.prefix_scratch.clear();
        for (sequence, index) in self.prefix_segment_scratch.iter().copied() {
            let Some(payload) = self
                .pending
                .get(index)
                .and_then(|pending| extract_tcp_frame(&pending.frame))
                .map(|view| view.payload)
            else {
                continue;
            };
            let offset = sequence.wrapping_sub(anchor) as i32;
            if offset < 0 {
                return PrefixEvaluation::ConflictingOverlap;
            }
            let offset = offset as usize;
            if offset > self.prefix_scratch.len() {
                return PrefixEvaluation::Classified(TcpPayloadSignatureResult::NeedMore);
            }
            let overlap = payload
                .len()
                .min(self.prefix_scratch.len().saturating_sub(offset));
            if self.prefix_scratch[offset..offset + overlap] != payload[..overlap] {
                return PrefixEvaluation::ConflictingOverlap;
            }
            let remaining = &payload[overlap..];
            let available =
                MAX_TCP_SIGNATURE_PREFIX_BYTES.saturating_sub(self.prefix_scratch.len());
            if available == 0 {
                break;
            }
            let take = remaining.len().min(available);
            self.prefix_scratch.extend_from_slice(&remaining[..take]);
            if take < remaining.len() {
                break;
            }
        }
        let result = signature(&self.prefix_scratch);
        PrefixEvaluation::Classified(
            if self.prefix_scratch.len() >= MAX_TCP_SIGNATURE_PREFIX_BYTES
                && result == TcpPayloadSignatureResult::NeedMore
            {
                TcpPayloadSignatureResult::Reject
            } else {
                result
            },
        )
    }

    fn expire_confirmed(&mut self, observed_micros: u64) {
        let idle_timeout = self.config.confirmed_idle_timeout_micros;
        self.confirmed
            .retain(|_, flow| observed_micros.saturating_sub(flow.last_seen_micros) < idle_timeout);
        self.reclaim_idle_epochs(observed_micros);
    }

    fn confirm(
        &mut self,
        key: ConnectionKey,
        epoch: u64,
        connection: TcpConnection,
        observed_micros: u64,
    ) {
        self.metrics.signature_matches = self.metrics.signature_matches.saturating_add(1);
        if self
            .confirmed
            .insert(
                key,
                ConfirmedFlow {
                    connection,
                    last_seen_micros: observed_micros,
                    epoch,
                },
            )
            .is_none()
        {
            self.metrics.confirmed_connections =
                self.metrics.confirmed_connections.saturating_add(1);
        }

        for pending in &mut self.pending {
            if pending.connection == key && pending.epoch == epoch {
                pending.disposition = PendingDisposition::Confirmed;
            }
        }
        self.drain_resolved_front();
    }

    fn ingest(&mut self, frame: CapturedFrame) {
        self.metrics.ingress_frames = self.metrics.ingress_frames.saturating_add(1);
        self.metrics.ingress_bytes = self
            .metrics
            .ingress_bytes
            .saturating_add(frame.bytes.len() as u64);
        if !self.metadata.link_types.contains(&frame.link_type) {
            self.metadata.link_types.push(frame.link_type);
        }

        let Some(view) = extract_tcp_frame(&frame) else {
            self.metrics.non_tcp_frames_discarded =
                self.metrics.non_tcp_frames_discarded.saturating_add(1);
            return;
        };
        let flow = view.flow;
        let key = flow.key();
        let payload_sequence = view.payload_sequence;
        let syn = view.syn;
        let ack = view.ack;
        let fin = view.fin;
        let rst = view.rst;
        let packet_classification = match self.signature {
            FlowSignature::Packet(signature) => signature(view.payload)
                .map(TcpPayloadSignatureResult::Match)
                .unwrap_or(TcpPayloadSignatureResult::Reject),
            FlowSignature::Prefix(_) => TcpPayloadSignatureResult::NeedMore,
        };
        self.discard_expired(frame.observed_micros);
        self.expire_confirmed(frame.observed_micros);
        let prefix_mode = matches!(self.signature, FlowSignature::Prefix(_));
        if !prefix_mode && syn {
            self.confirmed.remove(&key);
            self.purge_unknown_where(|pending| pending.connection == key);
        }
        let epoch = if prefix_mode {
            self.begin_or_continue_epoch(
                key,
                flow,
                syn,
                ack,
                payload_sequence,
                frame.observed_micros,
            )
        } else {
            0
        };
        let already_confirmed = self
            .confirmed
            .get_mut(&key)
            .filter(|confirmed| confirmed.epoch == epoch)
            .map(|confirmed| confirmed.last_seen_micros = frame.observed_micros)
            .is_some();

        let frame_bytes = frame.bytes.len();
        let observed_micros = frame.observed_micros;
        self.pending_bytes = self.pending_bytes.saturating_add(frame_bytes);
        self.pending.push_back(PendingFrame {
            frame,
            connection: key,
            flow,
            payload_sequence,
            epoch,
            disposition: if already_confirmed {
                PendingDisposition::Confirmed
            } else {
                PendingDisposition::Unknown
            },
        });
        self.metrics.peak_pending_frames = self.metrics.peak_pending_frames.max(self.pending.len());
        self.metrics.peak_pending_bytes = self.metrics.peak_pending_bytes.max(self.pending_bytes);

        let classification = if already_confirmed {
            PrefixEvaluation::Classified(TcpPayloadSignatureResult::NeedMore)
        } else if prefix_mode && self.flow_is_rejected(key, epoch, flow) {
            PrefixEvaluation::Classified(TcpPayloadSignatureResult::Reject)
        } else {
            match self.signature {
                FlowSignature::Packet(_) => PrefixEvaluation::Classified(packet_classification),
                FlowSignature::Prefix(signature) => {
                    // Prefix assembly may retain several segments, so enforce the
                    // existing global queue bounds before inspecting candidate bytes.
                    self.enforce_limits();
                    self.classify_prefix(flow, epoch, signature)
                }
            }
        };
        match classification {
            PrefixEvaluation::Classified(TcpPayloadSignatureResult::Match(direction)) => {
                let connection = match direction {
                    TcpPayloadDirection::ClientToServer => {
                        TcpConnection::new(flow.source, flow.destination)
                    }
                    TcpPayloadDirection::ServerToClient => {
                        TcpConnection::new(flow.destination, flow.source)
                    }
                };
                self.confirm(key, epoch, connection, observed_micros);
            }
            PrefixEvaluation::Classified(TcpPayloadSignatureResult::Reject)
            | PrefixEvaluation::ConflictingOverlap => {
                if matches!(self.signature, FlowSignature::Prefix(_)) {
                    self.reject_flow(key, epoch, flow);
                }
            }
            PrefixEvaluation::Classified(TcpPayloadSignatureResult::NeedMore) => {}
        }
        if matches!(self.signature, FlowSignature::Packet(_)) {
            self.enforce_limits();
        }
        self.drain_resolved_front();
        if fin || rst {
            self.confirmed.remove(&key);
            if prefix_mode {
                self.epochs.remove(&key);
            }
            self.purge_unknown_where(|pending| pending.connection == key && pending.epoch == epoch);
            self.drain_resolved_front();
        }
    }

    fn finish_source(&mut self) {
        while let Some(pending) = self.pending.pop_front() {
            self.pending_bytes = self.pending_bytes.saturating_sub(pending.frame.bytes.len());
            if pending.disposition == PendingDisposition::Confirmed {
                self.emit(pending.frame);
            } else {
                self.metrics.unidentified_frames_discarded =
                    self.metrics.unidentified_frames_discarded.saturating_add(1);
            }
        }
        self.source_finished = true;
    }
}

impl<S: CaptureSource> CaptureSource for SignatureFlowCapture<S> {
    fn metadata(&self) -> &CaptureSourceMetadata {
        &self.metadata
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        loop {
            if let Some(frame) = self.ready.pop_front() {
                return Ok(Some(frame));
            }
            if self.source_finished {
                return Ok(None);
            }
            match self.source.next_frame()? {
                Some(frame) => self.ingest(frame),
                None => self.finish_source(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        net::{IpAddr, Ipv4Addr},
    };

    use bytes::Bytes;
    use etherparse::PacketBuilder;

    use super::*;
    use crate::{CaptureLinkType, TcpEndpoint, TimestampNormalization, ValidatedCapture};

    #[derive(Debug)]
    struct FixtureSource {
        metadata: CaptureSourceMetadata,
        frames: VecDeque<CapturedFrame>,
    }

    impl CaptureSource for FixtureSource {
        fn metadata(&self) -> &CaptureSourceMetadata {
            &self.metadata
        }

        fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
            Ok(self.frames.pop_front())
        }
    }

    fn endpoint(last: u8, port: u16) -> TcpEndpoint {
        TcpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    fn frame(
        sequence: u64,
        observed_micros: u64,
        source: TcpEndpoint,
        destination: TcpEndpoint,
        payload: &[u8],
    ) -> CapturedFrame {
        tcp_frame(
            sequence,
            observed_micros,
            source,
            destination,
            sequence as u32,
            payload,
        )
    }

    fn tcp_frame(
        sequence: u64,
        observed_micros: u64,
        source: TcpEndpoint,
        destination: TcpEndpoint,
        tcp_sequence: u32,
        payload: &[u8],
    ) -> CapturedFrame {
        tcp_frame_with_flag(
            sequence,
            observed_micros,
            source,
            destination,
            tcp_sequence,
            payload,
            None,
        )
    }

    fn tcp_frame_with_flag(
        sequence: u64,
        observed_micros: u64,
        source: TcpEndpoint,
        destination: TcpEndpoint,
        tcp_sequence: u32,
        payload: &[u8],
        flag: Option<&str>,
    ) -> CapturedFrame {
        let (IpAddr::V4(source_address), IpAddr::V4(destination_address)) =
            (source.address, destination.address)
        else {
            unreachable!()
        };
        let builder = PacketBuilder::ethernet2([1; 6], [2; 6])
            .ipv4(source_address.octets(), destination_address.octets(), 64)
            .tcp(source.port, destination.port, tcp_sequence, 1_024);
        let builder = match flag {
            Some("syn") => builder.syn(),
            Some("fin") => builder.fin(),
            Some("rst") => builder.rst(),
            _ => builder,
        };
        let mut bytes = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut bytes, payload).unwrap();
        CapturedFrame {
            sequence,
            observed_micros,
            source_timestamp_nanos: Some(observed_micros as i64 * 1_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: Some(0),
            link_type: CaptureLinkType::Ethernet,
            original_length: bytes.len() as u32,
            bytes: Bytes::from(bytes),
        }
    }

    fn source(frames: Vec<CapturedFrame>) -> FixtureSource {
        FixtureSource {
            metadata: CaptureSourceMetadata {
                source_id: "broad-test-ingress".into(),
                display_name: "broad test ingress".into(),
                kind: CaptureSourceKind::Live,
                link_types: vec![CaptureLinkType::Ethernet],
                file_format: None,
            },
            frames: frames.into(),
        }
    }

    fn signature(payload: &[u8]) -> Option<TcpPayloadDirection> {
        payload
            .starts_with(b"BPSR")
            .then_some(TcpPayloadDirection::ServerToClient)
    }

    fn prefix_signature(payload: &[u8]) -> TcpPayloadSignatureResult {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        if SIGNATURE.starts_with(payload) {
            if payload.len() == SIGNATURE.len() {
                TcpPayloadSignatureResult::Match(TcpPayloadDirection::ServerToClient)
            } else {
                TcpPayloadSignatureResult::NeedMore
            }
        } else if payload.starts_with(SIGNATURE) {
            TcpPayloadSignatureResult::Match(TcpPayloadDirection::ServerToClient)
        } else {
            TcpPayloadSignatureResult::Reject
        }
    }

    fn always_needs_more(_: &[u8]) -> TcpPayloadSignatureResult {
        TcpPayloadSignatureResult::NeedMore
    }

    #[test]
    fn prefix_signature_matches_across_every_tcp_split_boundary() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        for split in 1..SIGNATURE.len() {
            let frames = vec![
                tcp_frame(1, 0, server, client, 10_000, &SIGNATURE[..split]),
                tcp_frame(
                    2,
                    1,
                    server,
                    client,
                    10_000 + split as u32,
                    &SIGNATURE[split..],
                ),
            ];
            let mut filtered = SignatureFlowCapture::new_prefix(
                source(frames),
                prefix_signature,
                SignatureFlowCaptureConfig::default(),
            )
            .unwrap();

            assert!(filtered.next_frame().unwrap().is_some(), "split {split}");
            assert!(filtered.next_frame().unwrap().is_some(), "split {split}");
            assert!(filtered.next_frame().unwrap().is_none(), "split {split}");
            assert_eq!(
                filtered.confirmed_connections(),
                vec![TcpConnection::new(client, server)],
                "split {split}"
            );
        }
    }

    #[test]
    fn out_of_order_and_retransmitted_segments_form_one_private_prefix() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let split = 7;
        let frames = vec![
            tcp_frame_with_flag(0, 0, server, client, 19_999, b"", Some("syn")),
            tcp_frame(
                1,
                1,
                server,
                client,
                20_000 + split as u32,
                &SIGNATURE[split..],
            ),
            tcp_frame(2, 2, server, client, 20_000, &SIGNATURE[..split]),
            tcp_frame(
                3,
                3,
                server,
                client,
                20_000 + split as u32,
                &SIGNATURE[split..],
            ),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(
            filtered.confirmed_connections(),
            vec![TcpConnection::new(client, server)]
        );
    }

    #[test]
    fn prefix_assembly_cannot_bypass_the_global_pending_frame_limit() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let frames = vec![
            tcp_frame(1, 0, server, client, 30_000, &SIGNATURE[..7]),
            tcp_frame(2, 1, server, client, 30_007, &SIGNATURE[7..]),
        ];
        let config = SignatureFlowCaptureConfig {
            max_pending_frames: 1,
            ..SignatureFlowCaptureConfig::default()
        };
        let mut filtered =
            SignatureFlowCapture::new_prefix(source(frames), prefix_signature, config).unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert!(filtered.confirmed_connections().is_empty());
        assert_eq!(filtered.metrics().emitted_frames, 0);
        assert_eq!(filtered.metrics().pending_limit_evictions, 1);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 2);
    }

    #[test]
    fn expired_prefix_bytes_cannot_later_confirm_or_enter_evidence() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let unrelated_client = endpoint(3, 33_000);
        let unrelated_server = endpoint(4, 44_000);
        let frames = vec![
            tcp_frame(1, 0, server, client, 40_000, &SIGNATURE[..7]),
            tcp_frame(
                2,
                101,
                unrelated_server,
                unrelated_client,
                50_000,
                b"private",
            ),
            tcp_frame(3, 102, server, client, 40_007, &SIGNATURE[7..]),
        ];
        let config = SignatureFlowCaptureConfig {
            pending_ttl_micros: 100,
            ..SignatureFlowCaptureConfig::default()
        };
        let mut filtered =
            SignatureFlowCapture::new_prefix(source(frames), prefix_signature, config).unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert!(filtered.confirmed_connections().is_empty());
        assert_eq!(filtered.metrics().emitted_frames, 0);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 3);
    }

    #[test]
    fn wrapped_tcp_sequences_still_form_a_contiguous_private_prefix() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let split = 7;
        let first_sequence = u32::MAX - 3;
        let second_sequence = first_sequence.wrapping_add(split as u32);
        let frames = vec![
            tcp_frame_with_flag(
                0,
                0,
                server,
                client,
                first_sequence.wrapping_sub(1),
                b"",
                Some("syn"),
            ),
            tcp_frame(1, 1, server, client, second_sequence, &SIGNATURE[split..]),
            tcp_frame(2, 2, server, client, first_sequence, &SIGNATURE[..split]),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(
            filtered.confirmed_connections(),
            vec![TcpConnection::new(client, server)]
        );
    }

    #[test]
    fn conflicting_overlap_is_terminal_but_exact_retransmission_is_accepted() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let conflicting = vec![
            tcp_frame_with_flag(1, 0, server, client, 59_999, b"", Some("syn")),
            tcp_frame(2, 1, server, client, 60_000, &SIGNATURE[..10]),
            tcp_frame(3, 2, server, client, 60_005, b"wrong"),
            tcp_frame(4, 3, server, client, 60_010, &SIGNATURE[10..]),
        ];
        let mut rejected = SignatureFlowCapture::new_prefix(
            source(conflicting),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();
        assert!(rejected.next_frame().unwrap().is_none());
        assert!(rejected.confirmed_connections().is_empty());
        assert_eq!(rejected.metrics().emitted_frames, 0);
        assert_eq!(rejected.metrics().unidentified_frames_discarded, 4);

        let exact = vec![
            tcp_frame_with_flag(1, 0, server, client, 69_999, b"", Some("syn")),
            tcp_frame(2, 1, server, client, 70_000, &SIGNATURE[..10]),
            tcp_frame(3, 2, server, client, 70_000, &SIGNATURE[..10]),
            tcp_frame(4, 3, server, client, 70_010, &SIGNATURE[10..]),
        ];
        let mut accepted = SignatureFlowCapture::new_prefix(
            source(exact),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();
        for _ in 0..4 {
            assert!(accepted.next_frame().unwrap().is_some());
        }
        assert!(accepted.next_frame().unwrap().is_none());
        assert_eq!(
            accepted.confirmed_connections(),
            vec![TcpConnection::new(client, server)]
        );
    }

    #[test]
    fn classifier_reject_is_terminal_for_that_direction_and_epoch() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let frames = vec![
            tcp_frame_with_flag(1, 0, server, client, 74_999, b"", Some("syn")),
            tcp_frame(2, 1, server, client, 75_000, b"X"),
            tcp_frame(3, 2, server, client, 75_000, SIGNATURE),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert!(filtered.confirmed_connections().is_empty());
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 3);
    }

    #[test]
    fn a_gap_before_a_matching_segment_cannot_confirm_or_release_the_epoch() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let frames = vec![
            tcp_frame_with_flag(1, 0, server, client, 79_999, b"", Some("syn")),
            tcp_frame(2, 1, server, client, 80_100, SIGNATURE),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert!(filtered.confirmed_connections().is_empty());
        assert_eq!(filtered.metrics().emitted_frames, 0);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 2);
    }

    #[test]
    fn fin_and_fresh_syn_revoke_confirmation_until_the_tuple_reproves() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let frames = vec![
            tcp_frame(1, 0, server, client, 90_000, SIGNATURE),
            tcp_frame_with_flag(2, 1, server, client, 90_020, b"", Some("fin")),
            tcp_frame_with_flag(3, 2, server, client, 99_999, b"", Some("syn")),
            tcp_frame(4, 3, server, client, 100_000, b"private tuple reuse"),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_none());
        assert!(filtered.confirmed_connections().is_empty());
        assert_eq!(filtered.metrics().emitted_frames, 2);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 2);
    }

    #[test]
    fn rst_purges_an_unconfirmed_epoch_and_its_prefix() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let frames = vec![
            tcp_frame_with_flag(1, 0, server, client, 129_999, b"", Some("syn")),
            tcp_frame(2, 1, server, client, 130_000, &SIGNATURE[..7]),
            tcp_frame_with_flag(3, 2, server, client, 130_007, b"", Some("rst")),
            tcp_frame(4, 3, server, client, 130_007, &SIGNATURE[7..]),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert!(filtered.confirmed_connections().is_empty());
        assert_eq!(filtered.metrics().emitted_frames, 0);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 4);
    }

    #[test]
    fn interleaved_confirmations_release_frames_in_original_capture_order() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let first_client = endpoint(1, 31_000);
        let first_server = endpoint(2, 32_000);
        let second_client = endpoint(3, 33_000);
        let second_server = endpoint(4, 44_000);
        let frames = vec![
            tcp_frame(1, 10, first_server, first_client, 110_000, &SIGNATURE[..7]),
            tcp_frame(2, 11, second_server, second_client, 120_000, SIGNATURE),
            tcp_frame(3, 12, first_server, first_client, 110_007, &SIGNATURE[7..]),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        let observed = std::iter::from_fn(|| filtered.next_frame().transpose())
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .map(|frame| frame.observed_micros)
            .collect::<Vec<_>>();
        assert_eq!(observed, vec![10, 11, 12]);
    }

    #[test]
    fn rejected_tuple_epochs_are_bounded_and_legacy_mode_tracks_none() {
        let mut prefix_frames = Vec::new();
        let mut packet_frames = Vec::new();
        for index in 1..=12_u8 {
            prefix_frames.push(tcp_frame(
                u64::from(index),
                u64::from(index),
                endpoint(index, 30_000 + u16::from(index)),
                endpoint(200, 40_000),
                1_000,
                b"X",
            ));
            packet_frames.push(tcp_frame(
                u64::from(index),
                u64::from(index),
                endpoint(index, 31_000 + u16::from(index)),
                endpoint(201, 41_000),
                2_000,
                b"private",
            ));
        }
        let config = SignatureFlowCaptureConfig {
            max_pending_frames: 4,
            ..SignatureFlowCaptureConfig::default()
        };
        let mut prefix =
            SignatureFlowCapture::new_prefix(source(prefix_frames), prefix_signature, config)
                .unwrap();
        assert!(prefix.next_frame().unwrap().is_none());
        assert!(prefix.epochs.len() <= 4);
        assert!(prefix.confirmed_connections().is_empty());

        let mut legacy =
            SignatureFlowCapture::new(source(packet_frames), signature, config).unwrap();
        assert!(legacy.next_frame().unwrap().is_none());
        assert!(legacy.epochs.is_empty());
    }

    #[test]
    fn fresh_and_confirmed_epochs_do_not_scan_the_pending_queue_during_reclamation() {
        let config = SignatureFlowCaptureConfig {
            pending_ttl_micros: 100,
            ..SignatureFlowCaptureConfig::default()
        };
        let mut filtered =
            SignatureFlowCapture::new_prefix(source(Vec::new()), prefix_signature, config).unwrap();
        for index in 1..=512_u16 {
            let key = ConnectionKey::new(
                endpoint((index % 250 + 1) as u8, 10_000 + index),
                endpoint(251, 20_000),
            );
            filtered.epochs.insert(
                key,
                ConnectionEpoch {
                    id: u64::from(index),
                    last_seen_micros: 1_000,
                    ..ConnectionEpoch::default()
                },
            );
        }
        for index in 513..=768_u16 {
            let client = endpoint((index % 250 + 1) as u8, 10_000 + index);
            let server = endpoint(252, 20_000);
            let key = ConnectionKey::new(client, server);
            filtered.epochs.insert(
                key,
                ConnectionEpoch {
                    id: u64::from(index),
                    last_seen_micros: 0,
                    ..ConnectionEpoch::default()
                },
            );
            filtered.confirmed.insert(
                key,
                ConfirmedFlow {
                    connection: TcpConnection::new(client, server),
                    last_seen_micros: 1_000,
                    epoch: u64::from(index),
                },
            );
        }

        filtered.reclaim_idle_epochs(1_099);

        assert_eq!(filtered.epochs.len(), 768);
        assert_eq!(filtered.reclaim_pending_scans.get(), 0);
    }

    #[test]
    fn confirmed_flow_expiry_reclaims_epoch_without_a_teardown_packet() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let old_key = ConnectionKey::new(client, server);
        let frames = vec![
            tcp_frame(1, 0, server, client, 140_000, SIGNATURE),
            tcp_frame(
                2,
                SignatureFlowCaptureConfig::default().confirmed_idle_timeout_micros + 1,
                endpoint(3, 33_000),
                endpoint(4, 44_000),
                150_000,
                b"X",
            ),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_none());
        assert!(!filtered.epochs.contains_key(&old_key));
        assert!(filtered.confirmed_connections().is_empty());
    }

    #[test]
    fn need_more_at_the_exact_prefix_ceiling_is_terminal() {
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let half = vec![0_u8; MAX_TCP_SIGNATURE_PREFIX_BYTES / 2];
        let frames = vec![
            tcp_frame_with_flag(1, 0, server, client, 159_999, b"", Some("syn")),
            tcp_frame(2, 1, server, client, 160_000, &half),
            tcp_frame(3, 2, server, client, 160_000 + half.len() as u32, &half),
            tcp_frame(
                4,
                3,
                server,
                client,
                160_000 + MAX_TCP_SIGNATURE_PREFIX_BYTES as u32,
                b"over cap",
            ),
        ];
        let mut filtered = SignatureFlowCapture::new_prefix(
            source(frames),
            always_needs_more,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert!(filtered.confirmed_connections().is_empty());
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 4);
    }

    #[test]
    fn exact_signature_releases_only_its_bidirectional_flow() {
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let unrelated_client = endpoint(3, 33_000);
        let unrelated_server = endpoint(4, 44_000);
        let frames = vec![
            frame(1, 0, unrelated_client, unrelated_server, b"private"),
            frame(2, 1, client, server, b"preface"),
            frame(3, 2, server, client, b"BPSR scene"),
            frame(4, 3, client, server, b"request"),
        ];
        let filtered = SignatureFlowCapture::new(
            source(frames),
            signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();
        let mut validated = ValidatedCapture::new(filtered);

        assert_eq!(validated.next_frame().unwrap().unwrap().observed_micros, 1);
        assert_eq!(validated.next_frame().unwrap().unwrap().observed_micros, 2);
        assert_eq!(validated.next_frame().unwrap().unwrap().observed_micros, 3);
        assert!(validated.next_frame().unwrap().is_none());
        assert_eq!(
            validated.source.confirmed_connections(),
            vec![TcpConnection::new(client, server)]
        );
        assert_eq!(validated.source.metrics().unidentified_frames_discarded, 1);
    }

    #[test]
    fn unidentified_traffic_never_leaves_the_capture_boundary() {
        let frames = vec![frame(
            1,
            0,
            endpoint(3, 33_000),
            endpoint(4, 44_000),
            b"not the game",
        )];
        let mut filtered = SignatureFlowCapture::new(
            source(frames),
            signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(filtered.metrics().emitted_frames, 0);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 1);
        assert!(filtered.confirmed_connections().is_empty());
    }

    #[test]
    fn pending_limits_evict_unknown_frames_before_any_consumer_sees_them() {
        let frames = vec![
            frame(1, 0, endpoint(3, 33_000), endpoint(4, 44_000), b"one"),
            frame(2, 1, endpoint(4, 44_000), endpoint(3, 33_000), b"two"),
        ];
        let config = SignatureFlowCaptureConfig {
            pending_ttl_micros: 100,
            confirmed_idle_timeout_micros: 1_000,
            max_pending_frames: 1,
            max_pending_bytes: 1_024,
        };
        let mut filtered = SignatureFlowCapture::new(source(frames), signature, config).unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(filtered.metrics().pending_limit_evictions, 1);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 2);
    }

    #[test]
    fn an_idle_four_tuple_must_prove_the_game_signature_again() {
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let frames = vec![
            frame(1, 0, server, client, b"BPSR scene"),
            frame(2, 10, client, server, b"game request"),
            frame(3, 1_010, client, server, b"reused private tuple"),
        ];
        let config = SignatureFlowCaptureConfig {
            pending_ttl_micros: 100,
            confirmed_idle_timeout_micros: 1_000,
            max_pending_frames: 16,
            max_pending_bytes: 1_024,
        };
        let mut filtered = SignatureFlowCapture::new(source(frames), signature, config).unwrap();

        assert_eq!(filtered.next_frame().unwrap().unwrap().observed_micros, 0);
        assert_eq!(filtered.next_frame().unwrap().unwrap().observed_micros, 10);
        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(filtered.metrics().emitted_frames, 2);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 1);
        assert!(filtered.confirmed_connections().is_empty());
    }

    #[test]
    fn a_late_signature_cannot_release_frames_older_than_the_transient_ttl() {
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let frames = vec![
            frame(1, 0, client, server, b"old preface"),
            frame(2, 101, server, client, b"BPSR scene"),
        ];
        let config = SignatureFlowCaptureConfig {
            pending_ttl_micros: 100,
            confirmed_idle_timeout_micros: 1_000,
            max_pending_frames: 16,
            max_pending_bytes: 1_024,
        };
        let mut filtered = SignatureFlowCapture::new(source(frames), signature, config).unwrap();

        assert_eq!(filtered.next_frame().unwrap().unwrap().observed_micros, 101);
        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(filtered.metrics().emitted_frames, 1);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 1);
    }
}
