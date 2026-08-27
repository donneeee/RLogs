use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    net::IpAddr,
};

use etherparse::{EtherType, NetSlice, SlicedPacket, TransportSlice};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CaptureError, CaptureLinkType, CaptureSource, CaptureSourceKind, CaptureSourceMetadata,
    CapturedFrame,
};

const LINUX_SLL2_HEADER_LEN: usize = 20;
const NULL_LOOPBACK_HEADER_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TcpEndpoint {
    pub address: IpAddr,
    pub port: u16,
}

impl TcpEndpoint {
    pub const fn new(address: IpAddr, port: u16) -> Self {
        Self { address, port }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TcpConnection {
    pub client: TcpEndpoint,
    pub server: TcpEndpoint,
}

impl TcpConnection {
    pub const fn new(client: TcpEndpoint, server: TcpEndpoint) -> Self {
        Self { client, server }
    }

    fn key(self) -> ConnectionKey {
        ConnectionKey::new(self.client, self.server)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ConnectionKey {
    first: TcpEndpoint,
    second: TcpEndpoint,
}

impl ConnectionKey {
    fn new(first: TcpEndpoint, second: TcpEndpoint) -> Self {
        if first <= second {
            Self { first, second }
        } else {
            Self {
                first: second,
                second: first,
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DirectedTcpFlow {
    source: TcpEndpoint,
    destination: TcpEndpoint,
}

impl DirectedTcpFlow {
    fn key(self) -> ConnectionKey {
        ConnectionKey::new(self.source, self.destination)
    }
}

/// Supplies exact TCP connections currently owned by one OS process.
///
/// Implementations must never infer ownership from server address or port
/// alone. Both endpoints and the owning process ID have to come from the
/// platform socket table.
pub trait ProcessSocketOwner: Send {
    fn snapshot(&mut self) -> Result<Vec<TcpConnection>, CaptureError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedProcessCaptureConfig {
    pub ownership_refresh_micros: u64,
    pub pending_ttl_micros: u64,
    pub max_pending_frames: usize,
    pub max_pending_bytes: usize,
}

impl Default for OwnedProcessCaptureConfig {
    fn default() -> Self {
        Self {
            ownership_refresh_micros: 20_000,
            pending_ttl_micros: 250_000,
            max_pending_frames: 8_192,
            max_pending_bytes: 16 * 1024 * 1024,
        }
    }
}

impl OwnedProcessCaptureConfig {
    pub fn validate(self) -> Result<Self, OwnedProcessCaptureConfigError> {
        if self.ownership_refresh_micros == 0 {
            return Err(OwnedProcessCaptureConfigError::ZeroOwnershipRefresh);
        }
        if self.pending_ttl_micros < self.ownership_refresh_micros {
            return Err(OwnedProcessCaptureConfigError::PendingTtlBelowRefresh);
        }
        if self.max_pending_frames == 0 {
            return Err(OwnedProcessCaptureConfigError::ZeroPendingFrames);
        }
        if self.max_pending_bytes == 0 {
            return Err(OwnedProcessCaptureConfigError::ZeroPendingBytes);
        }
        Ok(self)
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum OwnedProcessCaptureConfigError {
    #[error("ownership refresh interval must be greater than zero")]
    ZeroOwnershipRefresh,

    #[error("pending-frame TTL must be at least the ownership refresh interval")]
    PendingTtlBelowRefresh,

    #[error("pending frame limit must be greater than zero")]
    ZeroPendingFrames,

    #[error("pending byte limit must be greater than zero")]
    ZeroPendingBytes,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedProcessCaptureMetrics {
    pub ingress_frames: u64,
    pub ingress_bytes: u64,
    pub emitted_frames: u64,
    pub emitted_bytes: u64,
    pub non_tcp_frames_discarded: u64,
    pub unattributed_frames_discarded: u64,
    pub pending_limit_evictions: u64,
    pub ownership_refreshes: u64,
    pub peak_pending_frames: usize,
    pub peak_pending_bytes: usize,
}

#[derive(Debug)]
struct QueuedFrame {
    frame: CapturedFrame,
    flow: DirectedTcpFlow,
    owned: bool,
}

impl QueuedFrame {
    fn expires_at(&self, ttl_micros: u64) -> u64 {
        self.frame.observed_micros.saturating_add(ttl_micros)
    }
}

/// Filters a broad in-memory frame source against a continuously refreshed
/// process-owned socket table.
///
/// Frames leave this boundary only after their exact bidirectional four-tuple
/// has been attributed to the target process. Unknown frames stay in a bounded
/// queue briefly to cover the race between the first SYN and socket-table
/// visibility. Expired or evicted frames are discarded before persistence,
/// TCP reconstruction, or protocol decoding.
#[derive(Debug)]
pub struct OwnedProcessCapture<S, O> {
    source: S,
    owner: O,
    config: OwnedProcessCaptureConfig,
    metadata: CaptureSourceMetadata,
    owned_connections: BTreeMap<ConnectionKey, TcpConnection>,
    emitted_connections: BTreeSet<ConnectionKey>,
    queue: VecDeque<QueuedFrame>,
    ready: VecDeque<CapturedFrame>,
    pending_bytes: usize,
    next_sequence: u64,
    last_refresh_micros: u64,
    last_observed_micros: u64,
    metrics: OwnedProcessCaptureMetrics,
    source_finished: bool,
}

impl<S: CaptureSource, O: ProcessSocketOwner> OwnedProcessCapture<S, O> {
    pub fn new(
        source: S,
        owner: O,
        config: OwnedProcessCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let config = config.validate().map_err(|error| CaptureError::Adapter {
            adapter: "process-owned-filter".into(),
            message: error.to_string(),
        })?;
        let source_metadata = source.metadata();
        let metadata = CaptureSourceMetadata {
            source_id: "process-owned-live".into(),
            display_name: "Process-owned live capture".into(),
            kind: CaptureSourceKind::Live,
            link_types: source_metadata.link_types.clone(),
            file_format: source_metadata.file_format,
        };
        let mut capture = Self {
            source,
            owner,
            config,
            metadata,
            owned_connections: BTreeMap::new(),
            emitted_connections: BTreeSet::new(),
            queue: VecDeque::new(),
            ready: VecDeque::new(),
            pending_bytes: 0,
            next_sequence: 1,
            last_refresh_micros: 0,
            last_observed_micros: 0,
            metrics: OwnedProcessCaptureMetrics::default(),
            source_finished: false,
        };
        capture.refresh_ownership()?;
        Ok(capture)
    }

    pub fn metrics(&self) -> &OwnedProcessCaptureMetrics {
        &self.metrics
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        self.emitted_connections
            .iter()
            .filter_map(|key| self.owned_connections.get(key).copied())
            .collect()
    }

    #[cfg(windows)]
    pub(crate) fn source(&self) -> &S {
        &self.source
    }

    fn refresh_ownership(&mut self) -> Result<(), CaptureError> {
        for connection in self.owner.snapshot()? {
            if connection.client != connection.server {
                self.owned_connections
                    .entry(connection.key())
                    .or_insert(connection);
            }
        }
        self.metrics.ownership_refreshes = self.metrics.ownership_refreshes.saturating_add(1);
        for queued in &mut self.queue {
            queued.owned = self.owned_connections.contains_key(&queued.flow.key());
        }
        Ok(())
    }

    fn refresh_due(&self, observed_micros: u64) -> bool {
        observed_micros.saturating_sub(self.last_refresh_micros)
            >= self.config.ownership_refresh_micros
    }

    fn enqueue(&mut self, frame: CapturedFrame, flow: DirectedTcpFlow) {
        let bytes = frame.bytes.len();
        let owned = self.owned_connections.contains_key(&flow.key());
        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
        self.queue.push_back(QueuedFrame { frame, flow, owned });
        self.metrics.peak_pending_frames = self.metrics.peak_pending_frames.max(self.queue.len());
        self.metrics.peak_pending_bytes = self.metrics.peak_pending_bytes.max(self.pending_bytes);
    }

    fn drain_resolved_front(&mut self, observed_micros: u64, source_finished: bool) {
        while let Some(front) = self.queue.front() {
            if front.owned {
                let mut queued = self.queue.pop_front().expect("front exists");
                self.pending_bytes = self.pending_bytes.saturating_sub(queued.frame.bytes.len());
                self.emitted_connections.insert(queued.flow.key());
                queued.frame.sequence = self.next_sequence;
                self.next_sequence = self.next_sequence.saturating_add(1);
                self.metrics.emitted_frames = self.metrics.emitted_frames.saturating_add(1);
                self.metrics.emitted_bytes = self
                    .metrics
                    .emitted_bytes
                    .saturating_add(queued.frame.bytes.len() as u64);
                self.ready.push_back(queued.frame);
            } else if source_finished
                || front.expires_at(self.config.pending_ttl_micros) <= observed_micros
            {
                let queued = self.queue.pop_front().expect("front exists");
                self.pending_bytes = self.pending_bytes.saturating_sub(queued.frame.bytes.len());
                self.metrics.unattributed_frames_discarded =
                    self.metrics.unattributed_frames_discarded.saturating_add(1);
            } else {
                break;
            }
        }
    }

    fn enforce_pending_limits(&mut self) {
        while self.queue.len() > self.config.max_pending_frames
            || self.pending_bytes > self.config.max_pending_bytes
        {
            let Some(queued) = self.queue.pop_front() else {
                break;
            };
            self.pending_bytes = self.pending_bytes.saturating_sub(queued.frame.bytes.len());
            if queued.owned {
                self.emitted_connections.insert(queued.flow.key());
                let mut frame = queued.frame;
                frame.sequence = self.next_sequence;
                self.next_sequence = self.next_sequence.saturating_add(1);
                self.metrics.emitted_frames = self.metrics.emitted_frames.saturating_add(1);
                self.metrics.emitted_bytes = self
                    .metrics
                    .emitted_bytes
                    .saturating_add(frame.bytes.len() as u64);
                self.ready.push_back(frame);
            } else {
                self.metrics.unattributed_frames_discarded =
                    self.metrics.unattributed_frames_discarded.saturating_add(1);
                self.metrics.pending_limit_evictions =
                    self.metrics.pending_limit_evictions.saturating_add(1);
            }
        }
    }

    fn ingest(&mut self, frame: CapturedFrame) -> Result<(), CaptureError> {
        self.last_observed_micros = frame.observed_micros;
        self.metrics.ingress_frames = self.metrics.ingress_frames.saturating_add(1);
        self.metrics.ingress_bytes = self
            .metrics
            .ingress_bytes
            .saturating_add(frame.bytes.len() as u64);
        if !self.metadata.link_types.contains(&frame.link_type) {
            self.metadata.link_types.push(frame.link_type);
        }

        let Some(flow) = extract_tcp_flow(&frame) else {
            self.metrics.non_tcp_frames_discarded =
                self.metrics.non_tcp_frames_discarded.saturating_add(1);
            return Ok(());
        };
        self.enqueue(frame, flow);

        if self.refresh_due(self.last_observed_micros)
            && self.queue.iter().any(|queued| !queued.owned)
        {
            self.refresh_ownership()?;
            self.last_refresh_micros = self.last_observed_micros;
        }
        self.drain_resolved_front(self.last_observed_micros, false);
        self.enforce_pending_limits();
        self.drain_resolved_front(self.last_observed_micros, false);
        Ok(())
    }

    fn finish_source(&mut self) -> Result<(), CaptureError> {
        if self.source_finished {
            return Ok(());
        }
        self.refresh_ownership()?;
        self.drain_resolved_front(self.last_observed_micros, true);
        self.source_finished = true;
        Ok(())
    }
}

impl<S: CaptureSource, O: ProcessSocketOwner> CaptureSource for OwnedProcessCapture<S, O> {
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
                Some(frame) => self.ingest(frame)?,
                None => self.finish_source()?,
            }
        }
    }
}

fn extract_tcp_flow(frame: &CapturedFrame) -> Option<DirectedTcpFlow> {
    let bytes = frame.bytes.as_ref();
    let parsed = match frame.link_type {
        CaptureLinkType::Ethernet => SlicedPacket::from_ethernet(bytes).ok()?,
        CaptureLinkType::LinuxCookedV1 => SlicedPacket::from_linux_sll(bytes).ok()?,
        CaptureLinkType::RawIp | CaptureLinkType::RawIpv4 | CaptureLinkType::RawIpv6 => {
            SlicedPacket::from_ip(bytes).ok()?
        }
        CaptureLinkType::NullLoopback => {
            SlicedPacket::from_ip(bytes.get(NULL_LOOPBACK_HEADER_LEN..)?).ok()?
        }
        CaptureLinkType::LinuxCookedV2 => {
            let header = bytes.get(..LINUX_SLL2_HEADER_LEN)?;
            let payload = bytes.get(LINUX_SLL2_HEADER_LEN..)?;
            let protocol = EtherType(u16::from_be_bytes([header[0], header[1]]));
            SlicedPacket::from_ether_type(protocol, payload).ok()?
        }
        CaptureLinkType::Unknown(_) => return None,
    };
    let (source_address, destination_address) = match parsed.net.as_ref()? {
        NetSlice::Ipv4(ip) if !ip.is_payload_fragmented() => (
            IpAddr::V4(ip.header().source_addr()),
            IpAddr::V4(ip.header().destination_addr()),
        ),
        NetSlice::Ipv6(ip) if !ip.is_payload_fragmented() => (
            IpAddr::V6(ip.header().source_addr()),
            IpAddr::V6(ip.header().destination_addr()),
        ),
        _ => return None,
    };
    let TransportSlice::Tcp(tcp) = parsed.transport? else {
        return None;
    };
    Some(DirectedTcpFlow {
        source: TcpEndpoint::new(source_address, tcp.source_port()),
        destination: TcpEndpoint::new(destination_address, tcp.destination_port()),
    })
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
    use crate::{TimestampNormalization, ValidatedCapture};

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

    struct FixtureOwner {
        snapshots: VecDeque<Vec<TcpConnection>>,
        current: Vec<TcpConnection>,
    }

    impl ProcessSocketOwner for FixtureOwner {
        fn snapshot(&mut self) -> Result<Vec<TcpConnection>, CaptureError> {
            if let Some(snapshot) = self.snapshots.pop_front() {
                self.current = snapshot;
            }
            Ok(self.current.clone())
        }
    }

    fn endpoint(last: u8, port: u16) -> TcpEndpoint {
        TcpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    fn connection() -> TcpConnection {
        TcpConnection::new(endpoint(1, 31_000), endpoint(2, 32_000))
    }

    fn frame(
        sequence: u64,
        observed_micros: u64,
        source: TcpEndpoint,
        destination: TcpEndpoint,
    ) -> CapturedFrame {
        let (source_address, destination_address) = match (source.address, destination.address) {
            (IpAddr::V4(source_address), IpAddr::V4(destination_address)) => {
                (source_address, destination_address)
            }
            _ => unreachable!(),
        };
        let builder = PacketBuilder::ethernet2([1; 6], [2; 6])
            .ipv4(source_address.octets(), destination_address.octets(), 64)
            .tcp(source.port, destination.port, 1, 1_024);
        let mut bytes = Vec::with_capacity(builder.size(1));
        builder.write(&mut bytes, &[42]).unwrap();
        CapturedFrame {
            sequence,
            observed_micros,
            source_timestamp_nanos: Some(1_000_000_000 + observed_micros as i64 * 1_000),
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

    fn owner(snapshots: Vec<Vec<TcpConnection>>) -> FixtureOwner {
        FixtureOwner {
            snapshots: snapshots.into(),
            current: Vec::new(),
        }
    }

    #[test]
    fn exact_owned_flow_is_emitted_and_reverse_direction_matches() {
        let connection = connection();
        let frames = vec![
            frame(9, 0, connection.client, connection.server),
            frame(10, 1, connection.server, connection.client),
        ];
        let filtered = OwnedProcessCapture::new(
            source(frames),
            owner(vec![vec![connection]]),
            OwnedProcessCaptureConfig::default(),
        )
        .unwrap();
        let mut validated = ValidatedCapture::new(filtered);

        assert_eq!(validated.next_frame().unwrap().unwrap().sequence, 1);
        assert_eq!(validated.next_frame().unwrap().unwrap().sequence, 2);
        assert!(validated.next_frame().unwrap().is_none());
    }

    #[test]
    fn first_syn_waits_for_socket_table_visibility_without_reordering() {
        let connection = connection();
        let unrelated = TcpConnection::new(endpoint(3, 33_000), endpoint(4, 44_000));
        let frames = vec![
            frame(1, 0, connection.client, connection.server),
            frame(2, 10_000, connection.server, connection.client),
            frame(3, 20_000, unrelated.client, unrelated.server),
        ];
        let mut filtered = OwnedProcessCapture::new(
            source(frames),
            owner(vec![Vec::new(), vec![connection], vec![connection]]),
            OwnedProcessCaptureConfig::default(),
        )
        .unwrap();

        let first = filtered.next_frame().unwrap().unwrap();
        let second = filtered.next_frame().unwrap().unwrap();
        assert_eq!(first.observed_micros, 0);
        assert_eq!(second.observed_micros, 10_000);
        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(filtered.metrics().unattributed_frames_discarded, 1);
        assert_eq!(filtered.confirmed_connections(), vec![connection]);
    }

    #[test]
    fn unrelated_frames_expire_and_never_leave_capture_boundary() {
        let unrelated = TcpConnection::new(endpoint(3, 33_000), endpoint(4, 44_000));
        let frames = vec![
            frame(1, 0, unrelated.client, unrelated.server),
            frame(2, 100_000, unrelated.server, unrelated.client),
        ];
        let mut filtered = OwnedProcessCapture::new(
            source(frames),
            owner(vec![Vec::new()]),
            OwnedProcessCaptureConfig::default(),
        )
        .unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(filtered.metrics().emitted_frames, 0);
        assert_eq!(filtered.metrics().unattributed_frames_discarded, 2);
        assert!(filtered.confirmed_connections().is_empty());
    }

    #[test]
    fn pending_memory_limit_evicts_unknown_front_frame() {
        let unrelated = TcpConnection::new(endpoint(3, 33_000), endpoint(4, 44_000));
        let frames = vec![
            frame(1, 0, unrelated.client, unrelated.server),
            frame(2, 1, unrelated.server, unrelated.client),
        ];
        let config = OwnedProcessCaptureConfig {
            ownership_refresh_micros: 10,
            pending_ttl_micros: 100,
            max_pending_frames: 1,
            max_pending_bytes: 1024 * 1024,
        };
        let mut filtered =
            OwnedProcessCapture::new(source(frames), owner(vec![Vec::new()]), config).unwrap();

        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(filtered.metrics().pending_limit_evictions, 1);
        assert_eq!(filtered.metrics().unattributed_frames_discarded, 2);
    }
}
