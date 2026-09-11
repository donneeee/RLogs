use std::{
    collections::BTreeMap,
    ffi::{CStr, c_void},
    mem::{size_of, size_of_val},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
    sync::{Arc, Mutex, Once},
    thread::{self, JoinHandle},
    time::Duration,
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
use crate::fan_in::{
    MultiSourceFanIn, MultiSourceFanInMetrics, MultiSourceFanInStopHandle, MultiSourceIngress,
    MultiSourcePushError, MultiSourceRegistrationLease,
};
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
    NpcapFanIn(WindowsSignatureFanInStopHandle),
    Dumpcap(LiveCaptureStopHandle),
}

impl WindowsLiveCaptureStopHandle {
    pub fn request_stop(&self) -> Result<(), CaptureError> {
        match self {
            Self::Npcap(handle) => {
                handle.request_stop();
                Ok(())
            }
            Self::NpcapFanIn(handle) => {
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

const WINDOWS_FAN_IN_QUEUE_FRAMES: usize = 512;
const WINDOWS_FAN_IN_QUEUE_BYTES: usize = 16 * 1024 * 1024;
const CAPTURE_READER_THREAD_PREFIX: &str = "rlogs-capture-reader-";
const SANITIZED_CAPTURE_READER_PANIC: &str = "rLogs capture reader stopped unexpectedly";
static CAPTURE_READER_PANIC_HOOK: Once = Once::new();

fn install_capture_reader_panic_hook() {
    CAPTURE_READER_PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let is_capture_reader = thread::current()
                .name()
                .is_some_and(is_capture_reader_thread_name);
            if is_capture_reader {
                use std::io::Write as _;
                let _ = writeln!(std::io::stderr(), "{SANITIZED_CAPTURE_READER_PANIC}");
            } else {
                previous(info);
            }
        }));
    });
}

fn is_capture_reader_thread_name(name: &str) -> bool {
    let Some(ordinal) = name.strip_prefix(CAPTURE_READER_THREAD_PREFIX) else {
        return false;
    };
    if ordinal.is_empty()
        || (ordinal.len() > 1 && ordinal.starts_with('0'))
        || !ordinal.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    ordinal
        .parse::<usize>()
        .is_ok_and(|value| value < MAX_WINDOWS_CAPTURE_CANDIDATES)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowsSignatureFanInDiagnostics {
    pub planned_candidates: usize,
    pub opened_candidates: usize,
    pub candidate_open_failures: usize,
    pub active_sources: usize,
    pub completed_sources: u64,
    pub failed_sources: u64,
    pub accepted_frames: u64,
    pub delivered_frames: u64,
    pub queue_full_rejections: u64,
    pub byte_full_rejections: u64,
    pub oversized_frame_rejections: u64,
    pub peak_queued_frames: usize,
    pub peak_queued_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct WindowsSignatureFanInStopHandle {
    fan_in: MultiSourceFanInStopHandle,
    children: Arc<DynamicChildStopRegistry<NpcapLiveStopHandle>>,
}

impl WindowsSignatureFanInStopHandle {
    pub fn request_stop(&self) {
        request_fan_in_stop(&self.fan_in, &self.children);
    }
}

trait FanInChildStop: Clone + Send + 'static {
    fn request_child_stop(&self);
}

impl FanInChildStop for NpcapLiveStopHandle {
    fn request_child_stop(&self) {
        self.request_stop();
    }
}

#[derive(Debug)]
struct DynamicChildStopRegistry<S> {
    inner: Mutex<DynamicChildStopState<S>>,
    max_children: usize,
}

#[derive(Debug)]
struct DynamicChildStopState<S> {
    stopped: bool,
    children: Vec<S>,
}

impl<S: FanInChildStop> DynamicChildStopRegistry<S> {
    fn new(max_children: usize) -> Self {
        Self {
            inner: Mutex::new(DynamicChildStopState {
                stopped: false,
                children: Vec::new(),
            }),
            max_children,
        }
    }

    fn register(&self, child: S) -> Result<(), ()> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped || state.children.len() >= self.max_children {
            drop(state);
            child.request_child_stop();
            return Err(());
        }
        state.children.push(child);
        Ok(())
    }

    fn request_stop(&self) {
        let children = {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.stopped = true;
            state.children.to_vec()
        };
        for child in children {
            child.request_child_stop();
        }
    }
}

fn request_fan_in_stop<S: FanInChildStop>(
    fan_in: &MultiSourceFanInStopHandle,
    children: &DynamicChildStopRegistry<S>,
) {
    // Refuse queue writes and registrations before waking native readers.
    fan_in.request_stop();
    children.request_stop();
}

/// Initial bounded Windows adapter fan-in protected by exactly one shared
/// protocol-signature filter. Raw frames never leave this wrapper.
#[derive(Debug)]
pub struct WindowsSignatureFanInCapture {
    inner: SignatureFlowCapture<MultiSourceFanIn>,
    stop: WindowsSignatureFanInStopHandle,
    workers: Vec<JoinHandle<()>>,
    planned_candidates: usize,
    opened_candidates: usize,
    candidate_open_failures: usize,
}

trait FanInCandidateOpener {
    type Source: CaptureSource + 'static;
    type Stop: FanInChildStop;

    fn open(
        &mut self,
        candidate: &WindowsCaptureCandidate,
        duration_seconds: u32,
    ) -> Result<(Self::Source, Self::Stop), CaptureError>;
}

struct NpcapCandidateOpener;

impl FanInCandidateOpener for NpcapCandidateOpener {
    type Source = NpcapLiveCapture;
    type Stop = NpcapLiveStopHandle;

    fn open(
        &mut self,
        candidate: &WindowsCaptureCandidate,
        duration_seconds: u32,
    ) -> Result<(Self::Source, Self::Stop), CaptureError> {
        let source = NpcapLiveConfig::new(
            crate::npcap_device_name(&candidate.adapter_name),
            duration_seconds,
        )
        .and_then(NpcapLiveCapture::open)?;
        let stop = source.stop_handle();
        Ok((source, stop))
    }
}

struct StartedFanInSources<S> {
    stops: Vec<S>,
    workers: Vec<JoinHandle<()>>,
    open_failures: usize,
    first_open_error: Option<CaptureError>,
}

fn start_fan_in_sources<O: FanInCandidateOpener>(
    candidates: &[WindowsCaptureCandidate],
    duration_seconds: u32,
    registration: &MultiSourceRegistrationLease,
    opener: &mut O,
) -> Result<StartedFanInSources<O::Stop>, CaptureError> {
    install_capture_reader_panic_hook();
    let mut stops = Vec::new();
    let mut workers = Vec::new();
    let mut open_failures = 0_usize;
    let mut first_open_error = None;

    for (index, candidate) in candidates
        .iter()
        .take(MAX_WINDOWS_CAPTURE_CANDIDATES)
        .enumerate()
    {
        let (source, stop) = match opener.open(candidate, duration_seconds) {
            Ok(opened) => opened,
            Err(error) => {
                open_failures = open_failures.saturating_add(1);
                first_open_error.get_or_insert(error);
                continue;
            }
        };
        let ingress = registration
            .register_source()
            .map_err(|error| CaptureError::Adapter {
                adapter: "windows-signature-fan-in".into(),
                message: format!("could not register a bounded Npcap candidate: {error}"),
            })?;
        match thread::Builder::new()
            .name(format!("{CAPTURE_READER_THREAD_PREFIX}{index}"))
            .spawn(move || run_fan_in_source_guarded(source, ingress))
        {
            Ok(worker) => {
                stops.push(stop);
                workers.push(worker);
            }
            Err(error) => {
                open_failures = open_failures.saturating_add(1);
                first_open_error.get_or_insert_with(|| CaptureError::Adapter {
                    adapter: "windows-signature-fan-in".into(),
                    message: format!("could not start an Npcap candidate reader: {error}"),
                });
            }
        }
    }

    Ok(StartedFanInSources {
        stops,
        workers,
        open_failures,
        first_open_error,
    })
}

impl WindowsSignatureFanInCapture {
    fn open_prefix(
        candidates: &[WindowsCaptureCandidate],
        duration_seconds: u32,
        signature: TcpPayloadPrefixSignature,
        filter: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let (fan_in, registration) = MultiSourceFanIn::new_with_registration_lease(
            MAX_WINDOWS_CAPTURE_CANDIDATES,
            WINDOWS_FAN_IN_QUEUE_FRAMES,
            WINDOWS_FAN_IN_QUEUE_BYTES,
            Vec::new(),
        )?;
        let aggregate_stop = fan_in.stop_handle();
        // Validate and establish the one shared privacy boundary before any
        // candidate reader can observe a frame.
        let inner = SignatureFlowCapture::new_prefix(fan_in, signature, filter)?;
        let started = start_fan_in_sources(
            candidates,
            duration_seconds,
            &registration,
            &mut NpcapCandidateOpener,
        )?;
        // This initial-only runtime has no coordinator yet, so seal dynamic
        // registration immediately after the bounded candidate set starts.
        registration.close();

        if started.workers.is_empty() {
            aggregate_stop.request_stop();
            return Err(started
                .first_open_error
                .unwrap_or_else(|| CaptureError::Adapter {
                    adapter: "windows-signature-fan-in".into(),
                    message: "no Npcap capture candidate could be opened".into(),
                }));
        }
        let opened_candidates = started.workers.len();
        let children = Arc::new(DynamicChildStopRegistry::new(
            MAX_WINDOWS_CAPTURE_CANDIDATES,
        ));
        for child in started.stops {
            children
                .register(child)
                .map_err(|()| CaptureError::Adapter {
                    adapter: "windows-signature-fan-in".into(),
                    message: "could not retain a bounded Npcap stop handle".into(),
                })?;
        }
        Ok(Self {
            inner,
            stop: WindowsSignatureFanInStopHandle {
                fan_in: aggregate_stop,
                children,
            },
            workers: started.workers,
            planned_candidates: candidates.len().min(MAX_WINDOWS_CAPTURE_CANDIDATES),
            opened_candidates,
            candidate_open_failures: started.open_failures,
        })
    }

    pub fn metrics(&self) -> &SignatureFlowCaptureMetrics {
        self.inner.metrics()
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        self.inner.confirmed_connections()
    }

    pub fn stop_handle(&self) -> WindowsSignatureFanInStopHandle {
        self.stop.clone()
    }

    pub fn diagnostics(&self) -> WindowsSignatureFanInDiagnostics {
        let metrics = self.inner.source().metrics();
        fan_in_diagnostics(
            self.planned_candidates,
            self.opened_candidates,
            self.candidate_open_failures,
            metrics,
        )
    }

    fn finish_workers(&mut self) {
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl CaptureSource for WindowsSignatureFanInCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        self.inner.metadata()
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        let result = self.inner.next_frame();
        if !matches!(result, Ok(Some(_))) {
            self.finish_workers();
        }
        result
    }
}

impl Drop for WindowsSignatureFanInCapture {
    fn drop(&mut self) {
        self.stop.request_stop();
        self.finish_workers();
    }
}

fn run_fan_in_source<S: CaptureSource>(mut source: S, ingress: &mut MultiSourceIngress) {
    loop {
        let frame = match source.next_frame() {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                ingress.finish();
                return;
            }
            Err(_) => {
                ingress.fail();
                return;
            }
        };
        let mut pending = frame;
        loop {
            match ingress.try_push(pending) {
                Ok(()) => break,
                Err(MultiSourcePushError::QueueFull(frame))
                | Err(MultiSourcePushError::QueueBytesFull(frame)) => {
                    pending = frame;
                    if ingress.stop_requested() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(MultiSourcePushError::Stopped(_)) => return,
                Err(MultiSourcePushError::FrameTooLarge(_))
                | Err(MultiSourcePushError::SequenceExhausted(_)) => {
                    ingress.fail();
                    return;
                }
            }
        }
    }
}

fn run_fan_in_source_guarded<S: CaptureSource>(source: S, mut ingress: MultiSourceIngress) {
    if catch_unwind(AssertUnwindSafe(|| run_fan_in_source(source, &mut ingress))).is_err() {
        // The panic payload is intentionally neither retained nor exposed: a
        // worker panic is only a bounded, privacy-safe failed-source signal.
        ingress.fail();
    }
}

fn fan_in_diagnostics(
    planned_candidates: usize,
    opened_candidates: usize,
    candidate_open_failures: usize,
    metrics: MultiSourceFanInMetrics,
) -> WindowsSignatureFanInDiagnostics {
    WindowsSignatureFanInDiagnostics {
        planned_candidates,
        opened_candidates,
        candidate_open_failures,
        active_sources: metrics.active_sources,
        completed_sources: metrics.completed_sources,
        failed_sources: metrics.failed_sources,
        accepted_frames: metrics.accepted_frames,
        delivered_frames: metrics.delivered_frames,
        queue_full_rejections: metrics.queue_full_rejections,
        byte_full_rejections: metrics.byte_full_rejections,
        oversized_frame_rejections: metrics.oversized_frame_rejections,
        peak_queued_frames: metrics.peak_queued_frames,
        peak_queued_bytes: metrics.peak_queued_bytes,
    }
}

/// Packet-first Windows live capture. It opens broad TCP ingress in memory,
/// then exposes only exact connections proven by the supplied game signature.
#[derive(Debug)]
pub enum WindowsSignatureLiveCapture {
    Npcap(WindowsSignatureNpcapCapture),
    NpcapFanIn(WindowsSignatureFanInCapture),
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

    /// Opens the initial route-aware candidate set under one shared signature
    /// filter. Candidate discovery is point-in-time in this slice; no setting is
    /// changed and no adapter is added after the call returns.
    pub fn open_route_aware_prefix(
        primary_interface: &str,
        process_ids: &[u32],
        duration_seconds: u32,
        dumpcap_fallback: Option<DumpcapLiveConfig>,
        signature: TcpPayloadPrefixSignature,
        filter: SignatureFlowCaptureConfig,
    ) -> Result<Self, CaptureError> {
        let adapters = windows_capture_adapters().unwrap_or_default();
        let candidates =
            recommend_windows_capture_candidates(&adapters, process_ids, Some(primary_interface));
        match WindowsSignatureFanInCapture::open_prefix(
            &candidates,
            duration_seconds,
            signature,
            filter,
        ) {
            Ok(capture) => Ok(Self::NpcapFanIn(capture)),
            Err(npcap_error) => match dumpcap_fallback {
                Some(config) => WindowsSignatureDumpcapCapture::spawn_prefix(
                    config, signature, filter,
                )
                .map(Self::Dumpcap)
                .map_err(|dumpcap_error| CaptureError::Adapter {
                    adapter: "windows-signature-live-capture".into(),
                    message: format!(
                        "all bounded native Npcap candidates failed ({npcap_error}); dumpcap fallback also failed ({dumpcap_error})"
                    ),
                }),
                None => Err(npcap_error),
            },
        }
    }

    pub fn metrics(&self) -> &SignatureFlowCaptureMetrics {
        match self {
            Self::Npcap(capture) => capture.metrics(),
            Self::NpcapFanIn(capture) => capture.metrics(),
            Self::Dumpcap(capture) => capture.metrics(),
        }
    }

    pub fn confirmed_connections(&self) -> Vec<TcpConnection> {
        match self {
            Self::Npcap(capture) => capture.confirmed_connections(),
            Self::NpcapFanIn(capture) => capture.confirmed_connections(),
            Self::Dumpcap(capture) => capture.confirmed_connections(),
        }
    }

    pub fn stop_handle(&self) -> WindowsLiveCaptureStopHandle {
        match self {
            Self::Npcap(capture) => WindowsLiveCaptureStopHandle::Npcap(capture.stop_handle()),
            Self::NpcapFanIn(capture) => {
                WindowsLiveCaptureStopHandle::NpcapFanIn(capture.stop_handle())
            }
            Self::Dumpcap(capture) => WindowsLiveCaptureStopHandle::Dumpcap(capture.stop_handle()),
        }
    }
}

impl CaptureSource for WindowsSignatureLiveCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        match self {
            Self::Npcap(capture) => capture.metadata(),
            Self::NpcapFanIn(capture) => capture.metadata(),
            Self::Dumpcap(capture) => capture.metadata(),
        }
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        match self {
            Self::Npcap(capture) => capture.next_frame(),
            Self::NpcapFanIn(capture) => capture.next_frame(),
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
    use std::{
        collections::VecDeque,
        env,
        process::Command,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    use bytes::Bytes;
    use etherparse::PacketBuilder;

    use super::*;
    use crate::{
        CaptureLinkType, CaptureSourceKind, TcpPayloadDirection, TcpPayloadSignatureResult,
        TimestampNormalization,
    };

    #[derive(Debug, Default)]
    struct FixtureRoutes(BTreeMap<IpAddr, u32>);

    impl WindowsRouteResolver for FixtureRoutes {
        fn interface_index(&self, destination: IpAddr) -> Option<u32> {
            self.0.get(&destination).copied()
        }
    }

    #[derive(Debug)]
    struct FixtureCaptureSource {
        metadata: CaptureSourceMetadata,
        frames: VecDeque<Result<Option<CapturedFrame>, CaptureError>>,
        panic_on_next: bool,
    }

    impl FixtureCaptureSource {
        fn empty() -> Self {
            Self {
                metadata: CaptureSourceMetadata {
                    source_id: "fixture".into(),
                    display_name: "fixture".into(),
                    kind: CaptureSourceKind::Live,
                    link_types: vec![CaptureLinkType::Ethernet],
                    file_format: None,
                },
                frames: VecDeque::from([Ok(None)]),
                panic_on_next: false,
            }
        }

        fn failed() -> Self {
            let mut source = Self::empty();
            source.frames = VecDeque::from([Err(CaptureError::Adapter {
                adapter: "fixture".into(),
                message: "fixture read failure".into(),
            })]);
            source
        }

        fn panicking() -> Self {
            let mut source = Self::empty();
            source.panic_on_next = true;
            source
        }

        fn matching() -> Self {
            let mut source = Self::empty();
            source.frames = VecDeque::from([
                Ok(Some(tcp_frame(10_000, b"BPSR split signature"))),
                Ok(None),
            ]);
            source
        }
    }

    impl CaptureSource for FixtureCaptureSource {
        fn metadata(&self) -> &CaptureSourceMetadata {
            &self.metadata
        }

        fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
            if self.panic_on_next {
                self.panic_on_next = false;
                panic!("private fixture panic payload");
            }
            self.frames.pop_front().unwrap_or(Ok(None))
        }
    }

    #[derive(Debug, Clone)]
    struct FixtureStop(Arc<AtomicBool>);

    impl FanInChildStop for FixtureStop {
        fn request_child_stop(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[derive(Debug, Default)]
    struct FixtureOpener {
        attempts: Vec<String>,
    }

    impl FanInCandidateOpener for FixtureOpener {
        type Source = FixtureCaptureSource;
        type Stop = FixtureStop;

        fn open(
            &mut self,
            candidate: &WindowsCaptureCandidate,
            _duration_seconds: u32,
        ) -> Result<(Self::Source, Self::Stop), CaptureError> {
            self.attempts.push(candidate.adapter_name.clone());
            if candidate.adapter_name.starts_with("FAIL") {
                return Err(CaptureError::Adapter {
                    adapter: "fixture".into(),
                    message: "fixture open failure".into(),
                });
            }
            Ok((
                if candidate.adapter_name.starts_with("PANIC") {
                    FixtureCaptureSource::panicking()
                } else if candidate.adapter_name.starts_with("HEALTHY") {
                    FixtureCaptureSource::matching()
                } else if candidate.adapter_name.starts_with("RUNTIME-FAIL") {
                    FixtureCaptureSource::failed()
                } else {
                    FixtureCaptureSource::empty()
                },
                FixtureStop(Arc::new(AtomicBool::new(false))),
            ))
        }
    }

    fn candidate(name: &str) -> WindowsCaptureCandidate {
        WindowsCaptureCandidate {
            adapter_name: name.into(),
            sources: vec![WindowsCaptureCandidateSource::SystemRoute],
            matched_game_connections: 0,
        }
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

    fn tcp_frame(tcp_sequence: u32, payload: &[u8]) -> CapturedFrame {
        tcp_frame_for_ports(32_000, 31_000, tcp_sequence, payload)
    }

    fn tcp_frame_for_ports(
        source_port: u16,
        destination_port: u16,
        tcp_sequence: u32,
        payload: &[u8],
    ) -> CapturedFrame {
        let builder = PacketBuilder::ethernet2([1; 6], [2; 6])
            .ipv4([10, 0, 0, 2], [10, 0, 0, 1], 64)
            .tcp(source_port, destination_port, tcp_sequence, 1_024);
        let mut bytes = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut bytes, payload).unwrap();
        CapturedFrame {
            sequence: 1,
            observed_micros: 1,
            source_timestamp_nanos: Some(1_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: None,
            link_type: CaptureLinkType::Ethernet,
            original_length: bytes.len().try_into().unwrap(),
            bytes: Bytes::from(bytes),
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
    fn fan_in_candidate_opener_is_bounded_and_isolates_partial_open_failures() {
        let candidates = [
            candidate("A"),
            candidate("FAIL-B"),
            candidate("C"),
            candidate("FAIL-D"),
            candidate("NEVER-OPENED"),
        ];
        let (fan_in, registration) =
            MultiSourceFanIn::new_with_registration_lease(4, 8, 4_096, Vec::new()).unwrap();
        let _filtered = SignatureFlowCapture::new_prefix(
            fan_in,
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();
        let mut opener = FixtureOpener::default();
        let started = start_fan_in_sources(&candidates, 30, &registration, &mut opener).unwrap();
        registration.close();

        for worker in started.workers {
            worker.join().unwrap();
        }
        assert_eq!(opener.attempts, ["A", "FAIL-B", "C", "FAIL-D"]);
        assert_eq!(started.stops.len(), 2);
        assert_eq!(started.open_failures, 2);
        assert!(started.first_open_error.is_some());
    }

    #[test]
    fn shared_signature_filter_can_confirm_a_prefix_split_across_adapters() {
        const SIGNATURE: &[u8] = b"BPSR split signature";
        let fan_in = MultiSourceFanIn::new(4, 8, 16 * 1_024, Vec::new()).unwrap();
        let mut first = fan_in.register_source().unwrap();
        let mut second = fan_in.register_source().unwrap();
        let mut unmatched = fan_in.register_source().unwrap();
        let mut filtered = SignatureFlowCapture::new_prefix(
            fan_in,
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();

        first.try_push(tcp_frame(10_000, &SIGNATURE[..7])).unwrap();
        second.try_push(tcp_frame(10_007, &SIGNATURE[7..])).unwrap();
        unmatched
            .try_push(tcp_frame_for_ports(42_000, 41_000, 20_000, b"private"))
            .unwrap();
        first.finish();
        second.finish();
        unmatched.finish();

        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_none());
        assert_eq!(filtered.metrics().emitted_frames, 2);
        assert_eq!(filtered.metrics().unidentified_frames_discarded, 1);
        assert_eq!(filtered.confirmed_connections().len(), 1);
    }

    #[test]
    fn fan_in_candidate_workers_isolate_one_child_read_failure() {
        let candidates = [candidate("RUNTIME-FAIL-A"), candidate("B")];
        let (fan_in, registration) =
            MultiSourceFanIn::new_with_registration_lease(4, 8, 4_096, Vec::new()).unwrap();
        let filtered = SignatureFlowCapture::new_prefix(
            fan_in,
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();
        let mut opener = FixtureOpener::default();
        let started = start_fan_in_sources(&candidates, 30, &registration, &mut opener).unwrap();
        registration.close();

        for worker in started.workers {
            worker.join().unwrap();
        }
        let metrics = filtered.source().metrics();
        assert_eq!(metrics.failed_sources, 1);
        assert_eq!(metrics.completed_sources, 1);
        assert_eq!(metrics.active_sources, 0);
    }

    #[test]
    fn panicking_worker_is_failed_while_healthy_peer_frames_continue() {
        let candidates = [candidate("PANIC-A"), candidate("HEALTHY-B")];
        let (fan_in, registration) =
            MultiSourceFanIn::new_with_registration_lease(4, 8, 4_096, Vec::new()).unwrap();
        let mut filtered = SignatureFlowCapture::new_prefix(
            fan_in,
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();
        let mut opener = FixtureOpener::default();
        let started = start_fan_in_sources(&candidates, 30, &registration, &mut opener).unwrap();
        registration.close();

        assert!(filtered.next_frame().unwrap().is_some());
        assert!(filtered.next_frame().unwrap().is_none());
        for worker in started.workers {
            assert!(worker.join().is_ok());
        }
        let metrics = filtered.source().metrics();
        assert_eq!(metrics.failed_sources, 1);
        assert_eq!(metrics.completed_sources, 1);
    }

    #[test]
    fn capture_reader_hook_name_scope_is_exact_and_bounded() {
        for ordinal in 0..MAX_WINDOWS_CAPTURE_CANDIDATES {
            assert!(is_capture_reader_thread_name(&format!(
                "{CAPTURE_READER_THREAD_PREFIX}{ordinal}"
            )));
        }
        for name in [
            "rlogs-capture-reader-4",
            "rlogs-capture-reader-999",
            "rlogs-capture-reader-debug",
            "rlogs-capture-reader-0-extra",
            "rlogs-capture-reader-00",
            "unrelated-fixture-thread",
        ] {
            assert!(!is_capture_reader_thread_name(name), "{name}");
        }
    }

    #[test]
    fn capture_reader_panic_hook_sanitizes_only_capture_threads_in_subprocess() {
        const CHILD_ENV: &str = "RLOGS_CAPTURE_PANIC_HOOK_CHILD";
        const PRIOR_HOOK_SENTINEL: &str = "fixture prior hook reached";
        if env::var_os(CHILD_ENV).is_some() {
            std::panic::set_hook(Box::new(|info| {
                use std::io::Write as _;
                let payload = info
                    .payload()
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic payload");
                let _ = writeln!(std::io::stderr(), "{PRIOR_HOOK_SENTINEL}: {payload}");
            }));
            install_capture_reader_panic_hook();

            let fan_in = MultiSourceFanIn::new(1, 2, 4_096, Vec::new()).unwrap();
            let ingress = fan_in.register_source().unwrap();
            let capture_worker = thread::Builder::new()
                .name(format!("{CAPTURE_READER_THREAD_PREFIX}0"))
                .spawn(move || {
                    run_fan_in_source_guarded(FixtureCaptureSource::panicking(), ingress)
                })
                .unwrap();
            assert!(capture_worker.join().is_ok());

            for name in [
                "rlogs-capture-reader-4",
                "rlogs-capture-reader-999",
                "rlogs-capture-reader-debug",
                "rlogs-capture-reader-0-extra",
                "rlogs-capture-reader-00",
            ] {
                let payload = format!("forwarded private panic for {name}");
                let unrelated_worker = thread::Builder::new()
                    .name(name.into())
                    .spawn(move || panic!("{payload}"))
                    .unwrap();
                assert!(unrelated_worker.join().is_err());
            }
            return;
        }

        let output = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "windows::tests::capture_reader_panic_hook_sanitizes_only_capture_threads_in_subprocess",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child test failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(SANITIZED_CAPTURE_READER_PANIC));
        assert!(stderr.contains(PRIOR_HOOK_SENTINEL));
        assert!(!stderr.contains("private fixture panic payload"));
        for name in [
            "rlogs-capture-reader-4",
            "rlogs-capture-reader-999",
            "rlogs-capture-reader-debug",
            "rlogs-capture-reader-0-extra",
            "rlogs-capture-reader-00",
        ] {
            assert!(stderr.contains(&format!("forwarded private panic for {name}")));
        }
    }

    #[test]
    fn all_panicking_workers_return_terminal_error_without_panic_details() {
        let candidates = [candidate("PANIC-A"), candidate("PANIC-B")];
        let (fan_in, registration) =
            MultiSourceFanIn::new_with_registration_lease(4, 8, 4_096, Vec::new()).unwrap();
        let mut filtered = SignatureFlowCapture::new_prefix(
            fan_in,
            prefix_signature,
            SignatureFlowCaptureConfig::default(),
        )
        .unwrap();
        let mut opener = FixtureOpener::default();
        let started = start_fan_in_sources(&candidates, 30, &registration, &mut opener).unwrap();
        registration.close();

        let error = filtered.next_frame().unwrap_err();
        assert!(!error.to_string().contains("private fixture panic payload"));
        assert!(filtered.next_frame().unwrap().is_none());
        for worker in started.workers {
            assert!(worker.join().is_ok());
        }
        assert_eq!(filtered.source().metrics().failed_sources, 2);
    }

    #[test]
    fn all_failed_children_return_one_terminal_error_after_the_queue_drains() {
        let mut fan_in = MultiSourceFanIn::new(2, 8, 4_096, Vec::new()).unwrap();
        let mut first = fan_in.register_source().unwrap();
        let mut second = fan_in.register_source().unwrap();
        first.try_push(tcp_frame(10_000, b"private")).unwrap();
        first.fail();
        second.fail();

        assert!(fan_in.next_frame().unwrap().is_some());
        assert!(fan_in.next_frame().is_err());
        assert!(fan_in.next_frame().unwrap().is_none());
    }

    #[test]
    fn aggregate_stop_precedes_child_stop_and_refuses_late_sources() {
        let fan_in = MultiSourceFanIn::new(2, 8, 4_096, Vec::new()).unwrap();
        let aggregate_stop = fan_in.stop_handle();
        let flags = [
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        ];
        let children = DynamicChildStopRegistry::new(2);
        for flag in &flags {
            children.register(FixtureStop(Arc::clone(flag))).unwrap();
        }

        request_fan_in_stop(&aggregate_stop, &children);

        assert!(fan_in.register_source().is_err());
        assert!(flags.iter().all(|flag| flag.load(Ordering::SeqCst)));
    }

    #[test]
    fn cloned_stop_registry_observes_late_children_and_refuses_after_stop() {
        let registry = Arc::new(DynamicChildStopRegistry::new(2));
        let cloned_stop_view = Arc::clone(&registry);
        let late_flag = Arc::new(AtomicBool::new(false));
        registry
            .register(FixtureStop(Arc::clone(&late_flag)))
            .unwrap();

        cloned_stop_view.request_stop();
        assert!(late_flag.load(Ordering::SeqCst));

        let refused_flag = Arc::new(AtomicBool::new(false));
        assert!(
            registry
                .register(FixtureStop(Arc::clone(&refused_flag)))
                .is_err()
        );
        assert!(refused_flag.load(Ordering::SeqCst));
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
