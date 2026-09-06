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
            GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, GetExtendedTcpTable,
            IP_ADAPTER_ADDRESSES_LH, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
            MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6},
};

use crate::dumpcap::DumpcapLiveCapture;
use crate::npcap::NpcapLiveCapture;
use crate::{
    CaptureError, CaptureSource, CaptureSourceMetadata, CapturedFrame, DumpcapLiveConfig,
    LiveCaptureStopHandle, NpcapLiveConfig, NpcapLiveStopHandle, OwnedProcessCapture,
    OwnedProcessCaptureConfig, OwnedProcessCaptureMetrics, ProcessSocketOwner,
    SignatureFlowCapture, SignatureFlowCaptureConfig, SignatureFlowCaptureMetrics, TcpConnection,
    TcpEndpoint, TcpPayloadSignature,
};

const MAX_TABLE_QUERY_ATTEMPTS: usize = 4;
const MAX_ADAPTER_QUERY_ATTEMPTS: usize = 4;
const MAX_ADAPTERS: usize = 512;
const MAX_UNICAST_ADDRESSES_PER_ADAPTER: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCaptureAdapter {
    /// Windows adapter identifier used inside Npcap names such as
    /// `\Device\NPF_{GUID}`.
    pub adapter_name: String,
    pub friendly_name: String,
    pub description: String,
    pub interface_index: u32,
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
    let mut address_counts = BTreeMap::<IpAddr, usize>::new();
    for process_id in process_ids.iter().copied().filter(|value| *value != 0) {
        let Ok(mut owner) = WindowsProcessSocketOwner::new(process_id) else {
            continue;
        };
        let Ok(connections) = owner.snapshot() else {
            continue;
        };
        for connection in connections {
            *address_counts.entry(connection.client.address).or_default() += 1;
        }
    }

    let matched = adapters
        .iter()
        .map(|adapter| {
            let count = adapter
                .unicast_addresses
                .iter()
                .map(|address| address_counts.get(address).copied().unwrap_or_default())
                .sum::<usize>();
            (adapter, count)
        })
        .filter(|(_, count)| *count > 0)
        .max_by_key(|(adapter, count)| {
            (
                *count,
                usize::from(adapter.operational),
                usize::from(adapter.has_gateway),
                u32::MAX - adapter.ipv4_metric,
            )
        });
    if let Some((adapter, matched_game_connections)) = matched {
        return Some(WindowsCaptureAdapterRecommendation {
            adapter_name: adapter.adapter_name.clone(),
            source: WindowsCaptureAdapterRecommendationSource::GameTraffic,
            matched_game_connections,
        });
    }

    adapters
        .iter()
        .filter(|adapter| adapter.operational && adapter.has_gateway)
        .min_by_key(|adapter| (adapter.ipv4_metric, adapter.interface_index))
        .map(|adapter| WindowsCaptureAdapterRecommendation {
            adapter_name: adapter.adapter_name.clone(),
            source: WindowsCaptureAdapterRecommendationSource::SystemRoute,
            matched_game_connections: 0,
        })
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
            if usable_remote_connection(connection) {
                connections.push(connection);
            }
        }
        Ok(connections)
    }

    fn snapshot_ipv6(&self) -> Result<Vec<TcpConnection>, CaptureError> {
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
            if usable_remote_connection(connection) {
                connections.push(connection);
            }
        }
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

fn usable_remote_connection(connection: TcpConnection) -> bool {
    connection.client.port > 0
        && connection.server.port > 0
        && !connection.client.address.is_unspecified()
        && !connection.server.address.is_unspecified()
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
