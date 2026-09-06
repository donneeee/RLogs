use std::collections::{BTreeMap, VecDeque};

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

/// A classifier must return a direction only for an exact, game-specific
/// payload signature. Returning `None` keeps the frame inside the bounded
/// transient privacy buffer and prevents it from reaching persistence or
/// protocol decoding.
pub type TcpPayloadSignature = fn(&[u8]) -> Option<TcpPayloadDirection>;

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
}

#[derive(Debug, Clone, Copy)]
struct ConfirmedFlow {
    connection: TcpConnection,
    last_seen_micros: u64,
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
    signature: TcpPayloadSignature,
    config: SignatureFlowCaptureConfig,
    metadata: CaptureSourceMetadata,
    confirmed: BTreeMap<ConnectionKey, ConfirmedFlow>,
    pending: VecDeque<PendingFrame>,
    ready: VecDeque<CapturedFrame>,
    pending_bytes: usize,
    next_sequence: u64,
    last_emitted_micros: Option<u64>,
    metrics: SignatureFlowCaptureMetrics,
    source_finished: bool,
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
            pending: VecDeque::new(),
            ready: VecDeque::new(),
            pending_bytes: 0,
            next_sequence: 1,
            last_emitted_micros: None,
            metrics: SignatureFlowCaptureMetrics::default(),
            source_finished: false,
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
        if self
            .last_emitted_micros
            .is_some_and(|last| frame.observed_micros < last)
        {
            self.metrics.unidentified_frames_discarded =
                self.metrics.unidentified_frames_discarded.saturating_add(1);
            return;
        }
        self.last_emitted_micros = Some(frame.observed_micros);
        frame.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.metrics.emitted_frames = self.metrics.emitted_frames.saturating_add(1);
        self.metrics.emitted_bytes = self
            .metrics
            .emitted_bytes
            .saturating_add(frame.bytes.len() as u64);
        self.ready.push_back(frame);
    }

    fn discard_expired(&mut self, observed_micros: u64) {
        let ttl = self.config.pending_ttl_micros;
        let mut retained = VecDeque::with_capacity(self.pending.len());
        while let Some(pending) = self.pending.pop_front() {
            if pending.frame.observed_micros.saturating_add(ttl) <= observed_micros {
                self.pending_bytes = self.pending_bytes.saturating_sub(pending.frame.bytes.len());
                self.metrics.unidentified_frames_discarded =
                    self.metrics.unidentified_frames_discarded.saturating_add(1);
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
            self.metrics.unidentified_frames_discarded =
                self.metrics.unidentified_frames_discarded.saturating_add(1);
            self.metrics.pending_limit_evictions =
                self.metrics.pending_limit_evictions.saturating_add(1);
        }
    }

    fn expire_confirmed(&mut self, observed_micros: u64) {
        let idle_timeout = self.config.confirmed_idle_timeout_micros;
        self.confirmed
            .retain(|_, flow| observed_micros.saturating_sub(flow.last_seen_micros) < idle_timeout);
    }

    fn confirm(&mut self, key: ConnectionKey, connection: TcpConnection, observed_micros: u64) {
        self.metrics.signature_matches = self.metrics.signature_matches.saturating_add(1);
        if self
            .confirmed
            .insert(
                key,
                ConfirmedFlow {
                    connection,
                    last_seen_micros: observed_micros,
                },
            )
            .is_none()
        {
            self.metrics.confirmed_connections =
                self.metrics.confirmed_connections.saturating_add(1);
        }

        let mut retained = VecDeque::with_capacity(self.pending.len());
        while let Some(pending) = self.pending.pop_front() {
            self.pending_bytes = self.pending_bytes.saturating_sub(pending.frame.bytes.len());
            if pending.connection == key {
                self.emit(pending.frame);
            } else {
                self.pending_bytes = self.pending_bytes.saturating_add(pending.frame.bytes.len());
                retained.push_back(pending);
            }
        }
        self.pending = retained;
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
        let classification = (self.signature)(view.payload);
        self.discard_expired(frame.observed_micros);
        self.expire_confirmed(frame.observed_micros);
        if let Some(confirmed) = self.confirmed.get_mut(&key) {
            confirmed.last_seen_micros = frame.observed_micros;
            self.emit(frame);
            return;
        }

        let frame_bytes = frame.bytes.len();
        let observed_micros = frame.observed_micros;
        self.pending_bytes = self.pending_bytes.saturating_add(frame_bytes);
        self.pending.push_back(PendingFrame {
            frame,
            connection: key,
        });
        self.metrics.peak_pending_frames = self.metrics.peak_pending_frames.max(self.pending.len());
        self.metrics.peak_pending_bytes = self.metrics.peak_pending_bytes.max(self.pending_bytes);

        if let Some(direction) = classification {
            let connection = match direction {
                TcpPayloadDirection::ClientToServer => {
                    TcpConnection::new(flow.source, flow.destination)
                }
                TcpPayloadDirection::ServerToClient => {
                    TcpConnection::new(flow.destination, flow.source)
                }
            };
            self.confirm(key, connection, observed_micros);
        }
        self.enforce_limits();
    }

    fn finish_source(&mut self) {
        while let Some(pending) = self.pending.pop_front() {
            self.pending_bytes = self.pending_bytes.saturating_sub(pending.frame.bytes.len());
            self.metrics.unidentified_frames_discarded =
                self.metrics.unidentified_frames_discarded.saturating_add(1);
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
        let (IpAddr::V4(source_address), IpAddr::V4(destination_address)) =
            (source.address, destination.address)
        else {
            unreachable!()
        };
        let builder = PacketBuilder::ethernet2([1; 6], [2; 6])
            .ipv4(source_address.octets(), destination_address.octets(), 64)
            .tcp(source.port, destination.port, 1, 1_024);
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
