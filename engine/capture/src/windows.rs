use std::{
    collections::BTreeMap,
    ffi::{CStr, c_void},
    mem::{size_of, size_of_val},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ptr, slice,
};

use windows_sys::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_INSUFFICIENT_BUFFER, NO_ERROR},
    NetworkManagement::{
        IpHelper::{
            GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
            GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, GetBestInterfaceEx, GetExtendedTcpTable,
            IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH, MIB_TCP6ROW_OWNER_PID,
            MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
            TCP_TABLE_OWNER_PID_ALL,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, IN_ADDR, IN_ADDR_0, IN_ADDR_0_0, IN6_ADDR, IN6_ADDR_0,
        SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
    },
};

use crate::dumpcap::DumpcapLiveCapture;
use crate::npcap::NpcapLiveCapture;
use crate::{
    CaptureError, CaptureSource, CaptureSourceMetadata, CapturedFrame, DumpcapLiveConfig,
    LiveCaptureStopHandle, NpcapLiveConfig, NpcapLiveStopHandle, OwnedProcessCapture,
    OwnedProcessCaptureConfig, OwnedProcessCaptureMetrics, ProcessSocketOwner,
    SignatureFlowCapture, SignatureFlowCaptureConfig, SignatureFlowCaptureMetrics, TcpConnection,
    TcpEndpoint, TcpPayloadPrefixSignature, TcpPayloadSignature,
};

const MAX_TABLE_QUERY_ATTEMPTS: usize = 4;
const MAX_ADAPTER_QUERY_ATTEMPTS: usize = 4;
const MAX_ADAPTERS: usize = 512;
const MAX_UNICAST_ADDRESSES_PER_ADAPTER: usize = 512;
pub const MAX_WINDOWS_CAPTURE_CANDIDATES: usize = 4;
pub const NPCAP_LOOPBACK_ADAPTER_NAME: &str = r"\Device\NPF_Loopback";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCaptureAdapter {
    /// Windows adapter identifier used inside Npcap names such as
    /// `\Device\NPF_{GUID}`.
    pub adapter_name: String,
    pub friendly_name: String,
    pub description: String,
    pub interface_index: u32,
    pub ipv6_interface_index: u32,
    pub interface_type: u32,
    pub physical_address: Vec<u8>,
    pub operational: bool,
    pub has_gateway: bool,
    pub ipv4_metric: u32,
    pub unicast_addresses: Vec<IpAddr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsCaptureAdapterRecommendationSource {
    GameTraffic,
    SystemRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCaptureAdapterRecommendation {
    pub adapter_name: String,
    pub source: WindowsCaptureAdapterRecommendationSource,
    pub matched_game_connections: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowsCaptureCandidateSource {
    ExplicitPrimary,
    GameSocketLocalAddress,
    GameSocketRoute,
    LoopbackProbation,
    SystemRoute,
}

/// One bounded, privacy-safe adapter candidate for a future multi-adapter
/// capture. Reasons contain no socket endpoints or packet-derived data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCaptureCandidate {
    pub adapter_name: String,
    pub sources: Vec<WindowsCaptureCandidateSource>,
    pub matched_game_connections: usize,
}

trait WindowsRouteResolver {
    fn interface_index(&self, destination: IpAddr) -> Option<u32>;
}

#[derive(Debug, Clone, Copy)]
struct SystemWindowsRouteResolver;

/// Enumerates Windows adapters using the native IP Helper API.
///
/// This metadata is matched to Npcap/dumpcap interface identifiers by GUID, so
/// callers do not need to guess from dumpcap's numeric ordering.
pub fn windows_capture_adapters() -> Result<Vec<WindowsCaptureAdapter>, CaptureError> {
    let mut required_bytes = 0_u32;
    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    // SAFETY: a null output buffer is the documented sizing call.
    let status = unsafe {
        GetAdaptersAddresses(
            u32::from(AF_UNSPEC),
            flags,
            ptr::null(),
            ptr::null_mut(),
            &mut required_bytes,
        )
    };
    if status != ERROR_BUFFER_OVERFLOW && status != NO_ERROR {
        return Err(adapter_table_status(status));
    }
    if required_bytes == 0 {
        return Err(adapter_table_error(
            "Windows returned an empty adapter table",
        ));
    }

    for _ in 0..MAX_ADAPTER_QUERY_ATTEMPTS {
        let word_bytes = size_of::<usize>();
        let word_count = (required_bytes as usize)
            .checked_add(word_bytes - 1)
            .ok_or_else(|| adapter_table_error("adapter table size overflowed"))?
            / word_bytes;
        let mut buffer = vec![0_usize; word_count];
        let mut supplied_bytes = u32::try_from(size_of_val(buffer.as_slice()))
            .map_err(|_| adapter_table_error("adapter table is too large"))?;
        // SAFETY: the buffer is writable, pointer-aligned, and remains alive
        // while Windows populates its linked adapter records.
        let status = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC),
                flags,
                ptr::null(),
                buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>(),
                &mut supplied_bytes,
            )
        };
        if status == NO_ERROR {
            return parse_adapter_table(&buffer);
        }
        if status != ERROR_BUFFER_OVERFLOW {
            return Err(adapter_table_status(status));
        }
        required_bytes = supplied_bytes;
    }

    Err(adapter_table_error(
        "Windows adapter table kept changing during bounded retries",
    ))
}

/// Chooses the Npcap adapter backed by the local address of a game's active
/// TCP connections. If no game socket is currently available, the active
/// routed Windows adapter is returned as a clearly weaker fallback.
pub fn recommend_windows_capture_adapter(
    adapters: &[WindowsCaptureAdapter],
    process_ids: &[u32],
) -> Option<WindowsCaptureAdapterRecommendation> {
    let candidates = recommend_windows_capture_candidates(adapters, process_ids, None);
    compatibility_capture_recommendation(adapters, &candidates)
}

fn compatibility_capture_recommendation(
    adapters: &[WindowsCaptureAdapter],
    candidates: &[WindowsCaptureCandidate],
) -> Option<WindowsCaptureAdapterRecommendation> {
    if let Some(candidate) = candidates.iter().find(|candidate| {
        candidate
            .sources
            .contains(&WindowsCaptureCandidateSource::GameSocketLocalAddress)
    }) {
        return Some(WindowsCaptureAdapterRecommendation {
            adapter_name: candidate.adapter_name.clone(),
            source: WindowsCaptureAdapterRecommendationSource::GameTraffic,
            matched_game_connections: candidate.matched_game_connections,
        });
    }
    // Keep the legacy API's weaker fallback independent of the candidate cap:
    // several route-derived probes must not make the single recommendation
    // disappear when a routed adapter is still available.
    adapters
        .iter()
        .filter(|adapter| adapter.operational && adapter.has_gateway)
        .min_by_key(|adapter| {
            (
                adapter.ipv4_metric,
                adapter.interface_index,
                adapter.adapter_name.as_str(),
            )
        })
        .map(|adapter| WindowsCaptureAdapterRecommendation {
            adapter_name: adapter.adapter_name.clone(),
            source: WindowsCaptureAdapterRecommendationSource::SystemRoute,
            matched_game_connections: 0,
        })
}

/// Plans at most four adapter candidates without opening capture handles or
/// mutating the caller's selected interface. This is intentionally only a
/// discovery result; protocol ownership must still be proven by the signature
/// boundary before any frame is exposed.
pub fn recommend_windows_capture_candidates(
    adapters: &[WindowsCaptureAdapter],
    process_ids: &[u32],
    explicit_primary: Option<&str>,
) -> Vec<WindowsCaptureCandidate> {
    let mut connections = Vec::new();
    for process_id in process_ids.iter().copied().filter(|value| *value != 0) {
        let Ok(owner) = WindowsProcessSocketOwner::new(process_id) else {
            continue;
        };
        let Ok(mut snapshot) = owner.snapshot_adapter_candidates() else {
            continue;
        };
        connections.append(&mut snapshot);
    }
    connections.sort_unstable();
    connections.dedup();
    plan_windows_capture_candidates(
        adapters,
        &connections,
        explicit_primary,
        &SystemWindowsRouteResolver,
    )
}

fn plan_windows_capture_candidates<R: WindowsRouteResolver>(
    adapters: &[WindowsCaptureAdapter],
    connections: &[TcpConnection],
    explicit_primary: Option<&str>,
    routes: &R,
) -> Vec<WindowsCaptureCandidate> {
    let mut candidates = Vec::new();
    if let Some(primary) = explicit_primary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let canonical = adapters
            .iter()
            .find(|adapter| same_adapter_name(&adapter.adapter_name, primary))
            .map_or(primary, |adapter| adapter.adapter_name.as_str());
        add_capture_candidate(
            &mut candidates,
            canonical,
            WindowsCaptureCandidateSource::ExplicitPrimary,
            0,
        );
    }

    let mut address_counts = BTreeMap::<IpAddr, usize>::new();
    for connection in connections {
        *address_counts.entry(connection.client.address).or_default() += 1;
    }
    let mut directly_matched = adapters
        .iter()
        .filter(|adapter| adapter.interface_type != IF_TYPE_SOFTWARE_LOOPBACK)
        .filter_map(|adapter| {
            let count = adapter
                .unicast_addresses
                .iter()
                .filter(|address| !address.is_loopback())
                .map(|address| address_counts.get(address).copied().unwrap_or_default())
                .sum::<usize>();
            (count > 0).then_some((adapter, count))
        })
        .collect::<Vec<_>>();
    directly_matched.sort_by(|(left, left_count), (right, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| right.operational.cmp(&left.operational))
            .then_with(|| right.has_gateway.cmp(&left.has_gateway))
            .then_with(|| left.ipv4_metric.cmp(&right.ipv4_metric))
            .then_with(|| left.interface_index.cmp(&right.interface_index))
            .then_with(|| left.adapter_name.cmp(&right.adapter_name))
    });
    for (adapter, count) in &directly_matched {
        add_capture_candidate(
            &mut candidates,
            &adapter.adapter_name,
            WindowsCaptureCandidateSource::GameSocketLocalAddress,
            *count,
        );
    }

    let mut destinations = connections
        .iter()
        .map(|connection| connection.server.address)
        .filter(|address| !address.is_unspecified() && !address.is_loopback())
        .collect::<Vec<_>>();
    destinations.sort_unstable();
    destinations.dedup();
    for destination in destinations {
        let Some(interface_index) = routes.interface_index(destination) else {
            continue;
        };
        let Some(adapter) = adapters.iter().find(|adapter| match destination {
            IpAddr::V4(_) => adapter.interface_index == interface_index,
            IpAddr::V6(_) => adapter.ipv6_interface_index == interface_index,
        }) else {
            continue;
        };
        add_capture_candidate(
            &mut candidates,
            &adapter.adapter_name,
            WindowsCaptureCandidateSource::GameSocketRoute,
            0,
        );
    }

    if connections.iter().any(|connection| {
        connection.client.address.is_loopback() || connection.server.address.is_loopback()
    }) {
        add_capture_candidate(
            &mut candidates,
            NPCAP_LOOPBACK_ADAPTER_NAME,
            WindowsCaptureCandidateSource::LoopbackProbation,
            0,
        );
    }

    if let Some(adapter) = adapters
        .iter()
        .filter(|adapter| adapter.operational && adapter.has_gateway)
        .min_by_key(|adapter| {
            (
                adapter.ipv4_metric,
                adapter.interface_index,
                adapter.adapter_name.as_str(),
            )
        })
    {
        add_capture_candidate(
            &mut candidates,
            &adapter.adapter_name,
            WindowsCaptureCandidateSource::SystemRoute,
            0,
        );
    }

    for candidate in &mut candidates {
        candidate.sources.sort_unstable();
        candidate.sources.dedup();
    }
    candidates.truncate(MAX_WINDOWS_CAPTURE_CANDIDATES);
    candidates
}

fn add_capture_candidate(
    candidates: &mut Vec<WindowsCaptureCandidate>,
    adapter_name: &str,
    source: WindowsCaptureCandidateSource,
    matched_game_connections: usize,
) {
    if let Some(candidate) = candidates
        .iter_mut()
        .find(|candidate| same_adapter_name(&candidate.adapter_name, adapter_name))
    {
        candidate.sources.push(source);
        candidate.matched_game_connections = candidate
            .matched_game_connections
            .saturating_add(matched_game_connections);
        return;
    }
    candidates.push(WindowsCaptureCandidate {
        adapter_name: adapter_name.to_owned(),
        sources: vec![source],
        matched_game_connections,
    });
}

fn same_adapter_name(left: &str, right: &str) -> bool {
    normalized_adapter_name(left) == normalized_adapter_name(right)
}

fn normalized_adapter_name(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    normalized
        .strip_prefix(r"\device\npf_")
        .unwrap_or(&normalized)
        .trim_matches(|character| character == '{' || character == '}')
        .to_owned()
}

#[derive(Debug, Clone)]
pub struct WindowsProcessSocketOwner {
    process_id: u32,
}

impl WindowsProcessSocketOwner {
    pub fn new(process_id: u32) -> Result<Self, CaptureError> {
        if process_id == 0 {
            return Err(socket_table_error("process ID must be greater than zero"));
        }
        Ok(Self { process_id })
    }

    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    fn snapshot_ipv4(&self) -> Result<Vec<TcpConnection>, CaptureError> {
        self.snapshot_ipv4_with_loopback(false)
    }

    fn snapshot_ipv4_with_loopback(
        &self,
        include_loopback: bool,
    ) -> Result<Vec<TcpConnection>, CaptureError> {
        let buffer = query_tcp_table(u32::from(AF_INET))?;
        // SAFETY: `query_tcp_table` returns an aligned buffer initialized by
        // `GetExtendedTcpTable` for AF_INET and TCP_TABLE_OWNER_PID_ALL.
        let table = unsafe { &*buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>() };
        let count = table.dwNumEntries as usize;
        let first = ptr::addr_of!(table.table).cast::<MIB_TCPROW_OWNER_PID>();
        checked_rows_fit(
            &buffer,
            first.cast(),
            count,
            size_of::<MIB_TCPROW_OWNER_PID>(),
        )?;

        let mut connections = Vec::new();
        for index in 0..count {
            // SAFETY: bounds were checked against the returned buffer above.
            let row = unsafe { ptr::read_unaligned(first.add(index)) };
            if row.dwOwningPid != self.process_id {
                continue;
            }
            let connection = TcpConnection::new(
                TcpEndpoint::new(
                    IpAddr::V4(Ipv4Addr::from(u32::from_be(row.dwLocalAddr))),
                    network_port(row.dwLocalPort),
                ),
                TcpEndpoint::new(
                    IpAddr::V4(Ipv4Addr::from(u32::from_be(row.dwRemoteAddr))),
                    network_port(row.dwRemotePort),
                ),
            );
            if usable_candidate_connection(connection)
                && (include_loopback || usable_remote_connection(connection))
            {
                connections.push(connection);
            }
        }
        Ok(connections)
    }

    fn snapshot_ipv6(&self) -> Result<Vec<TcpConnection>, CaptureError> {
        self.snapshot_ipv6_with_loopback(false)
    }

    fn snapshot_ipv6_with_loopback(
        &self,
        include_loopback: bool,
    ) -> Result<Vec<TcpConnection>, CaptureError> {
        let buffer = query_tcp_table(u32::from(AF_INET6))?;
        // SAFETY: `query_tcp_table` returns an aligned buffer initialized by
        // `GetExtendedTcpTable` for AF_INET6 and TCP_TABLE_OWNER_PID_ALL.
        let table = unsafe { &*buffer.as_ptr().cast::<MIB_TCP6TABLE_OWNER_PID>() };
        let count = table.dwNumEntries as usize;
        let first = ptr::addr_of!(table.table).cast::<MIB_TCP6ROW_OWNER_PID>();
        checked_rows_fit(
            &buffer,
            first.cast(),
            count,
            size_of::<MIB_TCP6ROW_OWNER_PID>(),
        )?;

        let mut connections = Vec::new();
        for index in 0..count {
            // SAFETY: bounds were checked against the returned buffer above.
            let row = unsafe { ptr::read_unaligned(first.add(index)) };
            if row.dwOwningPid != self.process_id {
                continue;
            }
            let connection = TcpConnection::new(
                TcpEndpoint::new(
                    IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                    network_port(row.dwLocalPort),
                ),
                TcpEndpoint::new(
                    IpAddr::V6(Ipv6Addr::from(row.ucRemoteAddr)),
                    network_port(row.dwRemotePort),
                ),
            );
            if usable_candidate_connection(connection)
                && (include_loopback || usable_remote_connection(connection))
            {
                connections.push(connection);
            }
        }
        Ok(connections)
    }

    fn snapshot_adapter_candidates(&self) -> Result<Vec<TcpConnection>, CaptureError> {
        let mut connections = self.snapshot_ipv4_with_loopback(true)?;
        connections.extend(self.snapshot_ipv6_with_loopback(true)?);
        connections.sort_unstable();
        connections.dedup();
        Ok(connections)
    }
}

impl ProcessSocketOwner for WindowsProcessSocketOwner {
    fn snapshot(&mut self) -> Result<Vec<TcpConnection>, CaptureError> {
        let mut connections = self.snapshot_ipv4()?;
        connections.extend(self.snapshot_ipv6()?);
        connections.sort_unstable();
        connections.dedup();
        Ok(connections)
    }
}

/// Safe Windows live-capture entry point.
///
/// The broad dumpcap pipe is private to this wrapper, so callers cannot obtain
/// a frame until the exact TCP connection is attributed to `process_id`.
#[derive(Debug)]
pub struct WindowsOwnedDumpcapCapture {
    inner: OwnedProcessCapture<DumpcapLiveCapture, WindowsProcessSocketOwner>,
}

impl WindowsOwnedDumpcapCapture {
    pub fn spawn(
        process_id: u32,
        dumpcap: DumpcapLiveConfig,
        filter: OwnedProcessCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let source = DumpcapLiveCapture::spawn(dumpcap)?;
        let owner = WindowsProcessSocketOwner::new(process_id)?;
        Ok(Self {
            inner: OwnedProcessCapture::new(source, owner, filter)?,
        })
    }

    pub fn metrics(&self) -> &OwnedProcessCaptureMetrics {
        self.inner.metrics()
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        self.inner.confirmed_connections()
    }

    pub fn stop_handle(&self) -> LiveCaptureStopHandle {
        self.inner.source().stop_handle()
    }
}

impl CaptureSource for WindowsOwnedDumpcapCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        self.inner.metadata()
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        self.inner.next_frame()
    }
}

/// Native Npcap ingress with the same exact process-ownership privacy gate as
/// the dumpcap compatibility adapter.
#[derive(Debug)]
pub struct WindowsOwnedNpcapCapture {
    inner: OwnedProcessCapture<NpcapLiveCapture, WindowsProcessSocketOwner>,
}

impl WindowsOwnedNpcapCapture {
    pub fn open(
        process_id: u32,
        npcap: NpcapLiveConfig,
        filter: OwnedProcessCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let source = NpcapLiveCapture::open(npcap)?;
        let owner = WindowsProcessSocketOwner::new(process_id)?;
        Ok(Self {
            inner: OwnedProcessCapture::new(source, owner, filter)?,
        })
    }

    pub fn metrics(&self) -> &OwnedProcessCaptureMetrics {
        self.inner.metrics()
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        self.inner.confirmed_connections()
    }

    pub fn stop_handle(&self) -> NpcapLiveStopHandle {
        self.inner.source().stop_handle()
    }
}

impl CaptureSource for WindowsOwnedNpcapCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        self.inner.metadata()
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        self.inner.next_frame()
    }
}

#[derive(Debug, Clone)]
pub enum WindowsLiveCaptureStopHandle {
    Npcap(NpcapLiveStopHandle),
    Dumpcap(LiveCaptureStopHandle),
}

impl WindowsLiveCaptureStopHandle {
    pub fn request_stop(&self) -> Result<(), CaptureError> {
        match self {
            Self::Npcap(handle) => {
                handle.request_stop();
                Ok(())
            }
            Self::Dumpcap(handle) => handle.request_stop(),
        }
    }
}

/// Prefers direct native Npcap capture and retains dumpcap only as a
/// compatibility fallback for machines where the native API cannot open.
#[derive(Debug)]
pub enum WindowsOwnedLiveCapture {
    Npcap(WindowsOwnedNpcapCapture),
    Dumpcap(WindowsOwnedDumpcapCapture),
}

impl WindowsOwnedLiveCapture {
    pub fn open(
        process_id: u32,
        interface: &str,
        duration_seconds: u32,
        dumpcap_fallback: Option<DumpcapLiveConfig>,
        filter: OwnedProcessCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let npcap_result = NpcapLiveConfig::new(interface, duration_seconds)
            .and_then(|config| WindowsOwnedNpcapCapture::open(process_id, config, filter));
        match npcap_result {
            Ok(capture) => Ok(Self::Npcap(capture)),
            Err(npcap_error) => match dumpcap_fallback {
                Some(config) => WindowsOwnedDumpcapCapture::spawn(process_id, config, filter)
                    .map(Self::Dumpcap)
                    .map_err(|dumpcap_error| CaptureError::Adapter {
                        adapter: "windows-live-capture".into(),
                        message: format!(
                            "native Npcap failed ({npcap_error}); dumpcap fallback also failed ({dumpcap_error})"
                        ),
                    }),
                None => Err(npcap_error),
            },
        }
    }

    pub fn metrics(&self) -> &OwnedProcessCaptureMetrics {
        match self {
            Self::Npcap(capture) => capture.metrics(),
            Self::Dumpcap(capture) => capture.metrics(),
        }
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        match self {
            Self::Npcap(capture) => capture.confirmed_connections(),
            Self::Dumpcap(capture) => capture.confirmed_connections(),
        }
    }

    pub fn stop_handle(&self) -> WindowsLiveCaptureStopHandle {
        match self {
            Self::Npcap(capture) => WindowsLiveCaptureStopHandle::Npcap(capture.stop_handle()),
            Self::Dumpcap(capture) => WindowsLiveCaptureStopHandle::Dumpcap(capture.stop_handle()),
        }
    }
}

impl CaptureSource for WindowsOwnedLiveCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        match self {
            Self::Npcap(capture) => capture.metadata(),
            Self::Dumpcap(capture) => capture.metadata(),
        }
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        match self {
            Self::Npcap(capture) => capture.next_frame(),
            Self::Dumpcap(capture) => capture.next_frame(),
        }
    }
}

/// Native Npcap ingress protected by an exact game-protocol signature gate.
#[derive(Debug)]
pub struct WindowsSignatureNpcapCapture {
    inner: SignatureFlowCapture<NpcapLiveCapture>,
}

impl WindowsSignatureNpcapCapture {
    pub fn open(
        npcap: NpcapLiveConfig,
        signature: TcpPayloadSignature,
        filter: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let source = NpcapLiveCapture::open(npcap)?;
        Ok(Self {
            inner: SignatureFlowCapture::new(source, signature, filter)?,
        })
    }

    pub fn open_prefix(
        npcap: NpcapLiveConfig,
        signature: TcpPayloadPrefixSignature,
        filter: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let source = NpcapLiveCapture::open(npcap)?;
        Ok(Self {
            inner: SignatureFlowCapture::new_prefix(source, signature, filter)?,
        })
    }

    pub fn metrics(&self) -> &SignatureFlowCaptureMetrics {
        self.inner.metrics()
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        self.inner.confirmed_connections()
    }

    pub fn stop_handle(&self) -> NpcapLiveStopHandle {
        self.inner.source().stop_handle()
    }
}

impl CaptureSource for WindowsSignatureNpcapCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        self.inner.metadata()
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        self.inner.next_frame()
    }
}

/// Dumpcap compatibility ingress protected by the same protocol-signature
/// privacy boundary as native Npcap.
#[derive(Debug)]
pub struct WindowsSignatureDumpcapCapture {
    inner: SignatureFlowCapture<DumpcapLiveCapture>,
}

impl WindowsSignatureDumpcapCapture {
    pub fn spawn(
        dumpcap: DumpcapLiveConfig,
        signature: TcpPayloadSignature,
        filter: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let source = DumpcapLiveCapture::spawn(dumpcap)?;
        Ok(Self {
            inner: SignatureFlowCapture::new(source, signature, filter)?,
        })
    }

    pub fn spawn_prefix(
        dumpcap: DumpcapLiveConfig,
        signature: TcpPayloadPrefixSignature,
        filter: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let source = DumpcapLiveCapture::spawn(dumpcap)?;
        Ok(Self {
            inner: SignatureFlowCapture::new_prefix(source, signature, filter)?,
        })
    }

    pub fn metrics(&self) -> &SignatureFlowCaptureMetrics {
        self.inner.metrics()
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        self.inner.confirmed_connections()
    }

    pub fn stop_handle(&self) -> LiveCaptureStopHandle {
        self.inner.source().stop_handle()
    }
}

impl CaptureSource for WindowsSignatureDumpcapCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        self.inner.metadata()
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        self.inner.next_frame()
    }
}

/// Packet-first Windows live capture. It opens broad TCP ingress in memory,
/// then exposes only exact connections proven by the supplied game signature.
#[derive(Debug)]
pub enum WindowsSignatureLiveCapture {
    Npcap(WindowsSignatureNpcapCapture),
    Dumpcap(WindowsSignatureDumpcapCapture),
}

impl WindowsSignatureLiveCapture {
    pub fn open(
        interface: &str,
        duration_seconds: u32,
        dumpcap_fallback: Option<DumpcapLiveConfig>,
        signature: TcpPayloadSignature,
        filter: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let npcap_result = NpcapLiveConfig::new(interface, duration_seconds)
            .and_then(|config| WindowsSignatureNpcapCapture::open(config, signature, filter));
        match npcap_result {
            Ok(capture) => Ok(Self::Npcap(capture)),
            Err(npcap_error) => match dumpcap_fallback {
                Some(config) => WindowsSignatureDumpcapCapture::spawn(config, signature, filter)
                    .map(Self::Dumpcap)
                    .map_err(|dumpcap_error| CaptureError::Adapter {
                        adapter: "windows-signature-live-capture".into(),
                        message: format!(
                            "native Npcap failed ({npcap_error}); dumpcap fallback also failed ({dumpcap_error})"
                        ),
                    }),
                None => Err(npcap_error),
            },
        }
    }

    pub fn open_prefix(
        interface: &str,
        duration_seconds: u32,
        dumpcap_fallback: Option<DumpcapLiveConfig>,
        signature: TcpPayloadPrefixSignature,
        filter: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let npcap_result = NpcapLiveConfig::new(interface, duration_seconds).and_then(|config| {
            WindowsSignatureNpcapCapture::open_prefix(config, signature, filter)
        });
        match npcap_result {
            Ok(capture) => Ok(Self::Npcap(capture)),
            Err(npcap_error) => match dumpcap_fallback {
                Some(config) => WindowsSignatureDumpcapCapture::spawn_prefix(config, signature, filter)
                    .map(Self::Dumpcap)
                    .map_err(|dumpcap_error| CaptureError::Adapter {
                        adapter: "windows-signature-live-capture".into(),
                        message: format!(
                            "native Npcap failed ({npcap_error}); dumpcap fallback also failed ({dumpcap_error})"
                        ),
                    }),
                None => Err(npcap_error),
            },
        }
    }

    pub fn metrics(&self) -> &SignatureFlowCaptureMetrics {
        match self {
            Self::Npcap(capture) => capture.metrics(),
            Self::Dumpcap(capture) => capture.metrics(),
        }
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        match self {
            Self::Npcap(capture) => capture.confirmed_connections(),
            Self::Dumpcap(capture) => capture.confirmed_connections(),
        }
    }

    pub fn stop_handle(&self) -> WindowsLiveCaptureStopHandle {
        match self {
            Self::Npcap(capture) => WindowsLiveCaptureStopHandle::Npcap(capture.stop_handle()),
            Self::Dumpcap(capture) => WindowsLiveCaptureStopHandle::Dumpcap(capture.stop_handle()),
        }
    }
}

impl CaptureSource for WindowsSignatureLiveCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        match self {
            Self::Npcap(capture) => capture.metadata(),
            Self::Dumpcap(capture) => capture.metadata(),
        }
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        match self {
            Self::Npcap(capture) => capture.next_frame(),
            Self::Dumpcap(capture) => capture.next_frame(),
        }
    }
}

fn parse_adapter_table(buffer: &[usize]) -> Result<Vec<WindowsCaptureAdapter>, CaptureError> {
    let buffer_start = buffer.as_ptr() as usize;
    let buffer_end = buffer_start
        .checked_add(size_of_val(buffer))
        .ok_or_else(|| adapter_table_error("adapter table bounds overflowed"))?;
    let mut current = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    let mut adapters = Vec::new();

    while !current.is_null() {
        if adapters.len() >= MAX_ADAPTERS {
            return Err(adapter_table_error(
                "Windows adapter list exceeded the safety limit",
            ));
        }
        ensure_record_in_buffer::<IP_ADAPTER_ADDRESSES_LH>(
            current.cast(),
            buffer_start,
            buffer_end,
        )?;
        // SAFETY: the record address was checked against the live buffer and
        // Windows guarantees its string and address pointers for the duration
        // of this call.
        let adapter = unsafe { &*current };
        let mut unicast_addresses = Vec::new();
        let mut address = adapter.FirstUnicastAddress;
        while !address.is_null() {
            if unicast_addresses.len() >= MAX_UNICAST_ADDRESSES_PER_ADAPTER {
                return Err(adapter_table_error(
                    "Windows adapter address list exceeded the safety limit",
                ));
            }
            // SAFETY: nodes belong to the successful GetAdaptersAddresses
            // result and remain valid while `buffer` is alive.
            let node = unsafe { &*address };
            if let Some(ip) = socket_address_to_ip(node.Address) {
                unicast_addresses.push(ip);
            }
            address = node.Next;
        }
        unicast_addresses.sort_unstable();
        unicast_addresses.dedup();
        // SAFETY: fields point to NUL-terminated strings owned by the adapter
        // result buffer.
        let adapter_name = unsafe { narrow_string(adapter.AdapterName) };
        // SAFETY: same lifetime guarantee as AdapterName.
        let friendly_name = unsafe { wide_string(adapter.FriendlyName) };
        // SAFETY: same lifetime guarantee as AdapterName.
        let description = unsafe { wide_string(adapter.Description) };
        // SAFETY: this union arm is the documented layout for
        // IP_ADAPTER_ADDRESSES_LH.
        let interface_index = unsafe { adapter.Anonymous1.Anonymous.IfIndex };
        adapters.push(WindowsCaptureAdapter {
            adapter_name,
            friendly_name,
            description,
            interface_index,
            ipv6_interface_index: adapter.Ipv6IfIndex,
            interface_type: adapter.IfType,
            physical_address: adapter.PhysicalAddress[..usize::try_from(
                adapter.PhysicalAddressLength,
            )
            .unwrap_or_default()
            .min(adapter.PhysicalAddress.len())]
                .to_vec(),
            operational: adapter.OperStatus == IfOperStatusUp,
            has_gateway: !adapter.FirstGatewayAddress.is_null(),
            ipv4_metric: adapter.Ipv4Metric,
            unicast_addresses,
        });
        current = adapter.Next;
    }

    adapters.sort_by(|left, right| {
        left.interface_index
            .cmp(&right.interface_index)
            .then_with(|| left.friendly_name.cmp(&right.friendly_name))
    });
    Ok(adapters)
}

fn ensure_record_in_buffer<T>(
    record: *const c_void,
    buffer_start: usize,
    buffer_end: usize,
) -> Result<(), CaptureError> {
    let record_start = record as usize;
    let record_end = record_start
        .checked_add(size_of::<T>())
        .ok_or_else(|| adapter_table_error("adapter record bounds overflowed"))?;
    if record_start < buffer_start || record_end > buffer_end {
        return Err(adapter_table_error(
            "Windows returned an adapter record outside its buffer",
        ));
    }
    Ok(())
}

fn socket_address_to_ip(
    address: windows_sys::Win32::Networking::WinSock::SOCKET_ADDRESS,
) -> Option<IpAddr> {
    if address.lpSockaddr.is_null() {
        return None;
    }
    // SAFETY: GetAdaptersAddresses owns this SOCKADDR and supplies its family.
    let family = unsafe { (*address.lpSockaddr).sa_family };
    match family {
        AF_INET if address.iSockaddrLength as usize >= size_of::<SOCKADDR_IN>() => {
            // SAFETY: the family and reported record length match SOCKADDR_IN.
            let value = unsafe { &*address.lpSockaddr.cast::<SOCKADDR_IN>() };
            // SAFETY: reading the byte representation of IN_ADDR is valid for
            // an IPv4 sockaddr.
            let octets = unsafe { value.sin_addr.S_un.S_un_b };
            Some(IpAddr::V4(Ipv4Addr::new(
                octets.s_b1,
                octets.s_b2,
                octets.s_b3,
                octets.s_b4,
            )))
        }
        AF_INET6 if address.iSockaddrLength as usize >= size_of::<SOCKADDR_IN6>() => {
            // SAFETY: the family and reported record length match SOCKADDR_IN6.
            let value = unsafe { &*address.lpSockaddr.cast::<SOCKADDR_IN6>() };
            // SAFETY: the byte union member is the network-order IPv6 address.
            let octets = unsafe { value.sin6_addr.u.Byte };
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => None,
    }
}

unsafe fn narrow_string(value: *const u8) -> String {
    if value.is_null() {
        return String::new();
    }
    // SAFETY: the pointer is a NUL-terminated ANSI string supplied by
    // GetAdaptersAddresses for the lifetime of its output buffer.
    unsafe { CStr::from_ptr(value.cast()).to_string_lossy().into_owned() }
}

unsafe fn wide_string(value: *const u16) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut len = 0_usize;
    const MAX_WIDE_STRING_UNITS: usize = 32 * 1024;
    // SAFETY: the pointer is a NUL-terminated UTF-16 string supplied by
    // GetAdaptersAddresses. The fixed upper bound prevents runaway scanning
    // if Windows ever returns malformed metadata.
    while len < MAX_WIDE_STRING_UNITS && unsafe { *value.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: the scan above established `len` initialized UTF-16 units.
    String::from_utf16_lossy(unsafe { slice::from_raw_parts(value, len) })
}

fn query_tcp_table(address_family: u32) -> Result<Vec<usize>, CaptureError> {
    let mut required_bytes = 0_u32;
    // SAFETY: the first call deliberately supplies a null output pointer so
    // Windows reports the required buffer size in `required_bytes`.
    let status = unsafe {
        GetExtendedTcpTable(
            ptr::null_mut(),
            &mut required_bytes,
            0,
            address_family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if status != ERROR_INSUFFICIENT_BUFFER && status != NO_ERROR {
        return Err(socket_table_status(status));
    }
    if required_bytes == 0 {
        return Err(socket_table_error(
            "Windows returned an empty TCP ownership table",
        ));
    }

    for _ in 0..MAX_TABLE_QUERY_ATTEMPTS {
        let word_bytes = size_of::<usize>();
        let word_count = (required_bytes as usize)
            .checked_add(word_bytes - 1)
            .ok_or_else(|| socket_table_error("TCP ownership table size overflowed"))?
            / word_bytes;
        let mut buffer = vec![0_usize; word_count];
        let mut supplied_bytes = u32::try_from(size_of_val(buffer.as_slice()))
            .map_err(|_| socket_table_error("TCP ownership table is too large"))?;
        // SAFETY: `buffer` is writable for `supplied_bytes`, aligned to at
        // least `usize`, and remains alive for the call.
        let status = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut supplied_bytes,
                0,
                address_family,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if status == NO_ERROR {
            return Ok(buffer);
        }
        if status != ERROR_INSUFFICIENT_BUFFER {
            return Err(socket_table_status(status));
        }
        required_bytes = supplied_bytes;
    }

    Err(socket_table_error(
        "TCP ownership table kept changing during bounded retries",
    ))
}

fn checked_rows_fit(
    buffer: &[usize],
    first_row: *const c_void,
    count: usize,
    row_size: usize,
) -> Result<(), CaptureError> {
    let buffer_start = buffer.as_ptr() as usize;
    let buffer_end = buffer_start
        .checked_add(size_of_val(buffer))
        .ok_or_else(|| socket_table_error("TCP ownership table bounds overflowed"))?;
    let rows_start = first_row as usize;
    let rows_end = count
        .checked_mul(row_size)
        .and_then(|bytes| rows_start.checked_add(bytes))
        .ok_or_else(|| socket_table_error("TCP ownership row bounds overflowed"))?;
    if rows_start < buffer_start || rows_end > buffer_end {
        return Err(socket_table_error(
            "Windows returned a truncated TCP ownership table",
        ));
    }
    Ok(())
}

fn network_port(value: u32) -> u16 {
    u16::from_be(value as u16)
}

impl WindowsRouteResolver for SystemWindowsRouteResolver {
    fn interface_index(&self, destination: IpAddr) -> Option<u32> {
        let mut interface_index = 0_u32;
        let status = match destination {
            IpAddr::V4(address) => {
                let octets = address.octets();
                let socket = SOCKADDR_IN {
                    sin_family: AF_INET,
                    sin_port: 0,
                    sin_addr: IN_ADDR {
                        S_un: IN_ADDR_0 {
                            S_un_b: IN_ADDR_0_0 {
                                s_b1: octets[0],
                                s_b2: octets[1],
                                s_b3: octets[2],
                                s_b4: octets[3],
                            },
                        },
                    },
                    sin_zero: [0; 8],
                };
                // SAFETY: `socket` is a fully initialized IPv4 sockaddr and
                // `interface_index` is writable for the duration of the call.
                unsafe {
                    GetBestInterfaceEx(
                        ptr::addr_of!(socket).cast::<SOCKADDR>(),
                        &mut interface_index,
                    )
                }
            }
            IpAddr::V6(address) => {
                let socket = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_port: 0,
                    sin6_flowinfo: 0,
                    sin6_addr: IN6_ADDR {
                        u: IN6_ADDR_0 {
                            Byte: address.octets(),
                        },
                    },
                    Anonymous: Default::default(),
                };
                // SAFETY: `socket` is a fully initialized IPv6 sockaddr and
                // `interface_index` is writable for the duration of the call.
                unsafe {
                    GetBestInterfaceEx(
                        ptr::addr_of!(socket).cast::<SOCKADDR>(),
                        &mut interface_index,
                    )
                }
            }
        };
        (status == NO_ERROR && interface_index != 0).then_some(interface_index)
    }
}

fn usable_candidate_connection(connection: TcpConnection) -> bool {
    connection.client.port > 0
        && connection.server.port > 0
        && !connection.client.address.is_unspecified()
        && !connection.server.address.is_unspecified()
}

fn usable_remote_connection(connection: TcpConnection) -> bool {
    usable_candidate_connection(connection)
        && !connection.client.address.is_loopback()
        && !connection.server.address.is_loopback()
}

fn socket_table_status(status: u32) -> CaptureError {
    socket_table_error(format!(
        "GetExtendedTcpTable failed with Windows status {status}"
    ))
}

fn socket_table_error(message: impl Into<String>) -> CaptureError {
    CaptureError::Adapter {
        adapter: "windows-process-socket-owner".into(),
        message: message.into(),
    }
}

fn adapter_table_status(status: u32) -> CaptureError {
    adapter_table_error(format!(
        "GetAdaptersAddresses failed with Windows status {status}"
    ))
}

fn adapter_table_error(message: impl Into<String>) -> CaptureError {
    CaptureError::Adapter {
        adapter: "windows-capture-adapter-discovery".into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct FixtureRoutes(BTreeMap<IpAddr, u32>);

    impl WindowsRouteResolver for FixtureRoutes {
        fn interface_index(&self, destination: IpAddr) -> Option<u32> {
            self.0.get(&destination).copied()
        }
    }

    fn adapter(
        name: &str,
        interface_index: u32,
        metric: u32,
        address: [u8; 4],
    ) -> WindowsCaptureAdapter {
        WindowsCaptureAdapter {
            adapter_name: name.into(),
            friendly_name: name.into(),
            description: String::new(),
            interface_index,
            ipv6_interface_index: interface_index,
            interface_type: 6,
            physical_address: Vec::new(),
            operational: true,
            has_gateway: true,
            ipv4_metric: metric,
            unicast_addresses: vec![IpAddr::V4(Ipv4Addr::from(address))],
        }
    }

    fn connection(client: [u8; 4], server: [u8; 4]) -> TcpConnection {
        TcpConnection::new(
            TcpEndpoint::new(IpAddr::V4(Ipv4Addr::from(client)), 50_000),
            TcpEndpoint::new(IpAddr::V4(Ipv4Addr::from(server)), 443),
        )
    }

    fn ipv6_connection(client: &str, server: &str) -> TcpConnection {
        TcpConnection::new(
            TcpEndpoint::new(client.parse().expect("fixture IPv6 client"), 50_000),
            TcpEndpoint::new(server.parse().expect("fixture IPv6 server"), 443),
        )
    }

    #[test]
    fn candidate_plan_prioritizes_explicit_then_direct_game_traffic() {
        let adapters = vec![
            adapter("{PRIMARY}", 1, 5, [192, 0, 2, 10]),
            adapter("{GAME}", 2, 25, [198, 51, 100, 10]),
        ];
        let plan = plan_windows_capture_candidates(
            &adapters,
            &[connection([198, 51, 100, 10], [203, 0, 113, 8])],
            Some(r"\Device\NPF_{PRIMARY}"),
            &FixtureRoutes::default(),
        );

        assert_eq!(plan[0].adapter_name, "{PRIMARY}");
        assert_eq!(
            plan[0].sources,
            vec![
                WindowsCaptureCandidateSource::ExplicitPrimary,
                WindowsCaptureCandidateSource::SystemRoute,
            ]
        );
        assert_eq!(plan[1].adapter_name, "{GAME}");
        assert_eq!(plan[1].matched_game_connections, 1);
        assert!(
            plan[1]
                .sources
                .contains(&WindowsCaptureCandidateSource::GameSocketLocalAddress)
        );
    }

    #[test]
    fn exitlag_style_loopback_evidence_adds_only_bounded_loopback_probation() {
        let adapters = vec![adapter("{ETHERNET}", 8, 10, [192, 0, 2, 10])];
        let routes = FixtureRoutes(BTreeMap::from([(
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)),
            8,
        )]));
        let loopback = connection([127, 0, 0, 1], [127, 0, 0, 2]);
        let plan = plan_windows_capture_candidates(&adapters, &[loopback], None, &routes);

        assert_eq!(plan[0].adapter_name, NPCAP_LOOPBACK_ADAPTER_NAME);
        assert_eq!(
            plan[0].sources,
            vec![WindowsCaptureCandidateSource::LoopbackProbation]
        );
        assert_eq!(plan[1].adapter_name, "{ETHERNET}");
        assert!(
            plan[1]
                .sources
                .contains(&WindowsCaptureCandidateSource::SystemRoute)
        );
    }

    #[test]
    fn mixed_direct_and_loopback_connections_keep_special_loopback_candidate() {
        let adapters = vec![adapter("{ETHERNET}", 8, 10, [192, 0, 2, 10])];
        let connections = [
            connection([192, 0, 2, 10], [203, 0, 113, 9]),
            connection([127, 0, 0, 1], [127, 0, 0, 2]),
        ];
        let plan = plan_windows_capture_candidates(
            &adapters,
            &connections,
            None,
            &FixtureRoutes::default(),
        );

        assert_eq!(plan[0].adapter_name, "{ETHERNET}");
        assert_eq!(plan[1].adapter_name, NPCAP_LOOPBACK_ADAPTER_NAME);
        assert_eq!(
            plan[1].sources,
            vec![WindowsCaptureCandidateSource::LoopbackProbation]
        );
    }

    #[test]
    fn enumerated_software_loopback_does_not_replace_npcap_loopback() {
        let mut enumerated_loopback = adapter("{WINDOWS-LOOPBACK}", 7, 1, [127, 0, 0, 1]);
        enumerated_loopback.interface_type = IF_TYPE_SOFTWARE_LOOPBACK;
        enumerated_loopback.has_gateway = false;
        let physical = adapter("{ETHERNET}", 8, 10, [192, 0, 2, 10]);
        let loopback = connection([127, 0, 0, 1], [127, 0, 0, 2]);
        let plan = plan_windows_capture_candidates(
            &[enumerated_loopback, physical],
            &[loopback],
            None,
            &FixtureRoutes::default(),
        );

        assert_eq!(plan[0].adapter_name, NPCAP_LOOPBACK_ADAPTER_NAME);
        assert!(
            plan.iter()
                .all(|candidate| candidate.adapter_name != "{WINDOWS-LOOPBACK}")
        );
    }

    #[test]
    fn route_mapping_uses_only_the_destination_address_family_index() {
        let mut ipv6_route = adapter("{IPV6-ROUTE}", 7, 10, [192, 0, 2, 7]);
        ipv6_route.ipv6_interface_index = 42;
        let mut ipv4_route = adapter("{IPV4-ROUTE}", 42, 20, [192, 0, 2, 42]);
        ipv4_route.ipv6_interface_index = 7;
        let ipv4_remote = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9));
        let ipv6_remote: IpAddr = "2001:db8::9".parse().unwrap();
        let routes = FixtureRoutes(BTreeMap::from([(ipv4_remote, 42), (ipv6_remote, 42)]));
        let connections = [
            connection([198, 51, 100, 5], [203, 0, 113, 9]),
            ipv6_connection("2001:db8:1::5", "2001:db8::9"),
        ];
        let expected = plan_windows_capture_candidates(
            &[ipv6_route.clone(), ipv4_route.clone()],
            &connections,
            None,
            &routes,
        );
        let reversed =
            plan_windows_capture_candidates(&[ipv4_route, ipv6_route], &connections, None, &routes);

        assert_eq!(expected, reversed);
        assert_eq!(expected[0].adapter_name, "{IPV4-ROUTE}");
        assert_eq!(expected[1].adapter_name, "{IPV6-ROUTE}");
        assert!(
            expected[0]
                .sources
                .contains(&WindowsCaptureCandidateSource::GameSocketRoute)
        );
        assert!(
            expected[1]
                .sources
                .contains(&WindowsCaptureCandidateSource::GameSocketRoute)
        );
    }

    #[test]
    fn route_and_system_reasons_deduplicate_on_one_adapter() {
        let adapters = vec![adapter("{TUNNEL}", 12, 5, [10, 0, 0, 2])];
        let remote = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9));
        let routes = FixtureRoutes(BTreeMap::from([(remote, 12)]));
        let plan = plan_windows_capture_candidates(
            &adapters,
            &[connection([198, 51, 100, 5], [203, 0, 113, 9])],
            Some(r"\Device\NPF_{TUNNEL}"),
            &routes,
        );

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].adapter_name, "{TUNNEL}");
        assert_eq!(
            plan[0].sources,
            vec![
                WindowsCaptureCandidateSource::ExplicitPrimary,
                WindowsCaptureCandidateSource::GameSocketRoute,
                WindowsCaptureCandidateSource::SystemRoute,
            ]
        );
    }

    #[test]
    fn candidate_plan_is_deterministic_and_capped_at_four() {
        let adapters = (1_u8..=6)
            .rev()
            .map(|index| {
                adapter(
                    &format!("{{ADAPTER-{index}}}"),
                    u32::from(index),
                    u32::from(index),
                    [10, 0, 0, index],
                )
            })
            .collect::<Vec<_>>();
        let connections = (1_u8..=6)
            .map(|index| connection([10, 0, 0, index], [203, 0, 113, index]))
            .collect::<Vec<_>>();
        let first = plan_windows_capture_candidates(
            &adapters,
            &connections,
            None,
            &FixtureRoutes::default(),
        );
        let mut reversed = adapters.clone();
        reversed.reverse();
        let second = plan_windows_capture_candidates(
            &reversed,
            &connections,
            None,
            &FixtureRoutes::default(),
        );

        assert_eq!(first, second);
        assert_eq!(first.len(), MAX_WINDOWS_CAPTURE_CANDIDATES);
        assert_eq!(first[0].adapter_name, "{ADAPTER-1}");
    }

    #[test]
    fn unavailable_route_and_pid_do_not_enable_loopback_probation() {
        let adapters = vec![adapter("{ETHERNET}", 8, 10, [192, 0, 2, 10])];
        let plan = plan_windows_capture_candidates(&adapters, &[], None, &FixtureRoutes::default());

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].adapter_name, "{ETHERNET}");
        assert_eq!(
            plan[0].sources,
            vec![WindowsCaptureCandidateSource::SystemRoute]
        );
    }

    #[test]
    fn legacy_recommendation_retains_system_fallback_outside_candidate_cap() {
        let adapters = vec![adapter("{SYSTEM}", 1, 1, [192, 0, 2, 1])];
        let candidates = (0..MAX_WINDOWS_CAPTURE_CANDIDATES)
            .map(|index| WindowsCaptureCandidate {
                adapter_name: format!("{{ROUTE-{index}}}"),
                sources: vec![WindowsCaptureCandidateSource::GameSocketRoute],
                matched_game_connections: 0,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            compatibility_capture_recommendation(&adapters, &candidates),
            Some(WindowsCaptureAdapterRecommendation {
                adapter_name: "{SYSTEM}".into(),
                source: WindowsCaptureAdapterRecommendationSource::SystemRoute,
                matched_game_connections: 0,
            })
        );
    }

    #[test]
    fn windows_network_order_port_is_decoded() {
        assert_eq!(network_port(0x0000_50c3), 50_000);
        assert_eq!(network_port(0x0000_bb01), 443);
    }

    #[test]
    fn zero_and_loopback_connections_are_not_capture_candidates() {
        let remote = TcpEndpoint::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)), 443);
        assert!(!usable_remote_connection(TcpConnection::new(
            TcpEndpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50_000),
            remote,
        )));
        assert!(!usable_remote_connection(TcpConnection::new(
            TcpEndpoint::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            remote,
        )));
    }
}
